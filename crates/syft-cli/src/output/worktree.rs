use anyhow::Result;
use syft_types::{ManagedWorktree, WorktreeDetail};

use super::emit;

pub fn emit_worktree(as_json: bool, worktree: &ManagedWorktree, action: &str) -> Result<()> {
    if as_json {
        emit(true, worktree, "")
    } else {
        println!(
            "{} worktree: {} ({}) [{}] {}",
            action, worktree.name, worktree.id, worktree.branch, worktree.path
        );
        Ok(())
    }
}

pub fn emit_worktree_list(as_json: bool, worktrees: &[ManagedWorktree]) -> Result<()> {
    if as_json {
        return emit(true, &worktrees, "");
    }

    if worktrees.is_empty() {
        println!("no worktrees");
        return Ok(());
    }

    for worktree in worktrees {
        println!(
            "{}  task={}  {:?}  {}  {}",
            worktree.name, worktree.task_id, worktree.status, worktree.branch, worktree.path
        );
    }
    Ok(())
}

pub fn emit_worktree_detail(as_json: bool, detail: &WorktreeDetail) -> Result<()> {
    if as_json {
        return emit(true, detail, "");
    }

    let worktree = &detail.worktree;
    println!("worktree: {} ({})", worktree.name, worktree.id);
    println!("task: {}", worktree.task_id);
    println!("status: {:?}", worktree.status);
    println!("branch: {}", worktree.branch);
    println!("source ref: {}", worktree.source_ref);
    println!("path: {}", worktree.path);
    println!("linked changes: {}", detail.linked_change_count);
    println!("created: {}", worktree.created_at.to_rfc3339());
    println!("updated: {}", worktree.updated_at.to_rfc3339());
    Ok(())
}

pub fn emit_optional_worktree(as_json: bool, worktree: Option<&ManagedWorktree>) -> Result<()> {
    match worktree {
        Some(worktree) => emit_worktree(as_json, worktree, "current"),
        None if as_json => {
            println!("null");
            Ok(())
        }
        None => {
            println!("no current worktree");
            Ok(())
        }
    }
}
