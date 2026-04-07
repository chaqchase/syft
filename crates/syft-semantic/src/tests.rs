use std::fs;
use std::path::Path;

use tempfile::tempdir;

use crate::{diff_snapshots, extract_rust_symbols};
use syft_objects::capture_directory;
use syft_store::FsObjectStore;
use syft_types::{Snapshot, SnapshotMetadata, SnapshotSource, new_entity_id, now_utc};

#[test]
fn extractor_finds_public_items_and_modules() {
    let symbols = extract_rust_symbols(
        Path::new("src/lib.rs"),
        "pub struct User;\npub fn load(id: u32) {}\nmod inner { pub fn helper() {} }\n",
    )
    .unwrap();

    let ids = symbols
        .iter()
        .map(|symbol| symbol.symbol.id.path.clone())
        .collect::<Vec<_>>();
    assert!(ids.contains(&"User".to_string()));
    assert!(ids.contains(&"load".to_string()));
    assert!(ids.contains(&"inner".to_string()));
    assert!(ids.contains(&"inner::helper".to_string()));
    let user = symbols
        .iter()
        .find(|symbol| symbol.symbol.id.path == "User")
        .unwrap();
    assert!(user.symbol.source.span.start_line > 0);
    assert!(user.symbol.source.span.end_col > 0);
}

#[test]
fn formatting_only_signature_changes_are_ignored() {
    let first = extract_rust_symbols(
        Path::new("src/lib.rs"),
        "pub fn load(id: u32) -> &'static str { \"x\" }\n",
    )
    .unwrap();
    let second = extract_rust_symbols(
        Path::new("src/lib.rs"),
        "pub fn load( id : u32 )-> &'static str { \"x\" }\n",
    )
    .unwrap();

    let first_sig = first[0].attributes.get("signature").unwrap();
    let second_sig = second[0].attributes.get("signature").unwrap();
    assert_eq!(first_sig, second_sig);
}

#[test]
fn diff_detects_signature_change() {
    let base = tempdir().unwrap();
    let next = tempdir().unwrap();
    fs::create_dir_all(base.path().join("src")).unwrap();
    fs::create_dir_all(next.path().join("src")).unwrap();
    fs::write(base.path().join("src/lib.rs"), "pub fn load(id: u32) {}\n").unwrap();
    fs::write(next.path().join("src/lib.rs"), "pub fn load(id: u64) {}\n").unwrap();
    fs::write(
        base.path().join("Cargo.toml"),
        "[package]\nname=\"a\"\nversion=\"0.1.0\"\n",
    )
    .unwrap();
    fs::write(
        next.path().join("Cargo.toml"),
        "[package]\nname=\"a\"\nversion=\"0.1.0\"\n",
    )
    .unwrap();

    let store_dir = tempdir().unwrap();
    let store = FsObjectStore::new(store_dir.path());
    let (base_hash, _) = capture_directory(base.path(), &store, &[]).unwrap();
    let (next_hash, _) = capture_directory(next.path(), &store, &[]).unwrap();

    let base_snapshot = snapshot("repo", base_hash);
    let next_snapshot = snapshot("repo", next_hash);

    let delta = diff_snapshots(&base_snapshot, &next_snapshot, &store).unwrap();
    assert_eq!(delta.touched_symbols.len(), 1);
    assert!(delta.changed_public_api);
    assert!(delta.summary.contains("public API"));
    assert!(delta.summary.contains("load"));
}

#[test]
fn body_only_changes_touch_symbol_without_public_api_change() {
    let base = tempdir().unwrap();
    let next = tempdir().unwrap();
    fs::create_dir_all(base.path().join("src")).unwrap();
    fs::create_dir_all(next.path().join("src")).unwrap();
    fs::write(
        base.path().join("src/lib.rs"),
        "pub fn greet() -> &'static str { \"hello\" }\n",
    )
    .unwrap();
    fs::write(
        next.path().join("src/lib.rs"),
        "pub fn greet() -> &'static str { \"hello, syft\" }\n",
    )
    .unwrap();
    fs::write(
        base.path().join("Cargo.toml"),
        "[package]\nname=\"a\"\nversion=\"0.1.0\"\n",
    )
    .unwrap();
    fs::write(
        next.path().join("Cargo.toml"),
        "[package]\nname=\"a\"\nversion=\"0.1.0\"\n",
    )
    .unwrap();

    let store_dir = tempdir().unwrap();
    let store = FsObjectStore::new(store_dir.path());
    let (base_hash, _) = capture_directory(base.path(), &store, &[]).unwrap();
    let (next_hash, _) = capture_directory(next.path(), &store, &[]).unwrap();

    let base_snapshot = snapshot("repo", base_hash);
    let next_snapshot = snapshot("repo", next_hash);

    let delta = diff_snapshots(&base_snapshot, &next_snapshot, &store).unwrap();
    assert_eq!(delta.touched_symbols[0].id.path, "greet");
    assert!(!delta.changed_public_api);
}

fn snapshot(repo_id: &str, root_tree_hash: String) -> Snapshot {
    Snapshot {
        id: new_entity_id(),
        parent_snapshot_ids: Vec::new(),
        root_tree_hash,
        created_at: now_utc(),
        metadata: SnapshotMetadata {
            repo_id: repo_id.to_string(),
            source: SnapshotSource::MaterializedByHuman,
            labels: Vec::new(),
        },
    }
}
