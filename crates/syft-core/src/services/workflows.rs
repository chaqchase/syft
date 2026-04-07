use std::path::Path;

use anyhow::{Result, bail};
use syft_git::{
    capture_worktree_snapshot, export_snapshot_to_git_commit, import_git_commit,
    materialize_snapshot_to,
};
use syft_objects::{diff_snapshot_indices, snapshot_index};
use syft_semantic::diff_snapshots;
use syft_store::MetadataStore;
use syft_types::{
    ChangeNode, ChangeNodeStatus, PromotionRecord, Snapshot, Task, TaskStatus, ValidationPlan,
    ValidationStatus, new_entity_id, now_utc,
};
use syft_validate::ValidationRunner;

use crate::app::SyftApp;
use crate::contracts::{
    ChangeService, CreateTaskInput, PromoteChangeInput, ProposeChangeInput, RepoService,
    TaskService,
};
use crate::helpers::{calculate_risk, default_provenance};

impl RepoService for SyftApp {
    fn import_git_commit(&self, commit: &str) -> Result<Snapshot> {
        let parent_snapshot_ids = self
            .current_head_snapshot_id()?
            .map(|id| vec![id])
            .unwrap_or_default();
        let (snapshot, _) = import_git_commit(
            &self.repo_path,
            &self.repo_config,
            commit,
            &self.object_store,
            parent_snapshot_ids,
        )?;
        self.metadata_store.create_snapshot(&snapshot)?;
        self.persist_head(&snapshot.id)?;
        Ok(snapshot)
    }

    fn capture_snapshot(&self) -> Result<Snapshot> {
        let mut capture_config = self.repo_config.clone();
        capture_config.capture_excludes = self.effective_capture_excludes()?;
        let parent_snapshot_ids = self
            .current_head_snapshot_id()?
            .map(|id| vec![id])
            .unwrap_or_default();
        let (snapshot, _) = capture_worktree_snapshot(
            &self.repo_path,
            &capture_config,
            &self.object_store,
            parent_snapshot_ids,
        )?;
        self.metadata_store.create_snapshot(&snapshot)?;
        Ok(snapshot)
    }

    fn materialize_snapshot(&self, snapshot_id: &str, destination: &Path) -> Result<()> {
        let snapshot = self.get_snapshot(snapshot_id)?;
        materialize_snapshot_to(&snapshot.root_tree_hash, destination, &self.object_store)
    }
}

impl TaskService for SyftApp {
    fn create_task(&self, input: CreateTaskInput) -> Result<Task> {
        let now = now_utc();
        let task = Task {
            id: new_entity_id(),
            repo_id: self.repo_config.repo_id.clone(),
            title: input.title,
            description: input.description,
            acceptance_criteria: input.acceptance_criteria,
            constraints: input.constraints,
            labels: input.labels,
            status: TaskStatus::Open,
            priority: input.priority,
            created_at: now,
            updated_at: now,
        };
        self.metadata_store.create_task(&task)?;
        Ok(task)
    }

    fn list_tasks(&self) -> Result<Vec<Task>> {
        self.metadata_store.list_tasks(&self.repo_config.repo_id)
    }

    fn get_task(&self, id: &str) -> Result<Task> {
        self.get_task_by_id(id)
    }

    fn set_current_task(&self, task_id: &str) -> Result<Task> {
        let task = self.get_task_by_id(task_id)?;
        self.persist_current_task(&task.id)?;
        Ok(task)
    }

    fn get_current_task(&self) -> Result<Option<Task>> {
        self.current_task_with_validation()
    }
}

impl ChangeService for SyftApp {
    fn propose_change(&self, input: ProposeChangeInput) -> Result<ChangeNode> {
        let task_id = self.resolve_task_id(input.task_id.as_deref())?;
        let task = self.get_task_by_id(&task_id)?;
        let base_snapshot_id = self.resolve_base_snapshot_id(input.base_snapshot_id.as_deref())?;
        let base_snapshot = self.get_snapshot(&base_snapshot_id)?;
        let result_snapshot = self.get_snapshot(&input.result_snapshot_id)?;
        let base_index = snapshot_index(&base_snapshot.root_tree_hash, &self.object_store)?;
        let result_index = snapshot_index(&result_snapshot.root_tree_hash, &self.object_store)?;
        let patch_ops = diff_snapshot_indices(&base_index, &result_index);
        let semantic_delta = diff_snapshots(&base_snapshot, &result_snapshot, &self.object_store)?;
        let now = now_utc();

        let node = ChangeNode {
            id: new_entity_id(),
            repo_id: task.repo_id,
            task_id: task.id,
            title: input.title,
            intent: input.intent,
            rationale: input.rationale,
            parent_node_ids: Vec::new(),
            base_snapshot_id: base_snapshot.id,
            result_snapshot_id: result_snapshot.id,
            patch_ops,
            semantic_delta: semantic_delta.clone(),
            provenance: input.provenance.unwrap_or_else(default_provenance),
            validation_artifact_ids: Vec::new(),
            risk: calculate_risk(&semantic_delta, &[]),
            status: ChangeNodeStatus::Candidate,
            tags: input.tags,
            created_at: now,
            updated_at: now,
        };
        self.metadata_store.create_change_node(&node)?;
        Ok(node)
    }

    fn validate_change(&self, node_id: &str, plan: &ValidationPlan) -> Result<ChangeNode> {
        if !plan.any_enabled() {
            bail!("validation plan must enable at least one check");
        }

        let mut node = self.get_change_node(node_id)?;
        let result_snapshot = self.get_snapshot(&node.result_snapshot_id)?;
        let artifacts = self.validation_runner.validate(
            &self.repo_config.repo_id,
            &result_snapshot.id,
            Some(&node.id),
            &result_snapshot.root_tree_hash,
            &self.object_store,
            plan,
            &self.effective_capture_excludes()?,
        )?;

        for artifact in &artifacts {
            self.metadata_store.create_validation_artifact(artifact)?;
            node.validation_artifact_ids.push(artifact.id.clone());
        }

        node.status = if artifacts
            .iter()
            .all(|artifact| matches!(artifact.status, ValidationStatus::Passed))
        {
            ChangeNodeStatus::Validated
        } else {
            ChangeNodeStatus::Rejected
        };
        node.risk = calculate_risk(&node.semantic_delta, &artifacts);
        node.updated_at = now_utc();
        self.metadata_store.update_change_node(&node)?;
        Ok(node)
    }

    fn promote_change(&self, input: PromoteChangeInput) -> Result<PromotionRecord> {
        let mut node = self.get_change_node(&input.node_id)?;
        if !matches!(
            node.status,
            ChangeNodeStatus::Validated | ChangeNodeStatus::Approved
        ) {
            bail!("only validated changes can be promoted");
        }

        if input.export_to_git {
            let snapshot = self.get_snapshot(&node.result_snapshot_id)?;
            let commit_message =
                format!("syft promote: {} -> {}", node.title, input.target_lineage);
            let _commit_sha = export_snapshot_to_git_commit(
                &self.repo_path,
                &snapshot.root_tree_hash,
                &self.object_store,
                &commit_message,
            )?;
        }

        let record = PromotionRecord {
            id: new_entity_id(),
            repo_id: self.repo_config.repo_id.clone(),
            node_id: node.id.clone(),
            target_lineage: input.target_lineage,
            approved_by: input.approved_by,
            notes: input.notes,
            created_at: now_utc(),
        };
        self.metadata_store.create_promotion(&record)?;
        node.status = ChangeNodeStatus::Promoted;
        node.updated_at = now_utc();
        self.metadata_store.update_change_node(&node)?;
        self.persist_head(&node.result_snapshot_id)?;
        Ok(record)
    }
}
