use std::fs;
use std::path::Path;
use std::process::Command;

use serde_json::Value;
use tempfile::tempdir;

#[test]
fn read_commands_and_history_work_end_to_end() {
    let repo = setup_fixture_repo();

    let _config = syft_json(repo.path(), &["init", "--name", "fixture"]);
    let base_snapshot = syft_json(repo.path(), &["repo", "import-git", "--commit", "HEAD"]);
    let no_current_task = syft_json(repo.path(), &["task", "current"]);
    assert!(no_current_task.is_null());
    let task = syft_json(
        repo.path(),
        &[
            "task",
            "create",
            "--title",
            "Add bootstrap change",
            "--description",
            "exercise the full workflow",
            "--acceptance",
            "tests stay green",
        ],
    );
    let shown_task = syft_json(repo.path(), &["task", "show", task["id"].as_str().unwrap()]);
    assert_eq!(shown_task["id"], task["id"]);
    let set_current = syft_json(
        repo.path(),
        &["task", "set-current", task["id"].as_str().unwrap()],
    );
    assert_eq!(set_current["id"], task["id"]);
    let current_task = syft_json(repo.path(), &["task", "current"]);
    assert_eq!(current_task["id"], task["id"]);

    fs::write(
        repo.path().join("src/lib.rs"),
        "pub fn greet() -> &'static str {\n    \"hello, syft\"\n}\n",
    )
    .unwrap();
    fs::write(
        repo.path().join("tests/smoke.rs"),
        "use fixture::greet;\n\n#[test]\nfn smoke() {\n    assert_eq!(greet(), \"hello, syft\");\n}\n",
    )
    .unwrap();

    let result_snapshot = syft_json(repo.path(), &["snapshot", "capture"]);
    let change = syft_json(
        repo.path(),
        &[
            "change",
            "propose",
            "--title",
            "Update greeting",
            "--intent",
            "refresh the library output",
            "--result",
            result_snapshot["id"].as_str().unwrap(),
        ],
    );
    let validated = syft_json(
        repo.path(),
        &[
            "change",
            "validate",
            change["id"].as_str().unwrap(),
            "--tests",
            "--typecheck",
        ],
    );
    assert_eq!(validated["status"], "Validated");

    let promotion = syft_json(
        repo.path(),
        &[
            "change",
            "promote",
            change["id"].as_str().unwrap(),
            "--to",
            "main",
        ],
    );
    assert_eq!(promotion["node_id"], change["id"]);

    let status = syft_json(repo.path(), &["status"]);
    assert_eq!(status["repo_name"], "fixture");
    assert_eq!(status["current_head_snapshot_id"], result_snapshot["id"]);

    let history = syft_json(repo.path(), &["history"]);
    assert_eq!(history.as_array().unwrap().len(), 1);
    assert_eq!(history[0]["node_id"], change["id"]);

    let history_by_task = syft_json(
        repo.path(),
        &["history", "--task", task["id"].as_str().unwrap()],
    );
    assert_eq!(history_by_task.as_array().unwrap().len(), 1);

    let history_by_symbol = syft_json(repo.path(), &["history", "--symbol", "greet"]);
    assert_eq!(history_by_symbol.as_array().unwrap().len(), 1);

    let task_changes = syft_json(
        repo.path(),
        &["task", "changes", task["id"].as_str().unwrap()],
    );
    assert_eq!(task_changes.as_array().unwrap().len(), 1);
    assert_eq!(task_changes[0]["node_id"], change["id"]);

    let snapshots = syft_json(repo.path(), &["snapshot", "list"]);
    assert_eq!(snapshots.as_array().unwrap().len(), 2);

    let snapshot_detail = syft_json(
        repo.path(),
        &["snapshot", "show", result_snapshot["id"].as_str().unwrap()],
    );
    assert_eq!(snapshot_detail["snapshot"]["id"], result_snapshot["id"]);
    assert!(
        snapshot_detail["changed_file_count_from_parent"]
            .as_u64()
            .unwrap()
            >= 1
    );
    let snapshot_diff = syft_json(
        repo.path(),
        &[
            "snapshot",
            "diff",
            base_snapshot["id"].as_str().unwrap(),
            result_snapshot["id"].as_str().unwrap(),
        ],
    );
    assert!(snapshot_diff["ops"].as_array().unwrap().len() >= 1);

    let changes = syft_json(repo.path(), &["change", "list"]);
    assert_eq!(changes.as_array().unwrap().len(), 1);
    assert_eq!(changes[0]["node_id"], change["id"]);
    let change_diff = syft_json(
        repo.path(),
        &["change", "diff", change["id"].as_str().unwrap()],
    );
    assert_eq!(change_diff["change_node_id"], change["id"]);
    assert!(change_diff["ops"].as_array().unwrap().len() >= 1);

    let change_detail = syft_json(
        repo.path(),
        &["change", "show", change["id"].as_str().unwrap()],
    );
    assert_eq!(change_detail["node"]["id"], change["id"]);
    assert_eq!(change_detail["validations"].as_array().unwrap().len(), 2);
    assert_eq!(change_detail["promotions"].as_array().unwrap().len(), 1);
    let latest_change = syft_json(repo.path(), &["change", "latest"]);
    assert_eq!(latest_change["node"]["id"], change["id"]);

    let status_text = syft_text(repo.path(), &["status"]);
    assert!(status_text.contains("repo: fixture"));
    let change_text = syft_text(
        repo.path(),
        &["change", "show", change["id"].as_str().unwrap()],
    );
    assert!(change_text.contains("semantic:"));
    assert!(change_text.contains("promotions:"));
    let diff_text = syft_text(
        repo.path(),
        &["change", "diff", change["id"].as_str().unwrap()],
    );
    assert!(diff_text.contains("counts:"));
    assert!(diff_text.contains("ADD") || diff_text.contains("MODIFY"));

    let second_task = syft_json(
        repo.path(),
        &[
            "task",
            "create",
            "--title",
            "Unused task",
            "--description",
            "used to test latest override",
        ],
    );
    syft_json(
        repo.path(),
        &["task", "set-current", second_task["id"].as_str().unwrap()],
    );
    let latest_err = syft_fail(repo.path(), &["change", "latest"]);
    assert!(latest_err.contains("no changes found for task"));
    let latest_override = syft_json(
        repo.path(),
        &["change", "latest", "--task", task["id"].as_str().unwrap()],
    );
    assert_eq!(latest_override["node"]["id"], change["id"]);

    let message = run_capture(repo.path(), "git", &["log", "-1", "--pretty=%B"]);
    assert!(message.contains("syft promote: Update greeting -> main"));
}

#[test]
fn failing_validation_logs_are_exposed_in_change_show() {
    let repo = setup_fixture_repo();

    syft_json(repo.path(), &["init", "--name", "fixture"]);
    let base_snapshot = syft_json(repo.path(), &["repo", "import-git", "--commit", "HEAD"]);
    let task = syft_json(
        repo.path(),
        &[
            "task",
            "create",
            "--title",
            "Introduce a failing test",
            "--acceptance",
            "failure is visible",
        ],
    );

    fs::write(
        repo.path().join("tests/smoke.rs"),
        "use fixture::greet;\n\n#[test]\nfn smoke() {\n    assert_eq!(greet(), \"not hello\");\n}\n",
    )
    .unwrap();

    let result_snapshot = syft_json(repo.path(), &["snapshot", "capture"]);
    let change = syft_json(
        repo.path(),
        &[
            "change",
            "propose",
            "--task",
            task["id"].as_str().unwrap(),
            "--title",
            "Break the test",
            "--intent",
            "exercise failing validations",
            "--base",
            base_snapshot["id"].as_str().unwrap(),
            "--result",
            result_snapshot["id"].as_str().unwrap(),
        ],
    );

    let validated = syft_json(
        repo.path(),
        &[
            "change",
            "validate",
            change["id"].as_str().unwrap(),
            "--tests",
        ],
    );
    assert_eq!(validated["status"], "Rejected");

    let status = syft_json(repo.path(), &["status"]);
    assert!(
        status["attention_needed"]
            .as_array()
            .unwrap()
            .iter()
            .any(|item| item.as_str().unwrap().contains("failing validations"))
    );

    let detail = syft_json(
        repo.path(),
        &["change", "show", change["id"].as_str().unwrap(), "--logs"],
    );
    let validation = &detail["validations"][0];
    assert!(validation["artifact"]["details_ref"].is_string());
    assert_eq!(validation["details"]["command"], "cargo test");
    assert_ne!(validation["details"]["exit_status"], 0);
    assert!(
        validation["details"]["stdout"]
            .as_str()
            .unwrap()
            .contains("test")
            || validation["details"]["stderr"]
                .as_str()
                .unwrap()
                .contains("test")
    );

    let text = syft_text(
        repo.path(),
        &["change", "show", change["id"].as_str().unwrap(), "--logs"],
    );
    assert!(text.contains("stdout:") || text.contains("stderr:"));

    let history = syft_json(repo.path(), &["history"]);
    assert_eq!(history[0]["validation_status"], "Failed");
}

#[test]
fn missing_context_and_stale_current_task_fail_clearly() {
    let repo = setup_fixture_repo();
    syft_json(repo.path(), &["init", "--name", "fixture"]);

    let task = syft_json(
        repo.path(),
        &[
            "task",
            "create",
            "--title",
            "Needs context",
            "--acceptance",
            "must error usefully",
        ],
    );
    syft_json(
        repo.path(),
        &["task", "set-current", task["id"].as_str().unwrap()],
    );
    let result_snapshot = syft_json(repo.path(), &["snapshot", "capture"]);
    let no_head_error = syft_fail(
        repo.path(),
        &[
            "change",
            "propose",
            "--title",
            "Missing head",
            "--intent",
            "prove base default needs head",
            "--result",
            result_snapshot["id"].as_str().unwrap(),
        ],
    );
    assert!(no_head_error.contains("no base snapshot specified"));

    let repo_two = setup_fixture_repo();
    syft_json(repo_two.path(), &["init", "--name", "fixture"]);
    let base_snapshot = syft_json(repo_two.path(), &["repo", "import-git", "--commit", "HEAD"]);
    fs::write(
        repo_two.path().join("src/lib.rs"),
        "pub fn greet() -> &'static str {\n    \"changed\"\n}\n",
    )
    .unwrap();
    let result_snapshot = syft_json(repo_two.path(), &["snapshot", "capture"]);
    let no_task_error = syft_fail(
        repo_two.path(),
        &[
            "change",
            "propose",
            "--title",
            "Missing task",
            "--intent",
            "prove task default needs current task",
            "--result",
            result_snapshot["id"].as_str().unwrap(),
            "--base",
            base_snapshot["id"].as_str().unwrap(),
        ],
    );
    assert!(no_task_error.contains("no task specified"));

    fs::write(
        repo_two.path().join(".syft/state/current_task"),
        "01INVALIDTASKID\n",
    )
    .unwrap();
    let stale_error = syft_fail(repo_two.path(), &["task", "current"]);
    assert!(stale_error.contains("current task"));
    assert!(stale_error.contains("set-current"));
}

#[test]
fn snapshot_capture_excludes_target_and_validation_rejects_source_only_failure() {
    let repo = setup_fixture_repo();

    syft_json(repo.path(), &["init", "--name", "fixture"]);
    let _base_snapshot = syft_json(repo.path(), &["repo", "import-git", "--commit", "HEAD"]);
    let task = syft_json(
        repo.path(),
        &[
            "task",
            "create",
            "--title",
            "Source-only regression",
            "--acceptance",
            "target output stays out of snapshots",
        ],
    );
    syft_json(
        repo.path(),
        &["task", "set-current", task["id"].as_str().unwrap()],
    );

    run(repo.path(), "cargo", &["test"]);
    assert!(repo.path().join("target").exists());

    fs::write(
        repo.path().join("src/lib.rs"),
        "pub fn greet() -> &'static str {\n    \"hello from syft\"\n}\n",
    )
    .unwrap();

    let result_snapshot = syft_json(repo.path(), &["snapshot", "capture"]);
    let change = syft_json(
        repo.path(),
        &[
            "change",
            "propose",
            "--title",
            "Break greet",
            "--intent",
            "prove validation ignores stale target output",
            "--result",
            result_snapshot["id"].as_str().unwrap(),
        ],
    );

    let diff = syft_json(
        repo.path(),
        &["change", "diff", change["id"].as_str().unwrap()],
    );
    let ops = diff["ops"].as_array().unwrap();
    assert!(ops.iter().all(|op| {
        !op["path"]
            .as_str()
            .unwrap()
            .starts_with("target/")
    }));
    assert!(ops.iter().any(|op| op["path"] == "src/lib.rs"));

    let validate_text = syft_text(
        repo.path(),
        &["change", "validate", change["id"].as_str().unwrap(), "--tests"],
    );
    assert!(validate_text.contains("status=Rejected"));
    assert!(validate_text.contains("validation=failed"));
    assert!(validate_text.contains("ran=1"));

    let detail = syft_json(
        repo.path(),
        &["change", "show", change["id"].as_str().unwrap(), "--logs"],
    );
    assert_eq!(detail["node"]["status"], "Rejected");
    assert_eq!(detail["validations"][0]["artifact"]["status"], "Failed");
    assert!(
        detail["node"]["semantic_delta"]["touched_symbols"]
            .as_array()
            .unwrap()
            .iter()
            .any(|symbol| symbol["display_name"] == "greet")
    );

    let diff_text = syft_text(
        repo.path(),
        &["change", "diff", change["id"].as_str().unwrap()],
    );
    assert!(!diff_text.contains("target/"));

    let status = syft_json(repo.path(), &["status"]);
    assert!(
        status["attention_needed"]
            .as_array()
            .unwrap()
            .iter()
            .any(|item| item.as_str().unwrap().contains("failing validations"))
    );
    assert_eq!(
        status["latest_validated_or_failed_change"]["validation_status"],
        "Failed"
    );

    let history = syft_json(repo.path(), &["history"]);
    assert_eq!(history[0]["node_id"], change["id"]);
    assert_eq!(history[0]["validation_status"], "Failed");
}

fn setup_fixture_repo() -> tempfile::TempDir {
    let repo = tempdir().unwrap();
    run(repo.path(), "git", &["init"]);
    run(
        repo.path(),
        "git",
        &["config", "user.email", "test@example.com"],
    );
    run(repo.path(), "git", &["config", "user.name", "Test User"]);
    fs::create_dir_all(repo.path().join("src")).unwrap();
    fs::create_dir_all(repo.path().join("tests")).unwrap();
    fs::write(
        repo.path().join("Cargo.toml"),
        "[package]\nname = \"fixture\"\nversion = \"0.1.0\"\nedition = \"2024\"\n",
    )
    .unwrap();
    fs::write(
        repo.path().join("src/main.rs"),
        "fn main() {\n    println!(\"hello\");\n}\n",
    )
    .unwrap();
    fs::write(
        repo.path().join("src/lib.rs"),
        "pub fn greet() -> &'static str {\n    \"hello\"\n}\n",
    )
    .unwrap();
    fs::write(
        repo.path().join("tests/smoke.rs"),
        "use fixture::greet;\n\n#[test]\nfn smoke() {\n    assert_eq!(greet(), \"hello\");\n}\n",
    )
    .unwrap();
    run(repo.path(), "git", &["add", "."]);
    run(repo.path(), "git", &["commit", "-m", "init"]);
    repo
}

fn syft_json(repo_path: &Path, args: &[&str]) -> Value {
    let mut full_args = vec!["--json"];
    full_args.extend_from_slice(args);
    let output = run_capture(repo_path, syft_bin().as_str(), &full_args);
    serde_json::from_str(&output).unwrap()
}

fn syft_text(repo_path: &Path, args: &[&str]) -> String {
    run_capture(repo_path, syft_bin().as_str(), args)
}

fn syft_fail(repo_path: &Path, args: &[&str]) -> String {
    run_capture_fail(repo_path, syft_bin().as_str(), args)
}

fn syft_bin() -> String {
    std::env::var("CARGO_BIN_EXE_syft").expect("syft binary path")
}

fn run(repo_path: &Path, program: &str, args: &[&str]) {
    let status = Command::new(program)
        .args(args)
        .current_dir(repo_path)
        .status()
        .unwrap();
    assert!(status.success(), "{program:?} {:?} failed", args);
}

fn run_capture(repo_path: &Path, program: &str, args: &[&str]) -> String {
    let output = Command::new(program)
        .args(args)
        .current_dir(repo_path)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{program:?} {:?} failed: {}",
        args,
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap()
}

fn run_capture_fail(repo_path: &Path, program: &str, args: &[&str]) -> String {
    let output = Command::new(program)
        .args(args)
        .current_dir(repo_path)
        .output()
        .unwrap();
    assert!(
        !output.status.success(),
        "{program:?} {:?} unexpectedly succeeded",
        args
    );
    String::from_utf8_lossy(&output.stderr).to_string()
}
