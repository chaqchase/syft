use std::env;

use anyhow::{Result, bail};
use clap::Parser;
use syft_core::{
    ChangeService, CreateTaskInput, PromoteChangeInput, ProposeChangeInput, QueryService,
    RepoService, SyftApp, TaskService, current_username,
};
use syft_types::{HistoryQuery, TaskPriority, ValidationPlan};

mod cli;
mod output;

use cli::{
    ChangeCommands, Cli, Commands, RepoCommands, SnapshotCommands, TaskCommands,
};
use output::{
    emit_change, emit_change_detail, emit_change_list, emit_current_task_set, emit_diff_summary,
    emit_history, emit_optional_task, emit_promotion, emit_snapshot, emit_snapshot_detail,
    emit_snapshot_list, emit_status, emit_task, emit_task_detail, emit_tasks,
    emit_validated_change,
};

pub fn run() -> Result<()> {
    let cli = Cli::parse();
    let cwd = env::current_dir()?;

    match cli.command {
        Commands::Init(args) => {
            let app = SyftApp::init_repo(&cwd, args.name, args.sync_gitignore)?;
            output::emit(
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
                    emit_tasks(cli.json, &app.list_tasks()?)?;
                }
                TaskCommands::Show { task_id } => {
                    emit_task_detail(cli.json, &app.show_task(&task_id)?)?;
                }
                TaskCommands::Current => {
                    emit_optional_task(cli.json, app.get_current_task()?.as_ref())?;
                }
                TaskCommands::SetCurrent { task_id } => {
                    emit_current_task_set(cli.json, &app.set_current_task(&task_id)?)?;
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
                        .or_else(current_username)
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
