use std::path::{Path, PathBuf};

use anyhow::{Result, bail};
use syft_git::{
    branch_exists, capture_worktree_snapshot, create_git_worktree, export_snapshot_to_git_commit,
    import_git_commit, materialize_snapshot_to, remove_git_worktree, worktree_is_dirty,
};
use syft_objects::{diff_snapshot_indices, snapshot_index};
use syft_semantic::diff_snapshots;
use syft_store::MetadataStore;
use syft_types::{
    ChangeNode, ChangeNodeStatus, ManagedWorktree, ManagedWorktreeStatus, PromotionRecord,
    Snapshot, Task, TaskStatus, ValidationPlan, ValidationStatus, WorktreeDetail, new_entity_id,
    now_utc,
};
use syft_validate::ValidationRunner;

use crate::app::{SyftApp, write_worktree_marker};
use crate::contracts::{
    ChangeService, CreateTaskInput, PromoteChangeInput, ProposeChangeInput, RepoService,
    TaskService, WorktreeCreateInput, WorktreeService,
};
use crate::helpers::{calculate_risk, default_provenance, shorten_id, slugify};

impl RepoService for SyftApp {
    fn import_git_commit(&self, commit: &str) -> Result<Snapshot> {
        let parent_snapshot_ids = self
            .current_head_snapshot_id()?
            .map(|id| vec![id])
            .unwrap_or_default();
        let (snapshot, _) = import_git_commit(
            &self.control_repo_path,
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
        let (mut snapshot, _) = capture_worktree_snapshot(
            &self.workspace_path,
            &capture_config,
            &self.object_store,
            parent_snapshot_ids,
        )?;
        snapshot.metadata.worktree_id = self.current_worktree.as_ref().map(|worktree| worktree.id.clone());
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
            worktree_id: self.current_worktree.as_ref().map(|worktree| worktree.id.clone()),
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
                &self.control_repo_path,
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

impl WorktreeService for SyftApp {
    fn create_worktree(&self, input: WorktreeCreateInput) -> Result<ManagedWorktree> {
        let task_id = self.resolve_task_id(input.task_id.as_deref())?;
        let task = self.get_task_by_id(&task_id)?;
        let repo = self.repo()?;
        let base_name = input
            .name
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| default_worktree_name(&task));
        let base_path = input
            .path
            .map(|path| resolve_worktree_path(&self.workspace_path, PathBuf::from(path)))
            .unwrap_or_else(|| {
                default_worktree_root(&self.control_repo_path, &repo.name).join(&base_name)
            });

        let mut attempt = 0usize;
        loop {
            let suffix = if attempt == 0 {
                String::new()
            } else {
                format!("-{}", attempt + 1)
            };
            let name = format!("{base_name}{suffix}");
            let branch = format!("syft/{}/{}", task.id, name);
            let path = append_path_suffix(&base_path, &suffix);

            let path_string = path.to_string_lossy().to_string();
            let name_taken = self
                .metadata_store
                .get_worktree_by_name(&self.repo_config.repo_id, &name)?
                .is_some();
            if !name_taken && !path.exists() && !branch_exists(&self.control_repo_path, &branch)? {
                create_git_worktree(&self.control_repo_path, &branch, &path, &input.source_ref)?;
                let now = now_utc();
                let worktree = ManagedWorktree {
                    id: new_entity_id(),
                    repo_id: self.repo_config.repo_id.clone(),
                    task_id: task.id.clone(),
                    name,
                    branch,
                    path: path_string,
                    source_ref: input.source_ref.clone(),
                    status: ManagedWorktreeStatus::Active,
                    created_at: now,
                    updated_at: now,
                };
                self.metadata_store.create_worktree(&worktree)?;
                write_worktree_marker(&path, &self.control_repo_path, &worktree.id)?;
                return Ok(worktree);
            }
            attempt += 1;
        }
    }

    fn list_worktrees(&self) -> Result<Vec<ManagedWorktree>> {
        self.metadata_store.list_worktrees(&self.repo_config.repo_id)
    }

    fn show_worktree(&self, id_or_name: &str) -> Result<WorktreeDetail> {
        let worktree = self.resolve_worktree_by_id_or_name(id_or_name)?;
        let linked_change_count = self
            .metadata_store
            .list_change_nodes(&self.repo_config.repo_id, None)?
            .into_iter()
            .filter(|change| change.worktree_id.as_deref() == Some(worktree.id.as_str()))
            .count();
        Ok(WorktreeDetail {
            worktree,
            linked_change_count,
        })
    }

    fn current_worktree(&self) -> Result<Option<ManagedWorktree>> {
        Ok(self.current_worktree.clone())
    }

    fn remove_worktree(&self, id_or_name: &str, force: bool) -> Result<ManagedWorktree> {
        let mut worktree = self.resolve_worktree_by_id_or_name(id_or_name)?;
        let worktree_path = PathBuf::from(&worktree.path);

        if !force && worktree_is_dirty(&worktree_path)? {
            bail!(
                "worktree {} has uncommitted changes; rerun with --force to remove it",
                worktree.name
            );
        }

        remove_git_worktree(&self.control_repo_path, &worktree_path, force)?;
        worktree.status = ManagedWorktreeStatus::Removed;
        worktree.updated_at = now_utc();
        self.metadata_store.update_worktree(&worktree)?;
        Ok(worktree)
    }
}

fn default_worktree_name(task: &Task) -> String {
    let slug = slugify(&task.title);
    if slug.is_empty() {
        format!("task-{}", shorten_id(&task.id))
    } else {
        format!("{slug}-{}", shorten_id(&task.id))
    }
}

fn default_worktree_root(control_repo_path: &Path, repo_name: &str) -> PathBuf {
    control_repo_path
        .parent()
        .unwrap_or(control_repo_path)
        .join(format!("{}-syft", slugify(repo_name)))
}

fn resolve_worktree_path(workspace_path: &Path, path: PathBuf) -> PathBuf {
    if path.is_absolute() {
        path
    } else {
        workspace_path.join(path)
    }
}

fn append_path_suffix(base_path: &Path, suffix: &str) -> PathBuf {
    if suffix.is_empty() {
        return base_path.to_path_buf();
    }

    let file_name = base_path
        .file_name()
        .map(|name| name.to_string_lossy().to_string())
        .unwrap_or_else(|| "worktree".to_string());
    base_path
        .parent()
        .unwrap_or(base_path)
        .join(format!("{file_name}{suffix}"))
}
