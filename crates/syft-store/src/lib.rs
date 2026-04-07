use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow};
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Serialize, de::DeserializeOwned};
use syft_types::{
    ChangeNode, ManagedWorktree, ObjectHash, PromotionRecord, Repo, Snapshot, Task,
    ValidationArtifact, hash_bytes,
};

pub trait MetadataStore: Send + Sync {
    fn initialize(&self) -> Result<()>;
    fn put_repo(&self, repo: &Repo) -> Result<()>;
    fn get_repo(&self, id: &str) -> Result<Option<Repo>>;
    fn create_snapshot(&self, snapshot: &Snapshot) -> Result<()>;
    fn get_snapshot(&self, id: &str) -> Result<Option<Snapshot>>;
    fn list_snapshots(&self, repo_id: &str) -> Result<Vec<Snapshot>>;
    fn create_task(&self, task: &Task) -> Result<()>;
    fn get_task(&self, id: &str) -> Result<Option<Task>>;
    fn list_tasks(&self, repo_id: &str) -> Result<Vec<Task>>;
    fn create_change_node(&self, node: &ChangeNode) -> Result<()>;
    fn get_change_node(&self, id: &str) -> Result<Option<ChangeNode>>;
    fn list_change_nodes(&self, repo_id: &str, limit: Option<usize>) -> Result<Vec<ChangeNode>>;
    fn update_change_node(&self, node: &ChangeNode) -> Result<()>;
    fn create_worktree(&self, worktree: &ManagedWorktree) -> Result<()>;
    fn get_worktree(&self, id: &str) -> Result<Option<ManagedWorktree>>;
    fn get_worktree_by_name(&self, repo_id: &str, name: &str) -> Result<Option<ManagedWorktree>>;
    fn list_worktrees(&self, repo_id: &str) -> Result<Vec<ManagedWorktree>>;
    fn list_worktrees_for_task(&self, task_id: &str) -> Result<Vec<ManagedWorktree>>;
    fn update_worktree(&self, worktree: &ManagedWorktree) -> Result<()>;
    fn create_validation_artifact(&self, artifact: &ValidationArtifact) -> Result<()>;
    fn get_validation_artifact(&self, id: &str) -> Result<Option<ValidationArtifact>>;
    fn list_validation_artifacts_for_node(&self, node_id: &str) -> Result<Vec<ValidationArtifact>>;
    fn create_promotion(&self, record: &PromotionRecord) -> Result<()>;
    fn list_promotions(&self, repo_id: &str) -> Result<Vec<PromotionRecord>>;
    fn list_promotions_for_node(&self, node_id: &str) -> Result<Vec<PromotionRecord>>;
}

pub trait ObjectStore: Send + Sync {
    fn put_bytes(&self, bytes: &[u8]) -> Result<ObjectHash>;
    fn get_bytes(&self, hash: &str) -> Result<Option<Vec<u8>>>;
}

#[derive(Debug, Clone)]
pub struct SqliteMetadataStore {
    db_path: PathBuf,
}

impl SqliteMetadataStore {
    pub fn new(db_path: impl Into<PathBuf>) -> Self {
        Self {
            db_path: db_path.into(),
        }
    }

    fn connect(&self) -> Result<Connection> {
        if let Some(parent) = self.db_path.parent() {
            fs::create_dir_all(parent)?;
        }
        Connection::open(&self.db_path).with_context(|| {
            format!(
                "failed to open metadata database at {}",
                self.db_path.display()
            )
        })
    }
}

impl MetadataStore for SqliteMetadataStore {
    fn initialize(&self) -> Result<()> {
        let connection = self.connect()?;
        connection.execute_batch(
            "
            CREATE TABLE IF NOT EXISTS repos (
                id TEXT PRIMARY KEY,
                created_at TEXT NOT NULL,
                data TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS snapshots (
                id TEXT PRIMARY KEY,
                repo_id TEXT NOT NULL,
                created_at TEXT NOT NULL,
                data TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS tasks (
                id TEXT PRIMARY KEY,
                repo_id TEXT NOT NULL,
                title TEXT NOT NULL,
                created_at TEXT NOT NULL,
                data TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS change_nodes (
                id TEXT PRIMARY KEY,
                repo_id TEXT NOT NULL,
                task_id TEXT NOT NULL,
                created_at TEXT NOT NULL,
                status TEXT NOT NULL,
                data TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS worktrees (
                id TEXT PRIMARY KEY,
                repo_id TEXT NOT NULL,
                task_id TEXT NOT NULL,
                name TEXT NOT NULL,
                created_at TEXT NOT NULL,
                status TEXT NOT NULL,
                data TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS validation_artifacts (
                id TEXT PRIMARY KEY,
                repo_id TEXT NOT NULL,
                node_id TEXT,
                started_at TEXT NOT NULL,
                data TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS promotions (
                id TEXT PRIMARY KEY,
                repo_id TEXT NOT NULL,
                node_id TEXT NOT NULL,
                target_lineage TEXT NOT NULL,
                created_at TEXT NOT NULL,
                data TEXT NOT NULL
            );
            ",
        )?;
        Ok(())
    }

    fn put_repo(&self, repo: &Repo) -> Result<()> {
        let connection = self.connect()?;
        let data = to_json(repo)?;
        connection.execute(
            "INSERT OR REPLACE INTO repos (id, created_at, data) VALUES (?1, ?2, ?3)",
            params![repo.id, repo.created_at.to_rfc3339(), data],
        )?;
        Ok(())
    }

    fn get_repo(&self, id: &str) -> Result<Option<Repo>> {
        let connection = self.connect()?;
        let data = connection
            .query_row("SELECT data FROM repos WHERE id = ?1", params![id], |row| {
                row.get::<_, String>(0)
            })
            .optional()?;
        data.map(|raw| from_json(&raw)).transpose()
    }

    fn create_snapshot(&self, snapshot: &Snapshot) -> Result<()> {
        let connection = self.connect()?;
        let data = to_json(snapshot)?;
        connection.execute(
            "INSERT OR REPLACE INTO snapshots (id, repo_id, created_at, data) VALUES (?1, ?2, ?3, ?4)",
            params![
                snapshot.id,
                snapshot.metadata.repo_id,
                snapshot.created_at.to_rfc3339(),
                data
            ],
        )?;
        Ok(())
    }

    fn get_snapshot(&self, id: &str) -> Result<Option<Snapshot>> {
        let connection = self.connect()?;
        let data = connection
            .query_row(
                "SELECT data FROM snapshots WHERE id = ?1",
                params![id],
                |row| row.get::<_, String>(0),
            )
            .optional()?;
        data.map(|raw| from_json(&raw)).transpose()
    }

    fn list_snapshots(&self, repo_id: &str) -> Result<Vec<Snapshot>> {
        let connection = self.connect()?;
        let mut statement = connection
            .prepare("SELECT data FROM snapshots WHERE repo_id = ?1 ORDER BY created_at ASC")?;
        let rows = statement.query_map(params![repo_id], |row| row.get::<_, String>(0))?;
        collect_rows(rows)
    }

    fn create_task(&self, task: &Task) -> Result<()> {
        let connection = self.connect()?;
        let data = to_json(task)?;
        connection.execute(
            "INSERT OR REPLACE INTO tasks (id, repo_id, title, created_at, data) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                task.id,
                task.repo_id,
                task.title,
                task.created_at.to_rfc3339(),
                data
            ],
        )?;
        Ok(())
    }

    fn get_task(&self, id: &str) -> Result<Option<Task>> {
        let connection = self.connect()?;
        let data = connection
            .query_row("SELECT data FROM tasks WHERE id = ?1", params![id], |row| {
                row.get::<_, String>(0)
            })
            .optional()?;
        data.map(|raw| from_json(&raw)).transpose()
    }

    fn list_tasks(&self, repo_id: &str) -> Result<Vec<Task>> {
        let connection = self.connect()?;
        let mut statement = connection
            .prepare("SELECT data FROM tasks WHERE repo_id = ?1 ORDER BY created_at ASC")?;
        let rows = statement.query_map(params![repo_id], |row| row.get::<_, String>(0))?;
        collect_rows(rows)
    }

    fn create_change_node(&self, node: &ChangeNode) -> Result<()> {
        let connection = self.connect()?;
        let data = to_json(node)?;
        connection.execute(
            "INSERT OR REPLACE INTO change_nodes (id, repo_id, task_id, created_at, status, data) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                node.id,
                node.repo_id,
                node.task_id,
                node.created_at.to_rfc3339(),
                format!("{:?}", node.status),
                data
            ],
        )?;
        Ok(())
    }

    fn get_change_node(&self, id: &str) -> Result<Option<ChangeNode>> {
        let connection = self.connect()?;
        let data = connection
            .query_row(
                "SELECT data FROM change_nodes WHERE id = ?1",
                params![id],
                |row| row.get::<_, String>(0),
            )
            .optional()?;
        data.map(|raw| from_json(&raw)).transpose()
    }

    fn list_change_nodes(&self, repo_id: &str, limit: Option<usize>) -> Result<Vec<ChangeNode>> {
        let connection = self.connect()?;
        let sql = if limit.is_some() {
            "SELECT data FROM change_nodes WHERE repo_id = ?1 ORDER BY created_at DESC LIMIT ?2"
        } else {
            "SELECT data FROM change_nodes WHERE repo_id = ?1 ORDER BY created_at DESC"
        };
        let mut statement = connection.prepare(sql)?;
        if let Some(limit) = limit {
            let rows = statement.query_map(params![repo_id, limit as i64], |row| {
                row.get::<_, String>(0)
            })?;
            collect_rows(rows)
        } else {
            let rows = statement.query_map(params![repo_id], |row| row.get::<_, String>(0))?;
            collect_rows(rows)
        }
    }

    fn update_change_node(&self, node: &ChangeNode) -> Result<()> {
        self.create_change_node(node)
    }

    fn create_worktree(&self, worktree: &ManagedWorktree) -> Result<()> {
        let connection = self.connect()?;
        let data = to_json(worktree)?;
        connection.execute(
            "INSERT OR REPLACE INTO worktrees (id, repo_id, task_id, name, created_at, status, data) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                worktree.id,
                worktree.repo_id,
                worktree.task_id,
                worktree.name,
                worktree.created_at.to_rfc3339(),
                format!("{:?}", worktree.status),
                data
            ],
        )?;
        Ok(())
    }

    fn get_worktree(&self, id: &str) -> Result<Option<ManagedWorktree>> {
        let connection = self.connect()?;
        let data = connection
            .query_row(
                "SELECT data FROM worktrees WHERE id = ?1",
                params![id],
                |row| row.get::<_, String>(0),
            )
            .optional()?;
        data.map(|raw| from_json(&raw)).transpose()
    }

    fn get_worktree_by_name(&self, repo_id: &str, name: &str) -> Result<Option<ManagedWorktree>> {
        let connection = self.connect()?;
        let data = connection
            .query_row(
                "SELECT data FROM worktrees WHERE repo_id = ?1 AND name = ?2 ORDER BY created_at DESC LIMIT 1",
                params![repo_id, name],
                |row| row.get::<_, String>(0),
            )
            .optional()?;
        data.map(|raw| from_json(&raw)).transpose()
    }

    fn list_worktrees(&self, repo_id: &str) -> Result<Vec<ManagedWorktree>> {
        let connection = self.connect()?;
        let mut statement = connection
            .prepare("SELECT data FROM worktrees WHERE repo_id = ?1 ORDER BY created_at ASC")?;
        let rows = statement.query_map(params![repo_id], |row| row.get::<_, String>(0))?;
        collect_rows(rows)
    }

    fn list_worktrees_for_task(&self, task_id: &str) -> Result<Vec<ManagedWorktree>> {
        let connection = self.connect()?;
        let mut statement = connection
            .prepare("SELECT data FROM worktrees WHERE task_id = ?1 ORDER BY created_at ASC")?;
        let rows = statement.query_map(params![task_id], |row| row.get::<_, String>(0))?;
        collect_rows(rows)
    }

    fn update_worktree(&self, worktree: &ManagedWorktree) -> Result<()> {
        self.create_worktree(worktree)
    }

    fn create_validation_artifact(&self, artifact: &ValidationArtifact) -> Result<()> {
        let connection = self.connect()?;
        let data = to_json(artifact)?;
        connection.execute(
            "INSERT OR REPLACE INTO validation_artifacts (id, repo_id, node_id, started_at, data) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                artifact.id,
                artifact.repo_id,
                artifact.node_id,
                artifact.started_at.to_rfc3339(),
                data
            ],
        )?;
        Ok(())
    }

    fn get_validation_artifact(&self, id: &str) -> Result<Option<ValidationArtifact>> {
        let connection = self.connect()?;
        let data = connection
            .query_row(
                "SELECT data FROM validation_artifacts WHERE id = ?1",
                params![id],
                |row| row.get::<_, String>(0),
            )
            .optional()?;
        data.map(|raw| from_json(&raw)).transpose()
    }

    fn list_validation_artifacts_for_node(&self, node_id: &str) -> Result<Vec<ValidationArtifact>> {
        let connection = self.connect()?;
        let mut statement = connection.prepare(
            "SELECT data FROM validation_artifacts WHERE node_id = ?1 ORDER BY started_at ASC",
        )?;
        let rows = statement.query_map(params![node_id], |row| row.get::<_, String>(0))?;
        collect_rows(rows)
    }

    fn create_promotion(&self, record: &PromotionRecord) -> Result<()> {
        let connection = self.connect()?;
        let data = to_json(record)?;
        connection.execute(
            "INSERT OR REPLACE INTO promotions (id, repo_id, node_id, target_lineage, created_at, data) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                record.id,
                record.repo_id,
                record.node_id,
                record.target_lineage,
                record.created_at.to_rfc3339(),
                data
            ],
        )?;
        Ok(())
    }

    fn list_promotions(&self, repo_id: &str) -> Result<Vec<PromotionRecord>> {
        let connection = self.connect()?;
        let mut statement = connection
            .prepare("SELECT data FROM promotions WHERE repo_id = ?1 ORDER BY created_at ASC")?;
        let rows = statement.query_map(params![repo_id], |row| row.get::<_, String>(0))?;
        collect_rows(rows)
    }

    fn list_promotions_for_node(&self, node_id: &str) -> Result<Vec<PromotionRecord>> {
        let connection = self.connect()?;
        let mut statement = connection
            .prepare("SELECT data FROM promotions WHERE node_id = ?1 ORDER BY created_at ASC")?;
        let rows = statement.query_map(params![node_id], |row| row.get::<_, String>(0))?;
        collect_rows(rows)
    }
}

#[derive(Debug, Clone)]
pub struct FsObjectStore {
    root: PathBuf,
}

impl FsObjectStore {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    fn path_for_hash(&self, hash: &str) -> Result<PathBuf> {
        if hash.len() < 4 {
            return Err(anyhow!("hash must be at least 4 characters"));
        }
        Ok(self
            .root
            .join("blake3")
            .join(&hash[0..2])
            .join(&hash[2..4])
            .join(hash))
    }
}

impl ObjectStore for FsObjectStore {
    fn put_bytes(&self, bytes: &[u8]) -> Result<ObjectHash> {
        let hash = hash_bytes(bytes);
        let path = self.path_for_hash(&hash)?;
        if !path.exists() {
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::write(&path, bytes)?;
        }
        Ok(hash)
    }

    fn get_bytes(&self, hash: &str) -> Result<Option<Vec<u8>>> {
        let path = self.path_for_hash(hash)?;
        if !path.exists() {
            return Ok(None);
        }
        Ok(Some(fs::read(path)?))
    }
}

fn to_json<T: Serialize>(value: &T) -> Result<String> {
    serde_json::to_string(value).context("failed to serialize value")
}

fn from_json<T: DeserializeOwned>(raw: &str) -> Result<T> {
    serde_json::from_str(raw).context("failed to deserialize value")
}

fn collect_rows<T, I>(rows: I) -> Result<Vec<T>>
where
    T: DeserializeOwned,
    I: IntoIterator<Item = rusqlite::Result<String>>,
{
    rows.into_iter()
        .map(|row| {
            row.map_err(anyhow::Error::from)
                .and_then(|raw| from_json(&raw))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use tempfile::tempdir;

    use super::*;
    use syft_types::{Repo, new_entity_id, now_utc};

    #[test]
    fn fs_object_store_roundtrips_bytes() {
        let dir = tempdir().unwrap();
        let store = FsObjectStore::new(dir.path());

        let hash = store.put_bytes(b"hello").unwrap();
        let bytes = store.get_bytes(&hash).unwrap().unwrap();

        assert_eq!(bytes, b"hello");
    }

    #[test]
    fn sqlite_store_persists_repo() {
        let dir = tempdir().unwrap();
        let store = SqliteMetadataStore::new(dir.path().join("meta.db"));
        let repo = Repo {
            id: new_entity_id(),
            name: "syft".to_string(),
            root_path: ".".to_string(),
            default_lineage: "main".to_string(),
            created_at: now_utc(),
        };

        store.initialize().unwrap();
        store.put_repo(&repo).unwrap();

        let loaded = store.get_repo(&repo.id).unwrap().unwrap();
        assert_eq!(loaded.name, repo.name);
    }
}
