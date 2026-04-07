use std::collections::BTreeMap;

use anyhow::{Result, bail};
use syft_objects::{diff_snapshot_indices, snapshot_index};
use syft_store::MetadataStore;
use syft_types::{
    ChangeDetail, ChangeHeadline, ChangeListEntry, ChangeNodeStatus, HistoryEntry, HistoryQuery,
    PromotionHeadline, RepoStatusSummary, SnapshotDetail, SnapshotListEntry, Task,
    WorktreeHeadline,
};

use crate::app::SyftApp;
use crate::contracts::QueryService;
use crate::helpers::{diff_summary, provenance_summary, snapshot_source_summary, symbol_names_for_change};

impl QueryService for SyftApp {
    fn status(&self) -> Result<RepoStatusSummary> {
        let repo = self.repo()?;
        let snapshots = self.metadata_store.list_snapshots(&self.repo_config.repo_id)?;
        let tasks = self.metadata_store.list_tasks(&self.repo_config.repo_id)?;
        let changes = self
            .metadata_store
            .list_change_nodes(&self.repo_config.repo_id, None)?;
        let promotions = self.metadata_store.list_promotions(&self.repo_config.repo_id)?;
        let worktree_map = self.worktree_map()?;

        let mut task_counts = BTreeMap::new();
        for task in &tasks {
            *task_counts.entry(format!("{:?}", task.status)).or_insert(0) += 1;
        }

        let mut change_counts = BTreeMap::new();
        for change in &changes {
            *change_counts
                .entry(format!("{:?}", change.status))
                .or_insert(0) += 1;
        }

        let latest_promoted_change = promotions.last().map(|promotion| PromotionHeadline {
            node_id: promotion.node_id.clone(),
            target_lineage: promotion.target_lineage.clone(),
            created_at: promotion.created_at,
        });

        let task_map = self.task_map()?;
        let latest_validated_or_failed_change = changes.iter().find_map(|change| {
            self.latest_validation_for_node(&change.id)
                .ok()
                .flatten()
                .map(|artifact| ChangeHeadline {
                    node_id: change.id.clone(),
                    title: change.title.clone(),
                    status: change.status.clone(),
                    task_id: Some(change.task_id.clone()),
                    task_title: task_map.get(&change.task_id).map(|task| task.title.clone()),
                    worktree_name: change
                        .worktree_id
                        .as_ref()
                        .and_then(|id| worktree_map.get(id))
                        .map(|worktree| worktree.name.clone()),
                    validation_summary: Some(artifact.summary),
                    validation_status: Some(artifact.status),
                    risk_score: change.risk.score,
                    created_at: change.created_at,
                    updated_at: change.updated_at,
                })
        });

        let mut attention_needed = Vec::new();
        if self.current_head_snapshot_id()?.is_none() {
            attention_needed.push("no head snapshot has been imported or captured yet".to_string());
        }
        if changes
            .iter()
            .any(|change| matches!(change.status, ChangeNodeStatus::Rejected))
        {
            attention_needed.push("one or more changes have failing validations".to_string());
        }

        Ok(RepoStatusSummary {
            repo_name: repo.name,
            repo_id: repo.id,
            current_head_snapshot_id: self.current_head_snapshot_id()?,
            current_worktree: self.current_worktree.as_ref().map(|worktree| WorktreeHeadline {
                id: worktree.id.clone(),
                name: worktree.name.clone(),
                path: worktree.path.clone(),
                branch: worktree.branch.clone(),
                task_id: worktree.task_id.clone(),
            }),
            latest_snapshot_at: snapshots.iter().map(|snapshot| snapshot.created_at).max(),
            task_counts,
            change_counts,
            latest_promoted_change,
            latest_validated_or_failed_change,
            attention_needed,
        })
    }

    fn history(&self, query: &HistoryQuery) -> Result<Vec<HistoryEntry>> {
        let changes = self
            .metadata_store
            .list_change_nodes(&self.repo_config.repo_id, None)?;
        let task_map = self.task_map()?;
        let worktree_map = self.worktree_map()?;

        let mut entries = Vec::new();
        for change in changes {
            if let Some(task_id) = &query.task_id {
                if &change.task_id != task_id {
                    continue;
                }
            }

            let symbol_names = symbol_names_for_change(&change);
            if let Some(symbol) = &query.symbol {
                if !symbol_names.iter().any(|candidate| candidate == symbol) {
                    continue;
                }
            }

            let latest_validation = self.latest_validation_for_node(&change.id)?;
            let promotion_state = self.promotion_state_for_node(&change.id)?;
            let task_title = task_map
                .get(&change.task_id)
                .map(|task| task.title.clone())
                .unwrap_or_else(|| "<unknown task>".to_string());

            entries.push(HistoryEntry {
                node_id: change.id.clone(),
                title: change.title.clone(),
                task_id: change.task_id.clone(),
                task_title,
                worktree_name: change
                    .worktree_id
                    .as_ref()
                    .and_then(|id| worktree_map.get(id))
                    .map(|worktree| worktree.name.clone()),
                intent: change.intent.clone(),
                changed_file_count: change.semantic_delta.changed_files.len(),
                touched_symbols: symbol_names,
                validation_summary: latest_validation
                    .as_ref()
                    .map(|artifact| artifact.summary.clone()),
                validation_status: latest_validation.map(|artifact| artifact.status),
                promotion_state,
                provenance_summary: provenance_summary(&change.provenance),
                created_at: change.created_at,
                updated_at: change.updated_at,
            });
        }

        let limit = if query.limit == 0 { 20 } else { query.limit };
        entries.truncate(limit);
        Ok(entries)
    }

    fn show_task(&self, task_id: &str) -> Result<Task> {
        self.get_task_by_id(task_id)
    }

    fn list_changes_for_task(&self, task_id: &str) -> Result<Vec<ChangeListEntry>> {
        let task = self.get_task_by_id(task_id)?;
        let changes = self
            .metadata_store
            .list_change_nodes(&self.repo_config.repo_id, None)?;
        let task_map = self.task_map()?;
        let worktree_map = self.worktree_map()?;

        changes
            .into_iter()
            .filter(|change| change.task_id == task.id)
            .map(|change| self.change_list_entry(change, &task_map, &worktree_map))
            .collect()
    }

    fn latest_change(&self, task_id: Option<&str>) -> Result<ChangeDetail> {
        let effective_task_id = match task_id {
            Some(task_id) => Some(self.get_task_by_id(task_id)?.id),
            None => self.current_task_with_validation()?.map(|task| task.id),
        };

        let changes = self
            .metadata_store
            .list_change_nodes(&self.repo_config.repo_id, None)?;
        let Some(change) = changes.into_iter().find(|change| {
            effective_task_id
                .as_ref()
                .map(|task_id| &change.task_id == task_id)
                .unwrap_or(true)
        }) else {
            if let Some(task_id) = effective_task_id {
                bail!("no changes found for task {task_id}");
            }
            bail!("no changes found");
        };

        self.show_change(&change.id, false)
    }

    fn list_snapshots(&self) -> Result<Vec<SnapshotListEntry>> {
        let worktree_map = self.worktree_map()?;
        Ok(self
            .metadata_store
            .list_snapshots(&self.repo_config.repo_id)?
            .into_iter()
            .rev()
            .map(|snapshot| SnapshotListEntry {
                id: snapshot.id.clone(),
                source: snapshot_source_summary(&snapshot.metadata.source),
                label_summary: if snapshot.metadata.labels.is_empty() {
                    "-".to_string()
                } else {
                    snapshot.metadata.labels.join(", ")
                },
                worktree_name: snapshot
                    .metadata
                    .worktree_id
                    .as_ref()
                    .and_then(|id| worktree_map.get(id))
                    .map(|worktree| worktree.name.clone()),
                created_at: snapshot.created_at,
                parent_count: snapshot.parent_snapshot_ids.len(),
            })
            .collect())
    }

    fn show_snapshot(&self, snapshot_id: &str) -> Result<SnapshotDetail> {
        let snapshot = self.get_snapshot(snapshot_id)?;
        let worktree_map = self.worktree_map()?;
        let changed_file_count_from_parent =
            if let Some(parent_id) = snapshot.parent_snapshot_ids.first() {
                let parent = self.get_snapshot(parent_id)?;
                let base_index = snapshot_index(&parent.root_tree_hash, &self.object_store)?;
                let next_index = snapshot_index(&snapshot.root_tree_hash, &self.object_store)?;
                Some(diff_snapshot_indices(&base_index, &next_index).len())
            } else {
                None
            };

        Ok(SnapshotDetail {
            source: snapshot_source_summary(&snapshot.metadata.source),
            worktree_name: snapshot
                .metadata
                .worktree_id
                .as_ref()
                .and_then(|id| worktree_map.get(id))
                .map(|worktree| worktree.name.clone()),
            snapshot,
            changed_file_count_from_parent,
        })
    }

    fn diff_snapshots(&self, from_snapshot_id: &str, to_snapshot_id: &str) -> Result<syft_types::DiffSummary> {
        let from_snapshot = self.get_snapshot(from_snapshot_id)?;
        let to_snapshot = self.get_snapshot(to_snapshot_id)?;
        let from_index = snapshot_index(&from_snapshot.root_tree_hash, &self.object_store)?;
        let to_index = snapshot_index(&to_snapshot.root_tree_hash, &self.object_store)?;
        let ops = diff_snapshot_indices(&from_index, &to_index);
        Ok(diff_summary(Some(from_snapshot.id), Some(to_snapshot.id), None, ops))
    }

    fn list_changes(&self) -> Result<Vec<ChangeListEntry>> {
        let changes = self
            .metadata_store
            .list_change_nodes(&self.repo_config.repo_id, None)?;
        let task_map = self.task_map()?;
        let worktree_map = self.worktree_map()?;

        changes
            .into_iter()
            .map(|change| self.change_list_entry(change, &task_map, &worktree_map))
            .collect()
    }

    fn show_change(&self, node_id: &str, include_logs: bool) -> Result<ChangeDetail> {
        let node = self.get_change_node(node_id)?;
        let task = self.metadata_store.get_task(&node.task_id)?;
        let worktree = match node.worktree_id.as_deref() {
            Some(worktree_id) => self.metadata_store.get_worktree(worktree_id)?,
            None => None,
        };
        let validations = self.validation_records_for_node(node_id, include_logs)?;
        let promotions = self.metadata_store.list_promotions_for_node(node_id)?;

        Ok(ChangeDetail {
            node,
            task,
            worktree,
            validations,
            promotions,
        })
    }

    fn diff_change(&self, node_id: &str) -> Result<syft_types::DiffSummary> {
        let node = self.get_change_node(node_id)?;
        Ok(diff_summary(
            Some(node.base_snapshot_id.clone()),
            Some(node.result_snapshot_id.clone()),
            Some(node.id),
            node.patch_ops,
        ))
    }
}
