use std::collections::BTreeMap;

use blake3::Hasher;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use ulid::Ulid;

pub type EntityId = String;
pub type ObjectHash = String;

pub fn new_entity_id() -> EntityId {
    Ulid::new().to_string()
}

pub fn hash_bytes(bytes: &[u8]) -> ObjectHash {
    let mut hasher = Hasher::new();
    hasher.update(bytes);
    hasher.finalize().to_hex().to_string()
}

pub fn now_utc() -> DateTime<Utc> {
    Utc::now()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Repo {
    pub id: EntityId,
    pub name: String,
    pub root_path: String,
    pub default_lineage: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RepoConfig {
    pub repo_id: EntityId,
    pub name: String,
    pub default_lineage: String,
    pub object_store: String,
    pub metadata_db: String,
    pub semantic_languages: Vec<String>,
    pub git_bridge: bool,
    #[serde(default)]
    pub capture_excludes: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Snapshot {
    pub id: EntityId,
    pub parent_snapshot_ids: Vec<EntityId>,
    pub root_tree_hash: ObjectHash,
    pub created_at: DateTime<Utc>,
    pub metadata: SnapshotMetadata,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SnapshotMetadata {
    pub repo_id: EntityId,
    pub source: SnapshotSource,
    pub labels: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub enum SnapshotSource {
    ImportedFromGit {
        commit_sha: String,
    },
    #[default]
    MaterializedByHuman,
    MaterializedByAgent,
    MaterializedByCompose,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Task {
    pub id: EntityId,
    pub repo_id: EntityId,
    pub title: String,
    pub description: String,
    pub acceptance_criteria: Vec<String>,
    pub constraints: Vec<String>,
    pub labels: Vec<String>,
    pub status: TaskStatus,
    pub priority: TaskPriority,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub enum TaskStatus {
    #[default]
    Open,
    InReview,
    Done,
    Archived,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub enum TaskPriority {
    Low,
    #[default]
    Medium,
    High,
    Critical,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Provenance {
    pub author: Author,
    pub model: Option<ModelInfo>,
    pub prompt_ref: Option<EntityId>,
    pub retrieved_context_refs: Vec<EntityId>,
    pub tool_run_refs: Vec<EntityId>,
    pub session_ref: Option<EntityId>,
    pub created_at: DateTime<Utc>,
}

impl Default for Provenance {
    fn default() -> Self {
        Self {
            author: Author::Human {
                user_id: "unknown".to_string(),
            },
            model: None,
            prompt_ref: None,
            retrieved_context_refs: Vec::new(),
            tool_run_refs: Vec::new(),
            session_ref: None,
            created_at: now_utc(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Author {
    Human { user_id: String },
    Agent { agent_id: String },
    Tool { tool_name: String },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelInfo {
    pub provider: String,
    pub name: String,
    pub version: Option<String>,
    pub temperature: Option<f32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Hash, Eq, PartialEq)]
pub enum Language {
    Rust,
    TypeScript,
    Python,
    Go,
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize, Hash, Eq, PartialEq)]
pub enum Visibility {
    Public,
    Protected,
    Private,
    Internal,
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize, Hash, Eq, PartialEq)]
pub enum SymbolCategory {
    Callable,
    Type,
    Namespace,
    Value,
    Member,
    Macro,
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize, Hash, Eq, PartialEq)]
pub struct SymbolId {
    pub language: Language,
    pub namespace: String,
    pub path: String,
    pub local_name: String,
    pub disambiguator: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Hash, Eq, PartialEq)]
pub struct SpanRef {
    pub start_line: u32,
    pub start_col: u32,
    pub end_line: u32,
    pub end_col: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, Hash, Eq, PartialEq)]
pub struct SymbolSource {
    pub file_path: String,
    pub span: SpanRef,
    pub visibility: Visibility,
}

#[derive(Debug, Clone, Serialize, Deserialize, Hash, Eq, PartialEq)]
pub struct SymbolRef {
    pub id: SymbolId,
    pub display_name: String,
    pub source: SymbolSource,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SymbolDescriptor {
    pub symbol: SymbolRef,
    pub category: SymbolCategory,
    pub tags: Vec<String>,
    pub attributes: BTreeMap<String, serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SemanticEdge {
    pub from: SymbolId,
    pub to: SymbolTarget,
    pub kind: EdgeKind,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SymbolTarget {
    Symbol(SymbolId),
    External(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum EdgeKind {
    Defines,
    Calls,
    UsesType,
    Implements,
    Extends,
    Imports,
    Exports,
    Reads,
    Writes,
    Contains,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SemanticDelta {
    pub touched_symbols: Vec<SymbolRef>,
    pub added_symbols: Vec<SymbolRef>,
    pub removed_symbols: Vec<SymbolRef>,
    pub changed_public_api: bool,
    pub changed_dependencies: Vec<DependencyEdgeChange>,
    pub changed_files: Vec<String>,
    pub summary: String,
}

impl Default for SemanticDelta {
    fn default() -> Self {
        Self {
            touched_symbols: Vec::new(),
            added_symbols: Vec::new(),
            removed_symbols: Vec::new(),
            changed_public_api: false,
            changed_dependencies: Vec::new(),
            changed_files: Vec::new(),
            summary: "no semantic changes detected".to_string(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DependencyEdgeChange {
    pub from: String,
    pub to: String,
    pub kind: DependencyEdgeChangeKind,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum DependencyEdgeChangeKind {
    Added,
    Removed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PatchOp {
    pub path: String,
    pub kind: PatchOpKind,
    pub old_path: Option<String>,
    pub before_hash: Option<ObjectHash>,
    pub after_hash: Option<ObjectHash>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum PatchOpKind {
    Add,
    Delete,
    Modify,
    Rename,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidationArtifact {
    pub id: EntityId,
    pub repo_id: EntityId,
    pub snapshot_id: EntityId,
    pub node_id: Option<EntityId>,
    pub kind: ValidationKind,
    pub status: ValidationStatus,
    pub summary: String,
    pub details_ref: Option<String>,
    pub metrics: ValidationMetrics,
    pub started_at: DateTime<Utc>,
    pub completed_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidationDetails {
    pub command: String,
    pub exit_status: i32,
    pub stdout: String,
    pub stderr: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ValidationKind {
    Tests,
    Lint,
    Typecheck,
    Security,
    Benchmark,
    Custom { name: String },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ValidationStatus {
    Passed,
    Failed,
    Skipped,
    Error,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ValidationMetrics {
    pub duration_ms: u64,
    pub passed_count: Option<u64>,
    pub failed_count: Option<u64>,
    pub skipped_count: Option<u64>,
    pub coverage_delta: Option<f32>,
    pub benchmark_delta_pct: Option<f32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct RiskReport {
    pub score: u8,
    pub reasons: Vec<String>,
    pub impacted_domains: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChangeNode {
    pub id: EntityId,
    pub repo_id: EntityId,
    pub task_id: EntityId,
    pub title: String,
    pub intent: String,
    pub rationale: Option<String>,
    pub parent_node_ids: Vec<EntityId>,
    pub base_snapshot_id: EntityId,
    pub result_snapshot_id: EntityId,
    pub patch_ops: Vec<PatchOp>,
    pub semantic_delta: SemanticDelta,
    pub provenance: Provenance,
    pub validation_artifact_ids: Vec<EntityId>,
    pub risk: RiskReport,
    pub status: ChangeNodeStatus,
    pub tags: Vec<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ChangeNodeStatus {
    Draft,
    Candidate,
    Validated,
    Approved,
    Rejected,
    Promoted,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PromotionRecord {
    pub id: EntityId,
    pub repo_id: EntityId,
    pub node_id: EntityId,
    pub target_lineage: String,
    pub approved_by: String,
    pub notes: Option<String>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidationRecord {
    pub artifact: ValidationArtifact,
    pub details: Option<ValidationDetails>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PromotionHeadline {
    pub node_id: EntityId,
    pub target_lineage: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChangeHeadline {
    pub node_id: EntityId,
    pub title: String,
    pub status: ChangeNodeStatus,
    pub task_id: Option<EntityId>,
    pub task_title: Option<String>,
    pub validation_summary: Option<String>,
    pub validation_status: Option<ValidationStatus>,
    pub risk_score: u8,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RepoStatusSummary {
    pub repo_name: String,
    pub repo_id: EntityId,
    pub current_head_snapshot_id: Option<EntityId>,
    pub latest_snapshot_at: Option<DateTime<Utc>>,
    pub task_counts: BTreeMap<String, usize>,
    pub change_counts: BTreeMap<String, usize>,
    pub latest_promoted_change: Option<PromotionHeadline>,
    pub latest_validated_or_failed_change: Option<ChangeHeadline>,
    pub attention_needed: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HistoryQuery {
    pub task_id: Option<EntityId>,
    pub symbol: Option<String>,
    pub limit: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HistoryEntry {
    pub node_id: EntityId,
    pub title: String,
    pub task_id: EntityId,
    pub task_title: String,
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
    pub created_at: DateTime<Utc>,
    pub parent_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SnapshotDetail {
    pub snapshot: Snapshot,
    pub source: String,
    pub changed_file_count_from_parent: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChangeListEntry {
    pub node_id: EntityId,
    pub title: String,
    pub status: ChangeNodeStatus,
    pub task_id: EntityId,
    pub task_title: String,
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
    pub validations: Vec<ValidationRecord>,
    pub promotions: Vec<PromotionRecord>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiffSummary {
    pub from_snapshot_id: Option<EntityId>,
    pub to_snapshot_id: Option<EntityId>,
    pub change_node_id: Option<EntityId>,
    pub ops: Vec<PatchOp>,
    pub counts: BTreeMap<String, usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidationPlan {
    pub run_tests: bool,
    pub run_lint: bool,
    pub run_typecheck: bool,
}

impl ValidationPlan {
    pub fn any_enabled(&self) -> bool {
        self.run_tests || self.run_lint || self.run_typecheck
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum FileMode {
    File,
    Executable,
    Symlink,
    Directory,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TreeEntry {
    pub name: String,
    pub mode: FileMode,
    pub hash: ObjectHash,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TreeObject {
    pub entries: Vec<TreeEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SnapshotIndex {
    pub files: BTreeMap<String, ObjectHash>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn entity_ids_are_non_empty() {
        assert!(!new_entity_id().is_empty());
    }

    #[test]
    fn hashing_is_stable() {
        let first = hash_bytes(b"syft");
        let second = hash_bytes(b"syft");
        assert_eq!(first, second);
    }

    #[test]
    fn semantic_delta_defaults_are_empty() {
        let delta = SemanticDelta::default();
        assert!(delta.changed_files.is_empty());
        assert!(!delta.changed_public_api);
    }
}
