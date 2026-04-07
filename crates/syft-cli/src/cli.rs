use clap::{Args, Parser, Subcommand};

#[derive(Parser, Debug)]
#[command(name = "syft")]
#[command(about = "AI-native version control bootstrap CLI")]
pub struct Cli {
    #[arg(long, global = true)]
    pub json: bool,
    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Subcommand, Debug)]
pub enum Commands {
    Init(InitArgs),
    Status,
    History(HistoryArgs),
    Repo(RepoArgs),
    Snapshot(SnapshotArgs),
    Task(TaskArgs),
    Change(ChangeArgs),
}

#[derive(Args, Debug)]
pub struct InitArgs {
    #[arg(long)]
    pub name: Option<String>,
    #[arg(long)]
    pub sync_gitignore: bool,
}

#[derive(Args, Debug)]
pub struct HistoryArgs {
    #[arg(long)]
    pub task: Option<String>,
    #[arg(long)]
    pub symbol: Option<String>,
    #[arg(long, default_value_t = 20)]
    pub limit: usize,
}

#[derive(Args, Debug)]
pub struct RepoArgs {
    #[command(subcommand)]
    pub command: RepoCommands,
}

#[derive(Subcommand, Debug)]
pub enum RepoCommands {
    ImportGit {
        #[arg(long, default_value = "HEAD")]
        commit: String,
    },
}

#[derive(Args, Debug)]
pub struct SnapshotArgs {
    #[command(subcommand)]
    pub command: SnapshotCommands,
}

#[derive(Subcommand, Debug)]
pub enum SnapshotCommands {
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
pub struct TaskArgs {
    #[command(subcommand)]
    pub command: TaskCommands,
}

#[derive(Subcommand, Debug)]
pub enum TaskCommands {
    Create(TaskCreateArgs),
    List,
    Show { task_id: String },
    Current,
    SetCurrent { task_id: String },
    Changes { task_id: String },
}

#[derive(Args, Debug)]
pub struct TaskCreateArgs {
    #[arg(long)]
    pub title: String,
    #[arg(long, default_value = "")]
    pub description: String,
    #[arg(long = "acceptance")]
    pub acceptance_criteria: Vec<String>,
    #[arg(long = "constraint")]
    pub constraints: Vec<String>,
    #[arg(long = "label")]
    pub labels: Vec<String>,
    #[arg(long, default_value = "medium")]
    pub priority: String,
}

#[derive(Args, Debug)]
pub struct ChangeArgs {
    #[command(subcommand)]
    pub command: ChangeCommands,
}

#[derive(Subcommand, Debug)]
pub enum ChangeCommands {
    Propose(ChangeProposeArgs),
    Validate(ChangeValidateArgs),
    Promote(ChangePromoteArgs),
    List,
    Show(ChangeShowArgs),
    Diff { node_id: String },
    Latest(ChangeLatestArgs),
}

#[derive(Args, Debug)]
pub struct ChangeProposeArgs {
    #[arg(long = "task")]
    pub task_id: Option<String>,
    #[arg(long)]
    pub title: String,
    #[arg(long)]
    pub intent: String,
    #[arg(long)]
    pub base: Option<String>,
    #[arg(long)]
    pub result: String,
    #[arg(long)]
    pub rationale: Option<String>,
    #[arg(long = "tag")]
    pub tags: Vec<String>,
}

#[derive(Args, Debug)]
pub struct ChangeValidateArgs {
    pub node_id: String,
    #[arg(long)]
    pub tests: bool,
    #[arg(long)]
    pub lint: bool,
    #[arg(long)]
    pub typecheck: bool,
}

#[derive(Args, Debug)]
pub struct ChangePromoteArgs {
    pub node_id: String,
    #[arg(long = "to")]
    pub target_lineage: String,
    #[arg(long)]
    pub approved_by: Option<String>,
    #[arg(long)]
    pub notes: Option<String>,
    #[arg(long)]
    pub no_export: bool,
}

#[derive(Args, Debug)]
pub struct ChangeShowArgs {
    pub node_id: String,
    #[arg(long)]
    pub logs: bool,
}

#[derive(Args, Debug)]
pub struct ChangeLatestArgs {
    #[arg(long)]
    pub task: Option<String>,
}
