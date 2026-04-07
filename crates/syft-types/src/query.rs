use std::collections::BTreeMap;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::{
    ChangeHeadline, ChangeNode, ChangeNodeStatus, EntityId, ManagedWorktree, PatchOp,
    PromotionHeadline, PromotionRecord, Snapshot, Task, ValidationRecord, ValidationStatus,
};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RepoStatusSummary {
    pub repo_name: String,
    pub repo_id: EntityId,
    pub current_head_snapshot_id: Option<EntityId>,
    pub current_worktree: Option<WorktreeHeadline>,
    pub latest_snapshot_at: Option<DateTime<Utc>>,
    pub task_counts: BTreeMap<String, usize>,
    pub change_counts: BTreeMap<String, usize>,
    pub latest_promoted_change: Option<PromotionHeadline>,
    pub latest_validated_or_failed_change: Option<ChangeHeadline>,
    pub attention_needed: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HistoryQuery {
    pub task_id: Option<String>,
    pub symbol: Option<String>,
    pub limit: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HistoryEntry {
    pub node_id: EntityId,
    pub title: String,
    pub task_id: EntityId,
    pub task_title: String,
    pub worktree_name: Option<String>,
    pub intent: String,
    pub changed_file_count: usize,
    pub touched_symbols: Vec<String>,
    pub validation_summary: Option<String>,
    pub validation_status: Option<ValidationStatus>,
    pub promotion_state: Option<String>,
    pub provenance_summary: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SnapshotListEntry {
    pub id: EntityId,
    pub source: String,
    pub label_summary: String,
    pub worktree_name: Option<String>,
    pub parent_count: usize,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SnapshotDetail {
    pub snapshot: Snapshot,
    pub source: String,
    pub worktree_name: Option<String>,
    pub changed_file_count_from_parent: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChangeListEntry {
    pub node_id: EntityId,
    pub title: String,
    pub status: ChangeNodeStatus,
    pub task_id: EntityId,
    pub task_title: String,
    pub worktree_name: Option<String>,
    pub risk_score: u8,
    pub latest_validation_summary: Option<String>,
    pub latest_validation_status: Option<ValidationStatus>,
    pub promotion_state: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChangeDetail {
    pub node: ChangeNode,
    pub task: Option<Task>,
    pub worktree: Option<ManagedWorktree>,
    pub validations: Vec<ValidationRecord>,
    pub promotions: Vec<PromotionRecord>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorktreeHeadline {
    pub id: EntityId,
    pub name: String,
    pub path: String,
    pub branch: String,
    pub task_id: EntityId,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorktreeDetail {
    pub worktree: ManagedWorktree,
    pub linked_change_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiffSummary {
    pub from_snapshot_id: Option<String>,
    pub to_snapshot_id: Option<String>,
    pub change_node_id: Option<String>,
    pub ops: Vec<PatchOp>,
    pub counts: BTreeMap<String, usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ValidationPlan {
    pub run_typecheck: bool,
    pub run_tests: bool,
    pub run_lint: bool,
}

impl ValidationPlan {
    pub fn any_enabled(&self) -> bool {
        self.run_typecheck || self.run_tests || self.run_lint
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum FileMode {
    File,
    Executable,
    Directory,
    Symlink,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TreeEntry {
    pub name: String,
    pub mode: FileMode,
    pub hash: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TreeObject {
    pub entries: Vec<TreeEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SnapshotIndex {
    pub files: BTreeMap<String, String>,
}
