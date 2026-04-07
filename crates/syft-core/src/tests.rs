use std::fs;
use std::path::Path;
use std::process::Command;

use anyhow::{Result, bail};
use tempfile::tempdir;

use crate::{QueryService, RepoService, SyftApp};

#[test]
fn init_repo_writes_control_dir() {
    let dir = tempdir().unwrap();
    setup_repo(dir.path());

    let app = SyftApp::init_repo(dir.path(), Some("fixture".to_string()), false).unwrap();
    let snapshot = app.import_git_commit("HEAD").unwrap();

    assert!(dir.path().join(".syft/repo.toml").exists());
    assert!(!snapshot.id.is_empty());
}

#[test]
fn status_reports_missing_head_snapshot() {
    let dir = tempdir().unwrap();
    setup_repo(dir.path());

    let app = SyftApp::init_repo(dir.path(), Some("fixture".to_string()), false).unwrap();
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

    let app = SyftApp::init_repo(dir.path(), Some("fixture".to_string()), false).unwrap();
    let snapshot = app.import_git_commit("HEAD").unwrap();
    let status = app.status().unwrap();

    assert_eq!(
        status.current_head_snapshot_id.as_deref(),
        Some(snapshot.id.as_str())
    );
    assert!(status.change_counts.is_empty());
    assert!(status.attention_needed.is_empty());
}

#[test]
fn init_repo_can_seed_syftignore_from_gitignore() {
    let dir = tempdir().unwrap();
    setup_repo(dir.path());
    fs::write(dir.path().join(".gitignore"), "target/\n.DS_Store\n").unwrap();

    SyftApp::init_repo(dir.path(), Some("fixture".to_string()), true).unwrap();

    let syftignore = fs::read_to_string(dir.path().join(".syftignore")).unwrap();
    assert!(syftignore.contains("target/"));
    assert!(syftignore.contains(".DS_Store"));
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
