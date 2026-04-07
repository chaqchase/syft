use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::{EntityId, SemanticDelta};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PatchOp {
    pub path: String,
    pub kind: PatchOpKind,
    pub old_path: Option<String>,
    pub before_hash: Option<String>,
    pub after_hash: Option<String>,
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
    Typecheck,
    Tests,
    Lint,
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub worktree_id: Option<EntityId>,
    pub title: String,
    pub intent: String,
    pub rationale: Option<String>,
    pub parent_node_ids: Vec<EntityId>,
    pub base_snapshot_id: EntityId,
    pub result_snapshot_id: EntityId,
    pub patch_ops: Vec<PatchOp>,
    pub semantic_delta: SemanticDelta,
    pub provenance: crate::Provenance,
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub worktree_name: Option<String>,
    pub validation_summary: Option<String>,
    pub validation_status: Option<ValidationStatus>,
    pub risk_score: u8,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}
