use std::collections::{BTreeMap, BTreeSet};
use std::env;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::Result;
use syft_types::{
    Author, ChangeNode, PatchOp, Provenance, RiskReport, SnapshotSource, ValidationArtifact,
    ValidationStatus,
};

pub(crate) fn symbol_names_for_change(change: &ChangeNode) -> Vec<String> {
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

pub(crate) fn provenance_summary(provenance: &Provenance) -> String {
    match &provenance.author {
        Author::Human { user_id } => format!("human:{user_id}"),
        Author::Agent { agent_id } => format!("agent:{agent_id}"),
        Author::Tool { tool_name } => format!("tool:{tool_name}"),
    }
}

pub(crate) fn snapshot_source_summary(source: &SnapshotSource) -> String {
    match source {
        SnapshotSource::ImportedFromGit { commit_sha } => {
            format!("git import {}", shorten_id(commit_sha))
        }
        SnapshotSource::MaterializedByHuman => "worktree capture".to_string(),
        SnapshotSource::MaterializedByAgent => "agent materialization".to_string(),
        SnapshotSource::MaterializedByCompose => "composed snapshot".to_string(),
    }
}

pub(crate) fn calculate_risk(
    semantic_delta: &syft_types::SemanticDelta,
    artifacts: &[ValidationArtifact],
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

pub(crate) fn default_provenance() -> Provenance {
    let user = current_username().unwrap_or_else(|| "unknown".to_string());
    Provenance {
        author: Author::Human { user_id: user },
        ..Provenance::default()
    }
}

pub fn current_username() -> Option<String> {
    env::var("USER")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .or_else(|| env::var("USERNAME").ok().filter(|value| !value.trim().is_empty()))
}

pub(crate) fn ensure_git_exclude(repo_path: &Path, entry: &str) -> Result<()> {
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

pub(crate) fn sync_syftignore_from_gitignore(repo_path: &Path) -> Result<()> {
    let gitignore_path = repo_path.join(".gitignore");
    let syftignore_path = repo_path.join(".syftignore");

    if !gitignore_path.exists() {
        if !syftignore_path.exists() {
            fs::write(syftignore_path, "")?;
        }
        return Ok(());
    }

    let gitignore = fs::read_to_string(&gitignore_path)?;
    if !syftignore_path.exists() {
        fs::write(syftignore_path, gitignore)?;
        return Ok(());
    }

    let existing = fs::read_to_string(&syftignore_path)?;
    let existing_lines = existing
        .lines()
        .map(|line| line.trim_end().to_string())
        .collect::<BTreeSet<_>>();

    let mut next = existing;
    if !next.ends_with('\n') && !next.is_empty() {
        next.push('\n');
    }

    for line in gitignore.lines() {
        let line = line.trim_end();
        if line.is_empty() || existing_lines.contains(line) {
            continue;
        }
        next.push_str(line);
        next.push('\n');
    }

    fs::write(syftignore_path, next)?;
    Ok(())
}

pub(crate) fn load_capture_rules(path: PathBuf) -> Result<Vec<String>> {
    if !path.exists() {
        return Ok(Vec::new());
    }

    Ok(fs::read_to_string(path)?
        .lines()
        .filter_map(parse_capture_rule)
        .collect())
}

fn shorten_id(value: &str) -> String {
    value.chars().take(8).collect()
}

fn parse_capture_rule(line: &str) -> Option<String> {
    let trimmed = line.trim();
    if trimmed.is_empty() || trimmed.starts_with('#') || trimmed.starts_with('!') {
        return None;
    }

    let trimmed = trimmed.trim_start_matches('/').trim_end_matches('/');
    if trimmed.is_empty()
        || trimmed.contains('*')
        || trimmed.contains('?')
        || trimmed.contains('[')
        || trimmed.contains(']')
    {
        return None;
    }

    Some(trimmed.to_string())
}

pub(crate) fn diff_summary(
    from_snapshot_id: Option<String>,
    to_snapshot_id: Option<String>,
    change_node_id: Option<String>,
    mut ops: Vec<PatchOp>,
) -> syft_types::DiffSummary {
    ops.sort_by(|left, right| left.path.cmp(&right.path));
    let mut counts = BTreeMap::new();
    for op in &ops {
        *counts.entry(format!("{:?}", op.kind)).or_insert(0) += 1;
    }
    syft_types::DiffSummary {
        from_snapshot_id,
        to_snapshot_id,
        change_node_id,
        ops,
        counts,
    }
}
