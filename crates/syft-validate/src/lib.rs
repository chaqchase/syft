use std::path::Path;
use std::process::Command;
use std::time::Instant;

use anyhow::{Context, Result};
use chrono::Utc;
use syft_git::materialize_snapshot_to;
use syft_objects::remove_excluded_paths;
use syft_store::ObjectStore;
use syft_types::{
    EntityId, ValidationArtifact, ValidationDetails, ValidationKind, ValidationMetrics,
    ValidationPlan, ValidationStatus, new_entity_id,
};
use tempfile::tempdir;

pub trait ValidationRunner: Send + Sync {
    fn validate(
        &self,
        repo_id: &str,
        snapshot_id: &str,
        node_id: Option<&str>,
        root_tree_hash: &str,
        object_store: &dyn ObjectStore,
        plan: &ValidationPlan,
        exclude_paths: &[String],
    ) -> Result<Vec<ValidationArtifact>>;
}

#[derive(Debug, Default, Clone)]
pub struct LocalValidationRunner;

impl ValidationRunner for LocalValidationRunner {
    fn validate(
        &self,
        repo_id: &str,
        snapshot_id: &str,
        node_id: Option<&str>,
        root_tree_hash: &str,
        object_store: &dyn ObjectStore,
        plan: &ValidationPlan,
        exclude_paths: &[String],
    ) -> Result<Vec<ValidationArtifact>> {
        let temp = tempdir()?;
        materialize_snapshot_to(root_tree_hash, temp.path(), object_store)?;
        remove_excluded_paths(temp.path(), exclude_paths)?;
        let target_dir = temp.path().join(".syft-build/target");

        let mut artifacts = Vec::new();
        if plan.run_typecheck {
            artifacts.push(run_validation_command(
                temp.path(),
                &target_dir,
                repo_id,
                snapshot_id,
                node_id,
                object_store,
                ValidationKind::Typecheck,
                &["check"],
            )?);
        }
        if plan.run_tests {
            artifacts.push(run_validation_command(
                temp.path(),
                &target_dir,
                repo_id,
                snapshot_id,
                node_id,
                object_store,
                ValidationKind::Tests,
                &["test"],
            )?);
        }
        if plan.run_lint {
            artifacts.push(run_validation_command(
                temp.path(),
                &target_dir,
                repo_id,
                snapshot_id,
                node_id,
                object_store,
                ValidationKind::Lint,
                &["clippy", "--", "-D", "warnings"],
            )?);
        }
        Ok(artifacts)
    }
}

fn run_validation_command(
    repo_path: &Path,
    target_dir: &Path,
    repo_id: &str,
    snapshot_id: &str,
    node_id: Option<&str>,
    object_store: &dyn ObjectStore,
    kind: ValidationKind,
    args: &[&str],
) -> Result<ValidationArtifact> {
    let started_at = Utc::now();
    let started = Instant::now();
    std::fs::create_dir_all(target_dir)?;
    let output = Command::new("cargo")
        .args(args)
        .current_dir(repo_path)
        .env("CARGO_TARGET_DIR", target_dir)
        .output()
        .with_context(|| format!("failed to run cargo {:?}", args))?;
    let completed_at = Utc::now();

    let duration_ms = started.elapsed().as_millis() as u64;
    let stderr = String::from_utf8_lossy(&output.stderr);
    let stdout = String::from_utf8_lossy(&output.stdout);
    let summary = if output.status.success() {
        format!("cargo {:?} passed", args)
    } else {
        let snippet = stderr
            .lines()
            .next()
            .unwrap_or_else(|| stdout.lines().next().unwrap_or("validation failed"));
        format!("cargo {:?} failed: {}", args, snippet)
    };
    let details = ValidationDetails {
        command: format!("cargo {}", args.join(" ")),
        exit_status: output.status.code().unwrap_or(-1),
        stdout: stdout.to_string(),
        stderr: stderr.to_string(),
    };
    let details_ref = object_store_hash(object_store, &details)?;

    Ok(ValidationArtifact {
        id: new_entity_id(),
        repo_id: repo_id.to_string(),
        snapshot_id: snapshot_id.to_string(),
        node_id: node_id.map(EntityId::from),
        kind,
        status: if output.status.success() {
            ValidationStatus::Passed
        } else {
            ValidationStatus::Failed
        },
        summary,
        details_ref: Some(details_ref),
        metrics: ValidationMetrics {
            duration_ms,
            passed_count: None,
            failed_count: None,
            skipped_count: None,
            coverage_delta: None,
            benchmark_delta_pct: None,
        },
        started_at,
        completed_at,
    })
}

fn object_store_hash(
    object_store: &dyn ObjectStore,
    details: &ValidationDetails,
) -> Result<String> {
    let bytes = serde_json::to_vec(details)?;
    object_store.put_bytes(&bytes)
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::Path;

    use tempfile::tempdir;

    use super::*;
    use syft_objects::{capture_directory, capture_paths, effective_capture_excludes};
    use syft_store::FsObjectStore;
    use syft_types::{
        Snapshot, SnapshotMetadata, SnapshotSource, ValidationPlan, ValidationStatus,
        new_entity_id, now_utc,
    };

    fn fixture_snapshot(
        root_hash: String,
        source: SnapshotSource,
    ) -> Snapshot {
        Snapshot {
            id: new_entity_id(),
            parent_snapshot_ids: Vec::new(),
            root_tree_hash: root_hash,
            created_at: now_utc(),
            metadata: SnapshotMetadata {
                repo_id: "repo".to_string(),
                source,
                labels: Vec::new(),
                worktree_id: None,
            },
        }
    }

    fn write_fixture_project(project: &Path) {
        fs::create_dir_all(project.join("src")).unwrap();
        fs::write(
            project.join("Cargo.toml"),
            "[package]\nname = \"fixture\"\nversion = \"0.1.0\"\nedition = \"2024\"\n",
        )
        .unwrap();
        fs::write(
            project.join("src/lib.rs"),
            "pub fn greet() -> &'static str { \"hello\" }\n",
        )
        .unwrap();
        fs::create_dir_all(project.join("tests")).unwrap();
        fs::write(
            project.join("tests/smoke.rs"),
            "use fixture::greet;\n#[test]\nfn smoke() { assert_eq!(greet(), \"hello\"); }\n",
        )
        .unwrap();
    }

    #[test]
    fn validation_runner_persists_details_for_success_and_failure() {
        let project = tempdir().unwrap();
        write_fixture_project(project.path());

        let object_root = tempdir().unwrap();
        let object_store = FsObjectStore::new(object_root.path());
        let (root_hash, _) = capture_directory(project.path(), &object_store, &Vec::new()).unwrap();
        let snapshot = fixture_snapshot(root_hash, SnapshotSource::MaterializedByHuman);
        let runner = LocalValidationRunner;
        let artifacts = runner
            .validate(
                "repo",
                &snapshot.id,
                Some("node"),
                &snapshot.root_tree_hash,
                &object_store,
                &ValidationPlan {
                    run_tests: true,
                    run_lint: false,
                    run_typecheck: false,
                },
                &effective_capture_excludes(&[]),
            )
            .unwrap();
        assert!(artifacts[0].details_ref.is_some());

        fs::write(
            project.path().join("tests/smoke.rs"),
            "use fixture::greet;\n#[test]\nfn smoke() { assert_eq!(greet(), \"goodbye\"); }\n",
        )
        .unwrap();
        let (failing_hash, _) =
            capture_directory(project.path(), &object_store, &Vec::new()).unwrap();
        let failing_snapshot = fixture_snapshot(failing_hash, SnapshotSource::MaterializedByHuman);
        let failing_artifacts = runner
            .validate(
                "repo",
                &failing_snapshot.id,
                Some("node"),
                &failing_snapshot.root_tree_hash,
                &object_store,
                &ValidationPlan {
                    run_tests: true,
                    run_lint: false,
                    run_typecheck: false,
                },
                &effective_capture_excludes(&[]),
            )
            .unwrap();
        let hash = failing_artifacts[0].details_ref.clone().unwrap();
        let raw = object_store.get_bytes(&hash).unwrap().unwrap();
        let details: ValidationDetails = serde_json::from_slice(&raw).unwrap();
        assert_eq!(details.command, "cargo test");
        assert_ne!(details.exit_status, 0);
        assert!(details.stderr.contains("test") || details.stdout.contains("test"));
    }

    #[test]
    fn validation_ignores_polluted_target_artifacts() {
        let project = tempdir().unwrap();
        write_fixture_project(project.path());

        let runner = LocalValidationRunner;
        let object_root = tempdir().unwrap();
        let object_store = FsObjectStore::new(object_root.path());
        fs::create_dir_all(project.path().join("target/debug")).unwrap();
        fs::write(
            project.path().join("target/.rustc_info.json"),
            "{\"stale\":true}\n",
        )
        .unwrap();
        fs::write(
            project.path().join("target/debug/.cargo-lock"),
            "stale build output\n",
        )
        .unwrap();

        fs::write(
            project.path().join("src/lib.rs"),
            "pub fn greet() -> &'static str { \"hello from syft\" }\n",
        )
        .unwrap();

        let polluted_paths = vec![
            std::path::PathBuf::from("Cargo.toml"),
            std::path::PathBuf::from("src/lib.rs"),
            std::path::PathBuf::from("tests/smoke.rs"),
            std::path::PathBuf::from("target/.rustc_info.json"),
            std::path::PathBuf::from("target/debug/.cargo-lock"),
        ];
        let (polluted_hash, _) = capture_paths(project.path(), &polluted_paths, &object_store).unwrap();
        let polluted_snapshot = fixture_snapshot(polluted_hash, SnapshotSource::MaterializedByHuman);

        let artifacts = runner
            .validate(
                "repo",
                &polluted_snapshot.id,
                Some("node"),
                &polluted_snapshot.root_tree_hash,
                &object_store,
                &ValidationPlan {
                    run_tests: true,
                    run_lint: false,
                    run_typecheck: false,
                },
                &effective_capture_excludes(&[]),
            )
            .unwrap();

        assert!(matches!(artifacts[0].status, ValidationStatus::Failed));
        let details_hash = artifacts[0].details_ref.clone().unwrap();
        let raw = object_store.get_bytes(&details_hash).unwrap().unwrap();
        let details: ValidationDetails = serde_json::from_slice(&raw).unwrap();
        assert_ne!(details.exit_status, 0);
        assert!(details.stdout.contains("FAILED") || details.stderr.contains("FAILED"));
    }
}
