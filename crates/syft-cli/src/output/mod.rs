mod change;
mod repo;
mod task;
mod worktree;

pub use change::*;
pub use repo::*;
pub use task::*;
pub use worktree::*;

pub(crate) fn emit<T: serde::Serialize>(
    as_json: bool,
    value: &T,
    message: &str,
) -> anyhow::Result<()> {
    if as_json {
        println!("{}", serde_json::to_string_pretty(value)?);
    } else if !message.is_empty() {
        println!("{message}");
    }
    Ok(())
}
