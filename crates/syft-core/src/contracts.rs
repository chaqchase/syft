use std::path::Path;

use anyhow::Result;
use syft_types::{
    ChangeDetail, ChangeListEntry, ChangeNode, DiffSummary, HistoryEntry, HistoryQuery,
    ManagedWorktree, PromotionRecord, RepoStatusSummary, Snapshot, SnapshotDetail,
    SnapshotListEntry, Task, TaskPriority, ValidationPlan, WorktreeDetail,
};

#[derive(Debug, Clone)]
pub struct CreateTaskInput {
    pub title: String,
    pub description: String,
    pub acceptance_criteria: Vec<String>,
    pub constraints: Vec<String>,
    pub labels: Vec<String>,
    pub priority: TaskPriority,
}

#[derive(Debug, Clone)]
pub struct ProposeChangeInput {
    pub task_id: Option<String>,
    pub title: String,
    pub intent: String,
    pub rationale: Option<String>,
    pub base_snapshot_id: Option<String>,
    pub result_snapshot_id: String,
    pub provenance: Option<syft_types::Provenance>,
    pub tags: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct PromoteChangeInput {
    pub node_id: String,
    pub target_lineage: String,
    pub approved_by: String,
    pub notes: Option<String>,
    pub export_to_git: bool,
}

#[derive(Debug, Clone)]
pub struct WorktreeCreateInput {
    pub task_id: Option<String>,
    pub name: Option<String>,
    pub source_ref: String,
    pub path: Option<String>,
}

pub trait RepoService {
    fn import_git_commit(&self, commit: &str) -> Result<Snapshot>;
    fn capture_snapshot(&self) -> Result<Snapshot>;
    fn materialize_snapshot(&self, snapshot_id: &str, destination: &Path) -> Result<()>;
}

pub trait TaskService {
    fn create_task(&self, input: CreateTaskInput) -> Result<Task>;
    fn list_tasks(&self) -> Result<Vec<Task>>;
    fn get_task(&self, id: &str) -> Result<Task>;
    fn set_current_task(&self, task_id: &str) -> Result<Task>;
    fn get_current_task(&self) -> Result<Option<Task>>;
}

pub trait ChangeService {
    fn propose_change(&self, input: ProposeChangeInput) -> Result<ChangeNode>;
    fn validate_change(&self, node_id: &str, plan: &ValidationPlan) -> Result<ChangeNode>;
    fn promote_change(&self, input: PromoteChangeInput) -> Result<PromotionRecord>;
}

pub trait WorktreeService {
    fn create_worktree(&self, input: WorktreeCreateInput) -> Result<ManagedWorktree>;
    fn list_worktrees(&self) -> Result<Vec<ManagedWorktree>>;
    fn show_worktree(&self, id_or_name: &str) -> Result<WorktreeDetail>;
    fn current_worktree(&self) -> Result<Option<ManagedWorktree>>;
    fn remove_worktree(&self, id_or_name: &str, force: bool) -> Result<ManagedWorktree>;
}

pub trait QueryService {
    fn status(&self) -> Result<RepoStatusSummary>;
    fn history(&self, query: &HistoryQuery) -> Result<Vec<HistoryEntry>>;
    fn show_task(&self, task_id: &str) -> Result<Task>;
    fn list_changes_for_task(&self, task_id: &str) -> Result<Vec<ChangeListEntry>>;
    fn latest_change(&self, task_id: Option<&str>) -> Result<ChangeDetail>;
    fn list_snapshots(&self) -> Result<Vec<SnapshotListEntry>>;
    fn show_snapshot(&self, snapshot_id: &str) -> Result<SnapshotDetail>;
    fn diff_snapshots(&self, from_snapshot_id: &str, to_snapshot_id: &str) -> Result<DiffSummary>;
    fn list_changes(&self) -> Result<Vec<ChangeListEntry>>;
    fn show_change(&self, node_id: &str, include_logs: bool) -> Result<ChangeDetail>;
    fn diff_change(&self, node_id: &str) -> Result<DiffSummary>;
}
