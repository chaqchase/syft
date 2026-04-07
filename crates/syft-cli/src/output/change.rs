use anyhow::Result;
use syft_types::{
    ChangeDetail, ChangeListEntry, ChangeNode, PromotionRecord, ValidationPlan, ValidationRecord,
};

use super::emit;

pub fn emit_change(as_json: bool, node: &ChangeNode) -> Result<()> {
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

pub fn emit_validated_change(as_json: bool, node: &ChangeNode, plan: &ValidationPlan) -> Result<()> {
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

pub fn emit_promotion(as_json: bool, record: &PromotionRecord) -> Result<()> {
    emit(
        as_json,
        record,
        &format!(
            "promoted change {} to {}",
            record.node_id, record.target_lineage
        ),
    )
}

pub fn emit_change_list(as_json: bool, changes: &[ChangeListEntry]) -> Result<()> {
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

pub fn emit_change_detail(as_json: bool, detail: &ChangeDetail, include_logs: bool) -> Result<()> {
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

fn validation_outcome(node: &ChangeNode) -> &'static str {
    match node.status {
        syft_types::ChangeNodeStatus::Validated | syft_types::ChangeNodeStatus::Approved => {
            "passed"
        }
        syft_types::ChangeNodeStatus::Rejected => "failed",
        _ => "pending",
    }
}
