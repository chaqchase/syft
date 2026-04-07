use std::collections::BTreeMap;

use anyhow::Result;
use syft_types::{
    DiffSummary, HistoryEntry, PatchOpKind, RepoStatusSummary, Snapshot, SnapshotDetail,
    SnapshotListEntry,
};

use super::emit;

pub fn emit_snapshot(as_json: bool, snapshot: &Snapshot) -> Result<()> {
    emit(
        as_json,
        snapshot,
        &format!(
            "snapshot {} created with root {}",
            snapshot.id, snapshot.root_tree_hash
        ),
    )
}

pub fn emit_status(as_json: bool, status: &RepoStatusSummary) -> Result<()> {
    if as_json {
        return emit(true, status, "");
    }

    println!("repo: {} ({})", status.repo_name, status.repo_id);
    println!(
        "current worktree: {}",
        status
            .current_worktree
            .as_ref()
            .map(|worktree| format!("{} [{}] {}", worktree.name, worktree.branch, worktree.path))
            .unwrap_or_else(|| "<none>".to_string())
    );
    println!(
        "head snapshot: {}",
        status.current_head_snapshot_id.as_deref().unwrap_or("<none>")
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

pub fn emit_history(as_json: bool, entries: &[HistoryEntry]) -> Result<()> {
    if as_json {
        return emit(true, &entries, "");
    }

    if entries.is_empty() {
        println!("no history entries");
        return Ok(());
    }

    for entry in entries {
        println!(
            "{}  {}  task={}  worktree={}  files={}  validation={}  promotion={}",
            entry.node_id,
            entry.title,
            entry.task_title,
            entry
                .worktree_name
                .clone()
                .unwrap_or_else(|| "<none>".to_string()),
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

pub fn emit_snapshot_list(as_json: bool, snapshots: &[SnapshotListEntry]) -> Result<()> {
    if as_json {
        return emit(true, &snapshots, "");
    }

    if snapshots.is_empty() {
        println!("no snapshots");
        return Ok(());
    }

    for snapshot in snapshots {
        println!(
            "{}  {}  worktree={}  parents={}  labels={}  {}",
            snapshot.id,
            snapshot.source,
            snapshot
                .worktree_name
                .clone()
                .unwrap_or_else(|| "<none>".to_string()),
            snapshot.parent_count,
            snapshot.label_summary,
            snapshot.created_at.to_rfc3339()
        );
    }
    Ok(())
}

pub fn emit_snapshot_detail(as_json: bool, detail: &SnapshotDetail) -> Result<()> {
    if as_json {
        return emit(true, detail, "");
    }

    println!("snapshot: {}", detail.snapshot.id);
    println!("source: {}", detail.source);
    println!(
        "worktree: {}",
        detail
            .worktree_name
            .clone()
            .unwrap_or_else(|| "<none>".to_string())
    );
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

pub fn emit_diff_summary(as_json: bool, diff: &DiffSummary) -> Result<()> {
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
