use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::{EntityId, ObjectHash};

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
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
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
    ImportedFromGit { commit_sha: String },
    #[default]
    MaterializedByHuman,
    MaterializedByAgent,
    MaterializedByCompose,
}
