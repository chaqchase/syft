use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result, anyhow, bail};
use syft_objects::{capture_paths, capture_virtual_entries, materialize_snapshot};
use syft_store::ObjectStore;
use syft_types::{
    FileMode, RepoConfig, Snapshot, SnapshotIndex, SnapshotMetadata, SnapshotSource, new_entity_id,
    now_utc,
};
use tempfile::tempdir;

pub fn ensure_git_repo(repo_path: &Path) -> Result<()> {
    run_git(repo_path, &["rev-parse", "--show-toplevel"])?;
    Ok(())
}

pub fn current_commit(repo_path: &Path) -> Result<String> {
    Ok(run_git(repo_path, &["rev-parse", "HEAD"])?
        .trim()
        .to_string())
}

pub fn worktree_file_paths(repo_path: &Path) -> Result<Vec<PathBuf>> {
    let bytes = run_git_bytes(
        repo_path,
        &[
            "ls-files",
            "-z",
            "--cached",
            "--others",
            "--exclude-standard",
        ],
    )?;
    Ok(split_null_terminated(&bytes)
        .into_iter()
        .map(PathBuf::from)
        .collect())
}

pub fn import_git_commit(
    repo_path: &Path,
    repo_config: &RepoConfig,
    commit: &str,
    object_store: &dyn ObjectStore,
    parent_snapshot_ids: Vec<String>,
) -> Result<(Snapshot, SnapshotIndex)> {
    let entries = git_commit_entries(repo_path, commit)?;
    let (root_tree_hash, index) = capture_virtual_entries(&entries, object_store)?;
    let snapshot = Snapshot {
        id: new_entity_id(),
        parent_snapshot_ids,
        root_tree_hash,
        created_at: now_utc(),
        metadata: SnapshotMetadata {
            repo_id: repo_config.repo_id.clone(),
            source: SnapshotSource::ImportedFromGit {
                commit_sha: commit.to_string(),
            },
            labels: vec!["git-import".to_string()],
        },
    };
    Ok((snapshot, index))
}

pub fn capture_worktree_snapshot(
    repo_path: &Path,
    repo_config: &RepoConfig,
    object_store: &dyn ObjectStore,
    parent_snapshot_ids: Vec<String>,
) -> Result<(Snapshot, SnapshotIndex)> {
    let paths = worktree_file_paths(repo_path)?;
    let (root_tree_hash, index) = capture_paths(repo_path, &paths, object_store)?;
    let snapshot = Snapshot {
        id: new_entity_id(),
        parent_snapshot_ids,
        root_tree_hash,
        created_at: now_utc(),
        metadata: SnapshotMetadata {
            repo_id: repo_config.repo_id.clone(),
            source: SnapshotSource::MaterializedByHuman,
            labels: vec!["worktree".to_string()],
        },
    };
    Ok((snapshot, index))
}

pub fn materialize_snapshot_to(
    root_tree_hash: &str,
    destination: &Path,
    object_store: &dyn ObjectStore,
) -> Result<()> {
    materialize_snapshot(root_tree_hash, destination, object_store)
}

pub fn export_snapshot_to_git_commit(
    repo_path: &Path,
    root_tree_hash: &str,
    object_store: &dyn ObjectStore,
    message: &str,
) -> Result<String> {
    let materialized = tempdir()?;
    materialize_snapshot(root_tree_hash, materialized.path(), object_store)?;
    sync_worktree(materialized.path(), repo_path)?;

    run_git(repo_path, &["add", "-A"])?;
    run_git(repo_path, &["commit", "--allow-empty", "-m", message])?;
    current_commit(repo_path)
}

fn git_commit_entries(repo_path: &Path, commit: &str) -> Result<Vec<(PathBuf, FileMode, Vec<u8>)>> {
    let output = run_git_bytes(repo_path, &["ls-tree", "-r", "-z", commit])?;
    let mut entries = Vec::new();

    for row in split_null_terminated(&output) {
        if row.trim().is_empty() {
            continue;
        }
        let (header, path) = row
            .split_once('\t')
            .ok_or_else(|| anyhow!("unexpected git ls-tree row: {row}"))?;
        let parts: Vec<&str> = header.split_whitespace().collect();
        if parts.len() < 3 {
            bail!("unexpected git ls-tree header: {header}");
        }
        let mode = match parts[0] {
            "100755" => FileMode::Executable,
            _ => FileMode::File,
        };
        let spec = format!("{commit}:{path}");
        let bytes = run_git_bytes(repo_path, &["show", &spec])?;
        entries.push((PathBuf::from(path), mode, bytes));
    }

    Ok(entries)
}

fn split_null_terminated(bytes: &[u8]) -> Vec<String> {
    bytes
        .split(|byte| *byte == b'\0')
        .filter(|segment| !segment.is_empty())
        .map(|segment| String::from_utf8_lossy(segment).to_string())
        .collect()
}

fn sync_worktree(source: &Path, destination: &Path) -> Result<()> {
    for entry in fs::read_dir(destination)? {
        let entry = entry?;
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().to_string();
        if name == ".git" || name == ".syft" {
            continue;
        }
        remove_path(&path)?;
    }

    copy_directory(source, destination)
}

fn copy_directory(source: &Path, destination: &Path) -> Result<()> {
    for entry in fs::read_dir(source)? {
        let entry = entry?;
        let src_path = entry.path();
        let dst_path = destination.join(entry.file_name());
        if src_path.is_dir() {
            fs::create_dir_all(&dst_path)?;
            copy_directory(&src_path, &dst_path)?;
        } else {
            if let Some(parent) = dst_path.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::copy(&src_path, &dst_path)?;
        }
    }
    Ok(())
}

fn remove_path(path: &Path) -> Result<()> {
    if path.is_dir() {
        fs::remove_dir_all(path)?;
    } else if path.exists() {
        fs::remove_file(path)?;
    }
    Ok(())
}

fn run_git(repo_path: &Path, args: &[&str]) -> Result<String> {
    let output = run_git_bytes(repo_path, args)?;
    String::from_utf8(output).context("git output was not valid utf-8")
}

fn run_git_bytes(repo_path: &Path, args: &[&str]) -> Result<Vec<u8>> {
    let output = Command::new("git")
        .args(args)
        .current_dir(repo_path)
        .output()
        .with_context(|| format!("failed to run git {:?}", args))?;
    if !output.status.success() {
        return Err(anyhow!(
            "git {:?} failed: {}",
            args,
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    Ok(output.stdout)
}
