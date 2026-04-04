use std::collections::BTreeMap;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow, bail};
use serde::Serialize;
use syft_git::{
    capture_worktree_snapshot, current_commit, ensure_git_repo, export_snapshot_to_git_commit,
    import_git_commit, materialize_snapshot_to,
};
use syft_objects::{diff_snapshot_indices, snapshot_index};
use syft_semantic::diff_snapshots;
use syft_store::{FsObjectStore, MetadataStore, ObjectStore, SqliteMetadataStore};
use syft_types::{
    Author, ChangeDetail, ChangeHeadline, ChangeListEntry, ChangeNode, ChangeNodeStatus,
    DiffSummary, HistoryEntry, HistoryQuery, PatchOp, PromotionHeadline, PromotionRecord,
    Provenance, Repo, RepoConfig, RepoStatusSummary, RiskReport, Snapshot, SnapshotDetail,
    SnapshotListEntry, SnapshotSource, Task, TaskPriority, TaskStatus, ValidationArtifact,
    ValidationDetails, ValidationPlan, ValidationRecord, ValidationStatus, new_entity_id, now_utc,
};
use syft_validate::{LocalValidationRunner, ValidationRunner};

pub struct SyftApp {
    repo_path: PathBuf,
    repo_config: RepoConfig,
    metadata_store: SqliteMetadataStore,
    object_store: FsObjectStore,
    validation_runner: LocalValidationRunner,
}

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
    pub provenance: Option<Provenance>,
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

impl SyftApp {
    pub fn init_repo(repo_path: &Path, name: Option<String>) -> Result<Self> {
        ensure_git_repo(repo_path)?;

        let repo_name = name.unwrap_or_else(|| {
            repo_path
                .file_name()
                .map(|name| name.to_string_lossy().to_string())
                .unwrap_or_else(|| "syft-repo".to_string())
        });
        let syft_dir = repo_path.join(".syft");
        fs::create_dir_all(syft_dir.join("state"))?;
        fs::create_dir_all(syft_dir.join("cache"))?;
        fs::create_dir_all(syft_dir.join("index"))?;
        fs::create_dir_all(syft_dir.join("objects"))?;
        ensure_git_exclude(repo_path, ".syft/")?;

        let repo = Repo {
            id: new_entity_id(),
            name: repo_name.clone(),
            root_path: repo_path.to_string_lossy().to_string(),
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
        };

        fs::write(syft_dir.join("repo.toml"), toml::to_string_pretty(&config)?)?;

        let app = Self::open(repo_path)?;
        app.metadata_store.initialize()?;
        app.metadata_store.put_repo(&repo)?;
        Ok(app)
    }

    pub fn open(repo_path: &Path) -> Result<Self> {
        let config_path = repo_path.join(".syft/repo.toml");
        let raw = fs::read_to_string(&config_path)
            .with_context(|| format!("failed to read {}", config_path.display()))?;
        let repo_config: RepoConfig = toml::from_str(&raw)?;
        let metadata_store = SqliteMetadataStore::new(repo_path.join(".syft/state/metadata.db"));
        metadata_store.initialize()?;
        let object_store = FsObjectStore::new(repo_path.join(".syft/objects"));
        Ok(Self {
            repo_path: repo_path.to_path_buf(),
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
        &self.repo_path
    }

    pub fn current_head_snapshot_id(&self) -> Result<Option<String>> {
        let path = self.repo_path.join(".syft/state/head");
        if !path.exists() {
            return Ok(None);
        }
        Ok(Some(fs::read_to_string(path)?.trim().to_string()))
    }

    pub fn current_task_id(&self) -> Result<Option<String>> {
        let path = self.repo_path.join(".syft/state/current_task");
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

    fn persist_head(&self, snapshot_id: &str) -> Result<()> {
        fs::write(
            self.repo_path.join(".syft/state/head"),
            format!("{snapshot_id}\n"),
        )?;
        Ok(())
    }

    fn persist_current_task(&self, task_id: &str) -> Result<()> {
        fs::write(
            self.repo_path.join(".syft/state/current_task"),
            format!("{task_id}\n"),
        )?;
        Ok(())
    }

    fn resolve_task_id(&self, explicit_task_id: Option<&str>) -> Result<String> {
        if let Some(task_id) = explicit_task_id {
            let task = self.get_task_by_id(task_id)?;
            return Ok(task.id);
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

    fn resolve_base_snapshot_id(&self, explicit_snapshot_id: Option<&str>) -> Result<String> {
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

    fn current_task_with_validation(&self) -> Result<Option<Task>> {
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

    fn validation_records_for_node(
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

    fn load_validation_details(
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

    fn latest_validation_for_node(&self, node_id: &str) -> Result<Option<ValidationArtifact>> {
        Ok(self
            .metadata_store
            .list_validation_artifacts_for_node(node_id)?
            .into_iter()
            .last())
    }

    fn promotion_state_for_node(&self, node_id: &str) -> Result<Option<String>> {
        Ok(self
            .metadata_store
            .list_promotions_for_node(node_id)?
            .into_iter()
            .last()
            .map(|promotion| format!("promoted to {}", promotion.target_lineage)))
    }

    fn task_map(&self) -> Result<BTreeMap<String, Task>> {
        Ok(self
            .metadata_store
            .list_tasks(&self.repo_config.repo_id)?
            .into_iter()
            .map(|task| (task.id.clone(), task))
            .collect())
    }

    fn change_list_entry(
        &self,
        change: ChangeNode,
        task_map: &BTreeMap<String, Task>,
    ) -> Result<ChangeListEntry> {
        let latest_validation = self.latest_validation_for_node(&change.id)?;
        let promotion_state = self.promotion_state_for_node(&change.id)?;
        let task_title = task_map
            .get(&change.task_id)
            .map(|task| task.title.clone())
            .unwrap_or_else(|| "<unknown task>".to_string());
        Ok(ChangeListEntry {
            node_id: change.id.clone(),
            title: change.title,
            status: change.status,
            task_id: change.task_id,
            task_title,
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

    fn diff_summary(
        &self,
        from_snapshot_id: Option<String>,
        to_snapshot_id: Option<String>,
        change_node_id: Option<String>,
        mut ops: Vec<PatchOp>,
    ) -> DiffSummary {
        ops.sort_by(|left, right| left.path.cmp(&right.path));
        let mut counts = BTreeMap::new();
        for op in &ops {
            *counts.entry(format!("{:?}", op.kind)).or_insert(0) += 1;
        }
        DiffSummary {
            from_snapshot_id,
            to_snapshot_id,
            change_node_id,
            ops,
            counts,
        }
    }
}

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
        let parent_snapshot_ids = self
            .current_head_snapshot_id()?
            .map(|id| vec![id])
            .unwrap_or_default();
        let (snapshot, _) = capture_worktree_snapshot(
            &self.repo_path,
            &self.repo_config,
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

impl QueryService for SyftApp {
    fn status(&self) -> Result<RepoStatusSummary> {
        let repo = self.repo()?;
        let snapshots = self
            .metadata_store
            .list_snapshots(&self.repo_config.repo_id)?;
        let tasks = self.metadata_store.list_tasks(&self.repo_config.repo_id)?;
        let changes = self
            .metadata_store
            .list_change_nodes(&self.repo_config.repo_id, None)?;
        let promotions = self
            .metadata_store
            .list_promotions(&self.repo_config.repo_id)?;

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

        changes
            .into_iter()
            .filter(|change| change.task_id == task.id)
            .map(|change| self.change_list_entry(change, &task_map))
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
                created_at: snapshot.created_at,
                parent_count: snapshot.parent_snapshot_ids.len(),
            })
            .collect())
    }

    fn show_snapshot(&self, snapshot_id: &str) -> Result<SnapshotDetail> {
        let snapshot = self.get_snapshot(snapshot_id)?;
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
            snapshot,
            changed_file_count_from_parent,
        })
    }

    fn diff_snapshots(&self, from_snapshot_id: &str, to_snapshot_id: &str) -> Result<DiffSummary> {
        let from_snapshot = self.get_snapshot(from_snapshot_id)?;
        let to_snapshot = self.get_snapshot(to_snapshot_id)?;
        let from_index = snapshot_index(&from_snapshot.root_tree_hash, &self.object_store)?;
        let to_index = snapshot_index(&to_snapshot.root_tree_hash, &self.object_store)?;
        let ops = diff_snapshot_indices(&from_index, &to_index);
        Ok(self.diff_summary(Some(from_snapshot.id), Some(to_snapshot.id), None, ops))
    }

    fn list_changes(&self) -> Result<Vec<ChangeListEntry>> {
        let changes = self
            .metadata_store
            .list_change_nodes(&self.repo_config.repo_id, None)?;
        let task_map = self.task_map()?;

        changes
            .into_iter()
            .map(|change| self.change_list_entry(change, &task_map))
            .collect()
    }

    fn show_change(&self, node_id: &str, include_logs: bool) -> Result<ChangeDetail> {
        let node = self.get_change_node(node_id)?;
        let task = self.metadata_store.get_task(&node.task_id)?;
        let validations = self.validation_records_for_node(node_id, include_logs)?;
        let promotions = self.metadata_store.list_promotions_for_node(node_id)?;

        Ok(ChangeDetail {
            node,
            task,
            validations,
            promotions,
        })
    }

    fn diff_change(&self, node_id: &str) -> Result<DiffSummary> {
        let node = self.get_change_node(node_id)?;
        Ok(self.diff_summary(
            Some(node.base_snapshot_id.clone()),
            Some(node.result_snapshot_id.clone()),
            Some(node.id),
            node.patch_ops,
        ))
    }
}

pub fn init_or_open(repo_path: &Path) -> Result<SyftApp> {
    if repo_path.join(".syft/repo.toml").exists() {
        SyftApp::open(repo_path)
    } else {
        SyftApp::init_repo(repo_path, None)
    }
}

pub fn import_head(repo_path: &Path) -> Result<Snapshot> {
    let app = SyftApp::open(repo_path)?;
    let commit = current_commit(repo_path)?;
    app.import_git_commit(&commit)
}

fn symbol_names_for_change(change: &ChangeNode) -> Vec<String> {
    let mut seen = BTreeMap::new();
    for symbol in change
        .semantic_delta
        .touched_symbols
        .iter()
        .chain(change.semantic_delta.added_symbols.iter())
        .chain(change.semantic_delta.removed_symbols.iter())
    {
        seen.insert(symbol.id.path.clone(), ());
    }
    seen.into_keys().collect()
}

fn provenance_summary(provenance: &Provenance) -> String {
    match &provenance.author {
        Author::Human { user_id } => format!("human:{user_id}"),
        Author::Agent { agent_id } => format!("agent:{agent_id}"),
        Author::Tool { tool_name } => format!("tool:{tool_name}"),
    }
}

fn snapshot_source_summary(source: &SnapshotSource) -> String {
    match source {
        SnapshotSource::ImportedFromGit { commit_sha } => {
            format!("git import {}", shorten_id(commit_sha))
        }
        SnapshotSource::MaterializedByHuman => "worktree capture".to_string(),
        SnapshotSource::MaterializedByAgent => "agent materialization".to_string(),
        SnapshotSource::MaterializedByCompose => "composed snapshot".to_string(),
    }
}

fn shorten_id(value: &str) -> String {
    value.chars().take(8).collect()
}

fn calculate_risk(
    semantic_delta: &syft_types::SemanticDelta,
    artifacts: &[syft_types::ValidationArtifact],
) -> RiskReport {
    let mut score = 10u8;
    let mut reasons = Vec::new();

    if semantic_delta.changed_public_api {
        score = score.saturating_add(40);
        reasons.push("public API changed".to_string());
    }
    if !semantic_delta.changed_dependencies.is_empty() {
        score = score.saturating_add(20);
        reasons.push("dependency metadata changed".to_string());
    }
    let failing_validations = artifacts
        .iter()
        .filter(|artifact| !matches!(artifact.status, ValidationStatus::Passed))
        .count() as u8;
    if failing_validations > 0 {
        score = score.saturating_add(20 + failing_validations.saturating_mul(10));
        reasons.push(format!("{failing_validations} validation step(s) failed"));
    }
    if reasons.is_empty() {
        reasons.push("low risk bootstrap heuristic".to_string());
    }

    RiskReport {
        score: score.min(100),
        reasons,
        impacted_domains: semantic_delta.changed_files.clone(),
    }
}

fn default_provenance() -> Provenance {
    let user = env::var("USER").unwrap_or_else(|_| "unknown".to_string());
    Provenance {
        author: Author::Human { user_id: user },
        ..Provenance::default()
    }
}

fn ensure_git_exclude(repo_path: &Path, entry: &str) -> Result<()> {
    let exclude_path = repo_path.join(".git/info/exclude");
    let current = fs::read_to_string(&exclude_path).unwrap_or_default();
    if current.lines().any(|line| line.trim() == entry) {
        return Ok(());
    }

    let mut next = current;
    if !next.ends_with('\n') && !next.is_empty() {
        next.push('\n');
    }
    next.push_str(entry);
    next.push('\n');
    fs::write(exclude_path, next)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::process::Command;

    use tempfile::tempdir;

    use super::*;

    #[test]
    fn init_repo_writes_control_dir() {
        let dir = tempdir().unwrap();
        setup_repo(dir.path());

        let app = SyftApp::init_repo(dir.path(), Some("fixture".to_string())).unwrap();
        let snapshot = app.import_git_commit("HEAD").unwrap();

        assert!(dir.path().join(".syft/repo.toml").exists());
        assert!(!snapshot.id.is_empty());
    }

    #[test]
    fn status_reports_missing_head_snapshot() {
        let dir = tempdir().unwrap();
        setup_repo(dir.path());

        let app = SyftApp::init_repo(dir.path(), Some("fixture".to_string())).unwrap();
        let status = app.status().unwrap();

        assert!(status.current_head_snapshot_id.is_none());
        assert!(
            status
                .attention_needed
                .iter()
                .any(|item| item.contains("no head snapshot"))
        );
    }

    #[test]
    fn status_reports_head_snapshot_without_changes() {
        let dir = tempdir().unwrap();
        setup_repo(dir.path());

        let app = SyftApp::init_repo(dir.path(), Some("fixture".to_string())).unwrap();
        let snapshot = app.import_git_commit("HEAD").unwrap();
        let status = app.status().unwrap();

        assert_eq!(
            status.current_head_snapshot_id.as_deref(),
            Some(snapshot.id.as_str())
        );
        assert!(status.change_counts.is_empty());
        assert!(status.attention_needed.is_empty());
    }

    fn setup_repo(repo_path: &Path) {
        run_git(repo_path, &["init"]).unwrap();
        fs::write(
            repo_path.join("Cargo.toml"),
            "[package]\nname=\"fixture\"\nversion=\"0.1.0\"\nedition=\"2024\"\n",
        )
        .unwrap();
        fs::create_dir_all(repo_path.join("src")).unwrap();
        fs::write(repo_path.join("src/main.rs"), "fn main() {}\n").unwrap();
        run_git(repo_path, &["add", "."]).unwrap();
        run_git(repo_path, &["config", "user.email", "test@example.com"]).unwrap();
        run_git(repo_path, &["config", "user.name", "Test User"]).unwrap();
        run_git(repo_path, &["commit", "-m", "init"]).unwrap();
    }

    fn run_git(repo_path: &Path, args: &[&str]) -> Result<()> {
        let status = Command::new("git")
            .args(args)
            .current_dir(repo_path)
            .status()?;
        if !status.success() {
            bail!("git {:?} failed", args);
        }
        Ok(())
    }
}
