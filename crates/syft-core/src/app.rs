use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow, bail};
use serde::{Deserialize, Serialize};
use syft_git::{current_commit, ensure_git_repo, git_dir, git_top_level};
use syft_objects::effective_capture_excludes;
use syft_store::{FsObjectStore, MetadataStore, ObjectStore, SqliteMetadataStore};
use syft_types::{
    ChangeListEntry, ChangeNode, ManagedWorktree, Repo, RepoConfig, Snapshot, Task,
    ValidationArtifact, ValidationDetails, ValidationRecord, new_entity_id, now_utc,
};
use syft_validate::LocalValidationRunner;

use crate::helpers::{ensure_git_exclude, load_capture_rules, sync_syftignore_from_gitignore};

#[derive(Debug, Clone, Serialize, Deserialize)]
struct WorktreeMarker {
    control_repo_path: String,
    worktree_id: String,
}

pub struct SyftApp {
    pub(crate) control_repo_path: PathBuf,
    pub(crate) workspace_path: PathBuf,
    pub(crate) current_worktree: Option<ManagedWorktree>,
    pub(crate) repo_config: RepoConfig,
    pub(crate) metadata_store: SqliteMetadataStore,
    pub(crate) object_store: FsObjectStore,
    pub(crate) validation_runner: LocalValidationRunner,
}

impl SyftApp {
    pub fn init_repo(
        repo_path: &Path,
        name: Option<String>,
        sync_gitignore: bool,
    ) -> Result<Self> {
        ensure_git_repo(repo_path)?;

        let repo_root = git_top_level(repo_path)?;
        let repo_name = name.unwrap_or_else(|| {
            repo_root
                .file_name()
                .map(|name| name.to_string_lossy().to_string())
                .unwrap_or_else(|| "syft-repo".to_string())
        });
        let syft_dir = repo_root.join(".syft");
        fs::create_dir_all(syft_dir.join("state"))?;
        fs::create_dir_all(syft_dir.join("cache"))?;
        fs::create_dir_all(syft_dir.join("index"))?;
        fs::create_dir_all(syft_dir.join("objects"))?;
        ensure_git_exclude(&repo_root, ".syft/")?;

        let repo = Repo {
            id: new_entity_id(),
            name: repo_name.clone(),
            root_path: repo_root.to_string_lossy().to_string(),
            default_lineage: "main".to_string(),
            created_at: now_utc(),
        };
        let config = RepoConfig {
            repo_id: repo.id.clone(),
            name: repo_name,
            default_lineage: "main".to_string(),
            object_store: "local".to_string(),
            metadata_db: "sqlite".to_string(),
            semantic_languages: vec!["rust".to_string()],
            git_bridge: true,
            capture_excludes: Vec::new(),
        };

        fs::write(syft_dir.join("repo.toml"), toml::to_string_pretty(&config)?)?;
        if sync_gitignore {
            sync_syftignore_from_gitignore(&repo_root)?;
        }

        let app = Self::open(&repo_root)?;
        app.metadata_store.initialize()?;
        app.metadata_store.put_repo(&repo)?;
        Ok(app)
    }

    pub fn open(repo_path: &Path) -> Result<Self> {
        let (control_repo_path, workspace_path, marker) = resolve_repo_context(repo_path)?;
        let config_path = control_repo_path.join(".syft/repo.toml");
        let raw = fs::read_to_string(&config_path)
            .with_context(|| format!("failed to read {}", config_path.display()))?;
        let repo_config: RepoConfig = toml::from_str(&raw)?;
        let metadata_store =
            SqliteMetadataStore::new(control_repo_path.join(".syft/state/metadata.db"));
        metadata_store.initialize()?;
        let object_store = FsObjectStore::new(control_repo_path.join(".syft/objects"));
        let current_worktree = if let Some(marker) = marker {
            let Some(worktree) = metadata_store.get_worktree(&marker.worktree_id)? else {
                bail!(
                    "managed worktree {} no longer exists in syft metadata",
                    marker.worktree_id
                );
            };
            Some(worktree)
        } else {
            None
        };

        Ok(Self {
            control_repo_path,
            workspace_path,
            current_worktree,
            repo_config,
            metadata_store,
            object_store,
            validation_runner: LocalValidationRunner,
        })
    }

    pub fn repo_config(&self) -> &RepoConfig {
        &self.repo_config
    }

    pub fn repo_path(&self) -> &Path {
        &self.control_repo_path
    }

    pub fn workspace_path(&self) -> &Path {
        &self.workspace_path
    }

    pub fn current_worktree_ref(&self) -> Option<&ManagedWorktree> {
        self.current_worktree.as_ref()
    }

    pub fn current_head_snapshot_id(&self) -> Result<Option<String>> {
        let path = self.control_repo_path.join(".syft/state/head");
        if !path.exists() {
            return Ok(None);
        }
        Ok(Some(fs::read_to_string(path)?.trim().to_string()))
    }

    pub fn current_task_id(&self) -> Result<Option<String>> {
        let path = self.control_repo_path.join(".syft/state/current_task");
        if !path.exists() {
            return Ok(None);
        }
        Ok(Some(fs::read_to_string(path)?.trim().to_string()))
    }

    pub fn get_snapshot(&self, snapshot_id: &str) -> Result<Snapshot> {
        self.metadata_store
            .get_snapshot(snapshot_id)?
            .ok_or_else(|| anyhow!("snapshot {snapshot_id} not found"))
    }

    pub fn get_task_by_id(&self, task_id: &str) -> Result<Task> {
        self.metadata_store
            .get_task(task_id)?
            .ok_or_else(|| anyhow!("task {task_id} not found"))
    }

    pub fn get_change_node(&self, node_id: &str) -> Result<ChangeNode> {
        self.metadata_store
            .get_change_node(node_id)?
            .ok_or_else(|| anyhow!("change node {node_id} not found"))
    }

    pub fn get_worktree_by_id(&self, worktree_id: &str) -> Result<ManagedWorktree> {
        self.metadata_store
            .get_worktree(worktree_id)?
            .ok_or_else(|| anyhow!("worktree {worktree_id} not found"))
    }

    pub fn get_validation_summaries(&self, node_id: &str) -> Result<Vec<String>> {
        Ok(self
            .metadata_store
            .list_validation_artifacts_for_node(node_id)?
            .into_iter()
            .map(|artifact| artifact.summary)
            .collect())
    }

    pub fn repo(&self) -> Result<Repo> {
        self.metadata_store
            .get_repo(&self.repo_config.repo_id)?
            .ok_or_else(|| anyhow!("repo {} not found", self.repo_config.repo_id))
    }

    pub fn print_json<T: Serialize>(value: &T) -> Result<String> {
        Ok(serde_json::to_string_pretty(value)?)
    }

    pub(crate) fn persist_head(&self, snapshot_id: &str) -> Result<()> {
        fs::write(
            self.control_repo_path.join(".syft/state/head"),
            format!("{snapshot_id}\n"),
        )?;
        Ok(())
    }

    pub(crate) fn persist_current_task(&self, task_id: &str) -> Result<()> {
        fs::write(
            self.control_repo_path.join(".syft/state/current_task"),
            format!("{task_id}\n"),
        )?;
        Ok(())
    }

    pub(crate) fn resolve_task_id(&self, explicit_task_id: Option<&str>) -> Result<String> {
        if let Some(task_id) = explicit_task_id {
            let task = self.get_task_by_id(task_id)?;
            return Ok(task.id);
        }

        if let Some(worktree) = &self.current_worktree {
            return Ok(worktree.task_id.clone());
        }

        let Some(current_task_id) = self.current_task_id()? else {
            bail!(
                "no task specified and no current task is set; run `syft task set-current <id>` or pass `--task`"
            );
        };

        let task = self
            .metadata_store
            .get_task(&current_task_id)?
            .ok_or_else(|| {
                anyhow!(
                    "current task {} no longer exists; run `syft task set-current <id>`",
                    current_task_id
                )
            })?;
        Ok(task.id)
    }

    pub(crate) fn resolve_base_snapshot_id(
        &self,
        explicit_snapshot_id: Option<&str>,
    ) -> Result<String> {
        if let Some(snapshot_id) = explicit_snapshot_id {
            let snapshot = self.get_snapshot(snapshot_id)?;
            return Ok(snapshot.id);
        }

        self.current_head_snapshot_id()?.ok_or_else(|| {
            anyhow!(
                "no base snapshot specified and no head snapshot is set; import or capture a snapshot first"
            )
        })
    }

    pub(crate) fn current_task_with_validation(&self) -> Result<Option<Task>> {
        let Some(task_id) = self.current_task_id()? else {
            return Ok(None);
        };
        let Some(task) = self.metadata_store.get_task(&task_id)? else {
            bail!(
                "current task {} no longer exists; run `syft task set-current <id>`",
                task_id
            );
        };
        Ok(Some(task))
    }

    pub(crate) fn effective_capture_excludes(&self) -> Result<Vec<String>> {
        let mut rules = self.repo_config.capture_excludes.clone();
        rules.extend(load_capture_rules(self.workspace_path.join(".gitignore"))?);
        rules.extend(load_capture_rules(self.workspace_path.join(".syftignore"))?);
        Ok(effective_capture_excludes(&rules))
    }

    pub(crate) fn validation_records_for_node(
        &self,
        node_id: &str,
        include_logs: bool,
    ) -> Result<Vec<ValidationRecord>> {
        self.metadata_store
            .list_validation_artifacts_for_node(node_id)?
            .into_iter()
            .map(|artifact| {
                let details = if include_logs {
                    self.load_validation_details(&artifact.details_ref)?
                } else {
                    None
                };
                Ok(ValidationRecord { artifact, details })
            })
            .collect()
    }

    pub(crate) fn load_validation_details(
        &self,
        details_ref: &Option<String>,
    ) -> Result<Option<ValidationDetails>> {
        let Some(details_ref) = details_ref else {
            return Ok(None);
        };
        let Some(bytes) = self.object_store.get_bytes(details_ref)? else {
            return Ok(None);
        };
        Ok(Some(serde_json::from_slice(&bytes)?))
    }

    pub(crate) fn latest_validation_for_node(
        &self,
        node_id: &str,
    ) -> Result<Option<ValidationArtifact>> {
        Ok(self
            .metadata_store
            .list_validation_artifacts_for_node(node_id)?
            .into_iter()
            .last())
    }

    pub(crate) fn promotion_state_for_node(&self, node_id: &str) -> Result<Option<String>> {
        Ok(self
            .metadata_store
            .list_promotions_for_node(node_id)?
            .into_iter()
            .last()
            .map(|promotion| format!("promoted to {}", promotion.target_lineage)))
    }

    pub(crate) fn task_map(&self) -> Result<BTreeMap<String, Task>> {
        Ok(self
            .metadata_store
            .list_tasks(&self.repo_config.repo_id)?
            .into_iter()
            .map(|task| (task.id.clone(), task))
            .collect())
    }

    pub(crate) fn worktree_map(&self) -> Result<BTreeMap<String, ManagedWorktree>> {
        Ok(self
            .metadata_store
            .list_worktrees(&self.repo_config.repo_id)?
            .into_iter()
            .map(|worktree| (worktree.id.clone(), worktree))
            .collect())
    }

    pub(crate) fn change_list_entry(
        &self,
        change: ChangeNode,
        task_map: &BTreeMap<String, Task>,
        worktree_map: &BTreeMap<String, ManagedWorktree>,
    ) -> Result<ChangeListEntry> {
        let latest_validation = self.latest_validation_for_node(&change.id)?;
        let promotion_state = self.promotion_state_for_node(&change.id)?;
        let task_title = task_map
            .get(&change.task_id)
            .map(|task| task.title.clone())
            .unwrap_or_else(|| "<unknown task>".to_string());
        let worktree_name = change
            .worktree_id
            .as_ref()
            .and_then(|id| worktree_map.get(id))
            .map(|worktree| worktree.name.clone());
        Ok(ChangeListEntry {
            node_id: change.id.clone(),
            title: change.title,
            status: change.status,
            task_id: change.task_id,
            task_title,
            worktree_name,
            risk_score: change.risk.score,
            latest_validation_summary: latest_validation
                .as_ref()
                .map(|artifact| artifact.summary.clone()),
            latest_validation_status: latest_validation.map(|artifact| artifact.status),
            promotion_state,
            created_at: change.created_at,
            updated_at: change.updated_at,
        })
    }

    pub(crate) fn resolve_worktree_by_id_or_name(&self, id_or_name: &str) -> Result<ManagedWorktree> {
        if let Some(worktree) = self.metadata_store.get_worktree(id_or_name)? {
            return Ok(worktree);
        }
        self.metadata_store
            .get_worktree_by_name(&self.repo_config.repo_id, id_or_name)?
            .ok_or_else(|| anyhow!("worktree {id_or_name} not found"))
    }
}

fn resolve_repo_context(start_path: &Path) -> Result<(PathBuf, PathBuf, Option<WorktreeMarker>)> {
    if start_path.join(".syft/repo.toml").exists() {
        let root = start_path.to_path_buf();
        return Ok((root.clone(), root, None));
    }

    let workspace_path = git_top_level(start_path)?;
    if workspace_path.join(".syft/repo.toml").exists() {
        return Ok((workspace_path.clone(), workspace_path, None));
    }

    let git_dir_path = absolute_git_dir(start_path, &workspace_path)?;
    let marker_path = git_dir_path.join("syft-worktree.toml");
    if marker_path.exists() {
        let marker: WorktreeMarker = toml::from_str(&fs::read_to_string(&marker_path)?)?;
        let control_repo_path = PathBuf::from(marker.control_repo_path.clone());
        return Ok((control_repo_path, workspace_path, Some(marker)));
    }

    bail!(
        "syft is not initialized here; run `syft init` from the main repo root first"
    )
}

fn absolute_git_dir(start_path: &Path, workspace_path: &Path) -> Result<PathBuf> {
    let git_dir_path = git_dir(start_path)?;
    if git_dir_path.is_absolute() {
        Ok(git_dir_path)
    } else {
        Ok(workspace_path.join(git_dir_path))
    }
}

pub(crate) fn write_worktree_marker(
    workspace_path: &Path,
    control_repo_path: &Path,
    worktree_id: &str,
) -> Result<()> {
    let git_dir_path = absolute_git_dir(workspace_path, workspace_path)?;
    let marker = WorktreeMarker {
        control_repo_path: control_repo_path.to_string_lossy().to_string(),
        worktree_id: worktree_id.to_string(),
    };
    fs::write(
        git_dir_path.join("syft-worktree.toml"),
        toml::to_string_pretty(&marker)?,
    )?;
    Ok(())
}

pub fn init_or_open(repo_path: &Path) -> Result<SyftApp> {
    if repo_path.join(".syft/repo.toml").exists() {
        SyftApp::open(repo_path)
    } else {
        SyftApp::init_repo(repo_path, None, false)
    }
}

pub fn import_head(repo_path: &Path) -> Result<Snapshot> {
    let app = SyftApp::open(repo_path)?;
    let commit = current_commit(app.workspace_path())?;
    crate::contracts::RepoService::import_git_commit(&app, &commit)
}
