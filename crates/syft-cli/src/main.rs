use std::collections::BTreeMap;
use std::env;

use anyhow::{Result, bail};
use clap::{Args, Parser, Subcommand};
use syft_core::{
    ChangeService, CreateTaskInput, PromoteChangeInput, ProposeChangeInput, QueryService,
    RepoService, SyftApp, TaskService,
};
use syft_types::{
    ChangeDetail, ChangeListEntry, ChangeNode, DiffSummary, HistoryEntry, HistoryQuery,
    PatchOpKind, PromotionRecord, RepoStatusSummary, Snapshot, SnapshotDetail, SnapshotListEntry,
    Task, TaskPriority, ValidationPlan, ValidationRecord,
};

#[derive(Parser, Debug)]
#[command(name = "syft")]
#[command(about = "AI-native version control bootstrap CLI")]
struct Cli {
    #[arg(long, global = true)]
    json: bool,
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand, Debug)]
enum Commands {
    Init(InitArgs),
    Status,
    History(HistoryArgs),
    Repo(RepoArgs),
    Snapshot(SnapshotArgs),
    Task(TaskArgs),
    Change(ChangeArgs),
}

#[derive(Args, Debug)]
struct InitArgs {
    #[arg(long)]
    name: Option<String>,
}

#[derive(Args, Debug)]
struct HistoryArgs {
    #[arg(long)]
    task: Option<String>,
    #[arg(long)]
    symbol: Option<String>,
    #[arg(long, default_value_t = 20)]
    limit: usize,
}

#[derive(Args, Debug)]
struct RepoArgs {
    #[command(subcommand)]
    command: RepoCommands,
}

#[derive(Subcommand, Debug)]
enum RepoCommands {
    ImportGit {
        #[arg(long, default_value = "HEAD")]
        commit: String,
    },
}

#[derive(Args, Debug)]
struct SnapshotArgs {
    #[command(subcommand)]
    command: SnapshotCommands,
}

#[derive(Subcommand, Debug)]
enum SnapshotCommands {
    Capture,
    List,
    Show {
        snapshot_id: String,
    },
    Diff {
        from_snapshot_id: String,
        to_snapshot_id: String,
    },
}

#[derive(Args, Debug)]
struct TaskArgs {
    #[command(subcommand)]
    command: TaskCommands,
}

#[derive(Subcommand, Debug)]
enum TaskCommands {
    Create(TaskCreateArgs),
    List,
    Show { task_id: String },
    Current,
    SetCurrent { task_id: String },
    Changes { task_id: String },
}

#[derive(Args, Debug)]
struct TaskCreateArgs {
    #[arg(long)]
    title: String,
    #[arg(long, default_value = "")]
    description: String,
    #[arg(long = "acceptance")]
    acceptance_criteria: Vec<String>,
    #[arg(long = "constraint")]
    constraints: Vec<String>,
    #[arg(long = "label")]
    labels: Vec<String>,
    #[arg(long, default_value = "medium")]
    priority: String,
}

#[derive(Args, Debug)]
struct ChangeArgs {
    #[command(subcommand)]
    command: ChangeCommands,
}

#[derive(Subcommand, Debug)]
enum ChangeCommands {
    Propose(ChangeProposeArgs),
    Validate(ChangeValidateArgs),
    Promote(ChangePromoteArgs),
    List,
    Show(ChangeShowArgs),
    Diff { node_id: String },
    Latest(ChangeLatestArgs),
}

#[derive(Args, Debug)]
struct ChangeProposeArgs {
    #[arg(long = "task")]
    task_id: Option<String>,
    #[arg(long)]
    title: String,
    #[arg(long)]
    intent: String,
    #[arg(long)]
    base: Option<String>,
    #[arg(long)]
    result: String,
    #[arg(long)]
    rationale: Option<String>,
    #[arg(long = "tag")]
    tags: Vec<String>,
}

#[derive(Args, Debug)]
struct ChangeValidateArgs {
    node_id: String,
    #[arg(long)]
    tests: bool,
    #[arg(long)]
    lint: bool,
    #[arg(long)]
    typecheck: bool,
}

#[derive(Args, Debug)]
struct ChangePromoteArgs {
    node_id: String,
    #[arg(long = "to")]
    target_lineage: String,
    #[arg(long)]
    approved_by: Option<String>,
    #[arg(long)]
    notes: Option<String>,
    #[arg(long)]
    no_export: bool,
}

#[derive(Args, Debug)]
struct ChangeShowArgs {
    node_id: String,
    #[arg(long)]
    logs: bool,
}

#[derive(Args, Debug)]
struct ChangeLatestArgs {
    #[arg(long)]
    task: Option<String>,
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let cwd = env::current_dir()?;

    match cli.command {
        Commands::Init(args) => {
            let app = SyftApp::init_repo(&cwd, args.name)?;
            emit(
                cli.json,
                &app.repo_config(),
                &format!(
                    "initialized syft repo {} ({})",
                    app.repo_config().name,
                    app.repo_config().repo_id
                ),
            )?;
        }
        Commands::Status => {
            let app = SyftApp::open(&cwd)?;
            emit_status(cli.json, &app.status()?)?;
        }
        Commands::History(args) => {
            let app = SyftApp::open(&cwd)?;
            emit_history(
                cli.json,
                &app.history(&HistoryQuery {
                    task_id: args.task,
                    symbol: args.symbol,
                    limit: args.limit,
                })?,
            )?;
        }
        Commands::Repo(args) => {
            let app = SyftApp::open(&cwd)?;
            match args.command {
                RepoCommands::ImportGit { commit } => {
                    emit_snapshot(cli.json, &app.import_git_commit(&commit)?)?;
                }
            }
        }
        Commands::Snapshot(args) => {
            let app = SyftApp::open(&cwd)?;
            match args.command {
                SnapshotCommands::Capture => {
                    emit_snapshot(cli.json, &app.capture_snapshot()?)?;
                }
                SnapshotCommands::List => {
                    emit_snapshot_list(cli.json, &app.list_snapshots()?)?;
                }
                SnapshotCommands::Show { snapshot_id } => {
                    emit_snapshot_detail(cli.json, &app.show_snapshot(&snapshot_id)?)?;
                }
                SnapshotCommands::Diff {
                    from_snapshot_id,
                    to_snapshot_id,
                } => {
                    emit_diff_summary(
                        cli.json,
                        &app.diff_snapshots(&from_snapshot_id, &to_snapshot_id)?,
                    )?;
                }
            }
        }
        Commands::Task(args) => {
            let app = SyftApp::open(&cwd)?;
            match args.command {
                TaskCommands::Create(args) => {
                    emit_task(
                        cli.json,
                        &app.create_task(CreateTaskInput {
                            title: args.title,
                            description: args.description,
                            acceptance_criteria: args.acceptance_criteria,
                            constraints: args.constraints,
                            labels: args.labels,
                            priority: parse_priority(&args.priority)?,
                        })?,
                    )?;
                }
                TaskCommands::List => {
                    let tasks = app.list_tasks()?;
                    if cli.json {
                        emit(true, &tasks, "")?;
                    } else if tasks.is_empty() {
                        println!("no tasks");
                    } else {
                        for task in tasks {
                            println!(
                                "{}  {}  {:?}  {:?}",
                                task.id, task.title, task.status, task.priority
                            );
                        }
                    }
                }
                TaskCommands::Show { task_id } => {
                    emit_task_detail(cli.json, &app.show_task(&task_id)?)?;
                }
                TaskCommands::Current => match app.get_current_task()? {
                    Some(task) => emit_task_detail(cli.json, &task)?,
                    None if cli.json => println!("null"),
                    None => println!("no current task"),
                },
                TaskCommands::SetCurrent { task_id } => {
                    let task = app.set_current_task(&task_id)?;
                    if cli.json {
                        emit(true, &task, "")?;
                    } else {
                        println!("current task set: {} ({})", task.title, task.id);
                    }
                }
                TaskCommands::Changes { task_id } => {
                    emit_change_list(cli.json, &app.list_changes_for_task(&task_id)?)?;
                }
            }
        }
        Commands::Change(args) => {
            let app = SyftApp::open(&cwd)?;
            match args.command {
                ChangeCommands::Propose(args) => {
                    emit_change(
                        cli.json,
                        &app.propose_change(ProposeChangeInput {
                            task_id: args.task_id,
                            title: args.title,
                            intent: args.intent,
                            rationale: args.rationale,
                            base_snapshot_id: args.base,
                            result_snapshot_id: args.result,
                            provenance: None,
                            tags: args.tags,
                        })?,
                    )?;
                }
                ChangeCommands::Validate(args) => {
                    let mut plan = ValidationPlan {
                        run_tests: args.tests,
                        run_lint: args.lint,
                        run_typecheck: args.typecheck,
                    };
                    if !plan.any_enabled() {
                        plan = ValidationPlan {
                            run_tests: true,
                            run_lint: true,
                            run_typecheck: true,
                        };
                    }
                    let node = app.validate_change(&args.node_id, &plan)?;
                    emit_validated_change(cli.json, &node, &plan)?;
                }
                ChangeCommands::Promote(args) => {
                    let approved_by = args
                        .approved_by
                        .or_else(|| env::var("USER").ok())
                        .unwrap_or_else(|| "unknown".to_string());
                    emit_promotion(
                        cli.json,
                        &app.promote_change(PromoteChangeInput {
                            node_id: args.node_id,
                            target_lineage: args.target_lineage,
                            approved_by,
                            notes: args.notes,
                            export_to_git: !args.no_export,
                        })?,
                    )?;
                }
                ChangeCommands::List => {
                    emit_change_list(cli.json, &app.list_changes()?)?;
                }
                ChangeCommands::Show(args) => {
                    emit_change_detail(
                        cli.json,
                        &app.show_change(&args.node_id, args.logs)?,
                        args.logs,
                    )?;
                }
                ChangeCommands::Diff { node_id } => {
                    emit_diff_summary(cli.json, &app.diff_change(&node_id)?)?;
                }
                ChangeCommands::Latest(args) => {
                    emit_change_detail(cli.json, &app.latest_change(args.task.as_deref())?, false)?;
                }
            }
        }
    }

    Ok(())
}

fn parse_priority(input: &str) -> Result<TaskPriority> {
    match input.to_ascii_lowercase().as_str() {
        "low" => Ok(TaskPriority::Low),
        "medium" => Ok(TaskPriority::Medium),
        "high" => Ok(TaskPriority::High),
        "critical" => Ok(TaskPriority::Critical),
        other => bail!("unknown priority {other}"),
    }
}

fn emit_snapshot(as_json: bool, snapshot: &Snapshot) -> Result<()> {
    emit(
        as_json,
        snapshot,
        &format!(
            "snapshot {} created with root {}",
            snapshot.id, snapshot.root_tree_hash
        ),
    )
}

fn emit_task(as_json: bool, task: &Task) -> Result<()> {
    emit(
        as_json,
        task,
        &format!("task {} created: {}", task.id, task.title),
    )
}

fn emit_task_detail(as_json: bool, task: &Task) -> Result<()> {
    if as_json {
        return emit(true, task, "");
    }

    println!("task: {} ({})", task.title, task.id);
    println!("status: {:?}  priority: {:?}", task.status, task.priority);
    println!(
        "description: {}",
        if task.description.is_empty() {
            "<none>"
        } else {
            &task.description
        }
    );
    println!(
        "acceptance: {}",
        if task.acceptance_criteria.is_empty() {
            "<none>".to_string()
        } else {
            task.acceptance_criteria.join(" | ")
        }
    );
    println!(
        "constraints: {}",
        if task.constraints.is_empty() {
            "<none>".to_string()
        } else {
            task.constraints.join(" | ")
        }
    );
    println!(
        "labels: {}",
        if task.labels.is_empty() {
            "<none>".to_string()
        } else {
            task.labels.join(", ")
        }
    );
    println!("created: {}", task.created_at.to_rfc3339());
    println!("updated: {}", task.updated_at.to_rfc3339());
    Ok(())
}

fn emit_change(as_json: bool, node: &ChangeNode) -> Result<()> {
    emit(
        as_json,
        node,
        &format!(
            "change {} {} status={:?} validation={} risk={} validations={}",
            node.id,
            node.title,
            node.status,
            validation_outcome(node),
            node.risk.score,
            node.validation_artifact_ids.len()
        ),
    )
}

fn emit_validated_change(as_json: bool, node: &ChangeNode, plan: &ValidationPlan) -> Result<()> {
    let run_count = usize::from(plan.run_typecheck)
        + usize::from(plan.run_tests)
        + usize::from(plan.run_lint);
    emit(
        as_json,
        node,
        &format!(
            "change {} {} status={:?} validation={} ran={} risk={} validations={}",
            node.id,
            node.title,
            node.status,
            validation_outcome(node),
            run_count,
            node.risk.score,
            node.validation_artifact_ids.len()
        ),
    )
}

fn validation_outcome(node: &ChangeNode) -> &'static str {
    match node.status {
        syft_types::ChangeNodeStatus::Validated | syft_types::ChangeNodeStatus::Approved => {
            "passed"
        }
        syft_types::ChangeNodeStatus::Rejected => "failed",
        _ => "pending",
    }
}

fn emit_promotion(as_json: bool, record: &PromotionRecord) -> Result<()> {
    emit(
        as_json,
        record,
        &format!(
            "promoted change {} to {}",
            record.node_id, record.target_lineage
        ),
    )
}

fn emit_status(as_json: bool, status: &RepoStatusSummary) -> Result<()> {
    if as_json {
        return emit(true, status, "");
    }

    println!("repo: {} ({})", status.repo_name, status.repo_id);
    println!(
        "head snapshot: {}",
        status
            .current_head_snapshot_id
            .as_deref()
            .unwrap_or("<none>")
    );
    println!(
        "latest snapshot: {}",
        status
            .latest_snapshot_at
            .map(|value| value.to_rfc3339())
            .unwrap_or_else(|| "<none>".to_string())
    );
    println!("task counts: {}", join_counts(&status.task_counts));
    println!("change counts: {}", join_counts(&status.change_counts));
    println!(
        "latest promoted: {}",
        status
            .latest_promoted_change
            .as_ref()
            .map(|promotion| format!("{} -> {}", promotion.node_id, promotion.target_lineage))
            .unwrap_or_else(|| "<none>".to_string())
    );
    println!(
        "latest validated/failed: {}",
        status
            .latest_validated_or_failed_change
            .as_ref()
            .map(|change| {
                format!(
                    "{} [{}] {}",
                    change.node_id,
                    format!("{:?}", change.status),
                    change
                        .validation_summary
                        .clone()
                        .unwrap_or_else(|| "<no validations>".to_string())
                )
            })
            .unwrap_or_else(|| "<none>".to_string())
    );
    if !status.attention_needed.is_empty() {
        println!("attention:");
        for item in &status.attention_needed {
            println!("  - {item}");
        }
    }
    Ok(())
}

fn emit_history(as_json: bool, entries: &[HistoryEntry]) -> Result<()> {
    if as_json {
        return emit(true, &entries, "");
    }

    if entries.is_empty() {
        println!("no history entries");
        return Ok(());
    }

    for entry in entries {
        println!(
            "{}  {}  task={}  files={}  validation={}  promotion={}",
            entry.node_id,
            entry.title,
            entry.task_title,
            entry.changed_file_count,
            entry
                .validation_summary
                .clone()
                .unwrap_or_else(|| "<none>".to_string()),
            entry
                .promotion_state
                .clone()
                .unwrap_or_else(|| "<none>".to_string())
        );
        if !entry.touched_symbols.is_empty() {
            println!("  symbols: {}", entry.touched_symbols.join(", "));
        }
    }
    Ok(())
}

fn emit_snapshot_list(as_json: bool, snapshots: &[SnapshotListEntry]) -> Result<()> {
    if as_json {
        return emit(true, &snapshots, "");
    }

    if snapshots.is_empty() {
        println!("no snapshots");
        return Ok(());
    }

    for snapshot in snapshots {
        println!(
            "{}  {}  parents={}  labels={}  {}",
            snapshot.id,
            snapshot.source,
            snapshot.parent_count,
            snapshot.label_summary,
            snapshot.created_at.to_rfc3339()
        );
    }
    Ok(())
}

fn emit_snapshot_detail(as_json: bool, detail: &SnapshotDetail) -> Result<()> {
    if as_json {
        return emit(true, detail, "");
    }

    println!("snapshot: {}", detail.snapshot.id);
    println!("source: {}", detail.source);
    println!("created: {}", detail.snapshot.created_at.to_rfc3339());
    println!("root tree: {}", detail.snapshot.root_tree_hash);
    println!(
        "parents: {}",
        if detail.snapshot.parent_snapshot_ids.is_empty() {
            "<none>".to_string()
        } else {
            detail.snapshot.parent_snapshot_ids.join(", ")
        }
    );
    println!(
        "labels: {}",
        if detail.snapshot.metadata.labels.is_empty() {
            "<none>".to_string()
        } else {
            detail.snapshot.metadata.labels.join(", ")
        }
    );
    println!(
        "changed files from parent: {}",
        detail
            .changed_file_count_from_parent
            .map(|count| count.to_string())
            .unwrap_or_else(|| "<n/a>".to_string())
    );
    Ok(())
}

fn emit_change_list(as_json: bool, changes: &[ChangeListEntry]) -> Result<()> {
    if as_json {
        return emit(true, &changes, "");
    }

    if changes.is_empty() {
        println!("no changes");
        return Ok(());
    }

    for change in changes {
        println!(
            "{}  {}  {:?}  task={}  risk={}  validation={}  promotion={}",
            change.node_id,
            change.title,
            change.status,
            change.task_title,
            change.risk_score,
            change
                .latest_validation_summary
                .clone()
                .unwrap_or_else(|| "<none>".to_string()),
            change
                .promotion_state
                .clone()
                .unwrap_or_else(|| "<none>".to_string())
        );
    }
    Ok(())
}

fn emit_change_detail(as_json: bool, detail: &ChangeDetail, include_logs: bool) -> Result<()> {
    if as_json {
        return emit(true, detail, "");
    }

    println!("change: {} ({})", detail.node.title, detail.node.id);
    println!("status: {:?}", detail.node.status);
    println!(
        "task: {}",
        detail
            .task
            .as_ref()
            .map(|task| format!("{} ({})", task.title, task.id))
            .unwrap_or_else(|| detail.node.task_id.clone())
    );
    println!("intent: {}", detail.node.intent);
    println!(
        "snapshots: base={} result={}",
        detail.node.base_snapshot_id, detail.node.result_snapshot_id
    );
    println!("risk: {}", detail.node.risk.score);
    println!("semantic: {}", detail.node.semantic_delta.summary);
    println!(
        "changed files: {}",
        if detail.node.semantic_delta.changed_files.is_empty() {
            "<none>".to_string()
        } else {
            detail.node.semantic_delta.changed_files.join(", ")
        }
    );
    let touched = detail
        .node
        .semantic_delta
        .touched_symbols
        .iter()
        .chain(detail.node.semantic_delta.added_symbols.iter())
        .chain(detail.node.semantic_delta.removed_symbols.iter())
        .map(|symbol| symbol.id.path.clone())
        .collect::<Vec<_>>();
    println!(
        "symbols: {}",
        if touched.is_empty() {
            "<none>".to_string()
        } else {
            touched.join(", ")
        }
    );
    println!("validations:");
    if detail.validations.is_empty() {
        println!("  <none>");
    } else {
        for validation in &detail.validations {
            emit_validation_record(validation, include_logs);
        }
    }
    println!("promotions:");
    if detail.promotions.is_empty() {
        println!("  <none>");
    } else {
        for promotion in &detail.promotions {
            println!(
                "  {} -> {} at {}",
                promotion.node_id,
                promotion.target_lineage,
                promotion.created_at.to_rfc3339()
            );
        }
    }
    Ok(())
}

fn emit_diff_summary(as_json: bool, diff: &DiffSummary) -> Result<()> {
    if as_json {
        return emit(true, diff, "");
    }

    println!(
        "diff: from={} to={} change={}",
        diff.from_snapshot_id.as_deref().unwrap_or("<none>"),
        diff.to_snapshot_id.as_deref().unwrap_or("<none>"),
        diff.change_node_id.as_deref().unwrap_or("<none>")
    );
    println!("counts: {}", join_counts(&diff.counts));
    if diff.ops.is_empty() {
        println!("no patch operations");
        return Ok(());
    }

    for op in &diff.ops {
        match op.kind {
            PatchOpKind::Rename => {
                let from = op.old_path.as_deref().unwrap_or("<unknown>");
                println!("RENAME  {} -> {}", from, op.path);
            }
            PatchOpKind::Add => println!("ADD     {}", op.path),
            PatchOpKind::Delete => println!("DELETE  {}", op.path),
            PatchOpKind::Modify => println!("MODIFY  {}", op.path),
        }
    }
    Ok(())
}

fn emit_validation_record(record: &ValidationRecord, include_logs: bool) {
    println!(
        "  {:?} {:?} {}",
        record.artifact.kind, record.artifact.status, record.artifact.summary
    );
    if include_logs {
        if let Some(details) = &record.details {
            println!("    command: {}", details.command);
            if !details.stdout.trim().is_empty() {
                println!("    stdout:");
                for line in details.stdout.lines() {
                    println!("      {line}");
                }
            }
            if !details.stderr.trim().is_empty() {
                println!("    stderr:");
                for line in details.stderr.lines() {
                    println!("      {line}");
                }
            }
        }
    }
}

fn join_counts(counts: &BTreeMap<String, usize>) -> String {
    if counts.is_empty() {
        "<none>".to_string()
    } else {
        counts
            .iter()
            .map(|(key, value)| format!("{key}={value}"))
            .collect::<Vec<_>>()
            .join(", ")
    }
}

fn emit<T: serde::Serialize>(as_json: bool, value: &T, message: &str) -> Result<()> {
    if as_json {
        println!("{}", serde_json::to_string_pretty(value)?);
    } else if !message.is_empty() {
        println!("{message}");
    }
    Ok(())
}
