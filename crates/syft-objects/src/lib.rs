use std::collections::BTreeMap;
use std::fs;
use std::path::{Component, Path, PathBuf};

use anyhow::{Context, Result, anyhow};
use serde::{Deserialize, Serialize};
use syft_store::ObjectStore;
use syft_types::{
    FileMode, ObjectHash, PatchOp, PatchOpKind, SnapshotIndex, TreeEntry, TreeObject,
};

#[derive(Debug, Clone, Serialize, Deserialize)]
struct StoredBlob {
    mode: FileMode,
    bytes: Vec<u8>,
}

#[derive(Debug, Default)]
struct TreeNode {
    files: BTreeMap<String, (FileMode, ObjectHash)>,
    dirs: BTreeMap<String, TreeNode>,
}

pub const DEFAULT_CAPTURE_EXCLUDES: &[&str] = &[".git", ".syft", "target"];

pub fn effective_capture_excludes(extra_excludes: &[String]) -> Vec<String> {
    let mut excludes = DEFAULT_CAPTURE_EXCLUDES
        .iter()
        .map(|value| value.to_string())
        .collect::<Vec<_>>();

    for value in extra_excludes {
        if let Some(rule) = normalize_exclude_rule(value)
            && !excludes.contains(&rule)
        {
            excludes.push(rule);
        }
    }

    excludes
}

pub fn path_is_excluded(relative_path: &Path, exclude_paths: &[String]) -> bool {
    let normalized = normalize_path(relative_path);
    if normalized.is_empty() {
        return false;
    }

    exclude_paths.iter().any(|value| {
        normalize_exclude_rule(value).is_some_and(|rule| {
            normalized == rule || normalized.starts_with(&format!("{rule}/"))
        })
    })
}

pub fn filter_capture_paths(relative_paths: &[PathBuf], exclude_paths: &[String]) -> Vec<PathBuf> {
    relative_paths
        .iter()
        .filter(|path| !path_is_excluded(path, exclude_paths))
        .cloned()
        .collect()
}

pub fn remove_excluded_paths(root: &Path, exclude_paths: &[String]) -> Result<()> {
    for value in exclude_paths {
        let Some(rule) = normalize_exclude_rule(value) else {
            continue;
        };
        let path = root.join(rule);
        if !path.exists() {
            continue;
        }
        if path.is_dir() {
            fs::remove_dir_all(&path)?;
        } else {
            fs::remove_file(&path)?;
        }
    }
    Ok(())
}

pub fn capture_directory(
    root: &Path,
    object_store: &dyn ObjectStore,
    exclude_paths: &[String],
) -> Result<(ObjectHash, SnapshotIndex)> {
    let mut paths = Vec::new();
    collect_paths(root, root, &mut paths)?;
    let paths = filter_capture_paths(&paths, exclude_paths);
    capture_paths(root, &paths, object_store)
}

pub fn capture_paths(
    root: &Path,
    relative_paths: &[PathBuf],
    object_store: &dyn ObjectStore,
) -> Result<(ObjectHash, SnapshotIndex)> {
    let mut tree = TreeNode::default();
    let mut files = BTreeMap::new();

    for relative_path in relative_paths {
        let absolute_path = root.join(relative_path);
        if absolute_path.is_dir() {
            continue;
        }

        let bytes = fs::read(&absolute_path)
            .with_context(|| format!("failed to read file {}", absolute_path.display()))?;
        let mode = file_mode(&absolute_path)?;
        let blob = StoredBlob {
            mode: mode.clone(),
            bytes,
        };
        let blob_bytes = serde_json::to_vec(&blob)?;
        let blob_hash = object_store.put_bytes(&blob_bytes)?;

        let relative_string = normalize_path(relative_path);
        files.insert(relative_string.clone(), blob_hash.clone());
        insert_file(&mut tree, relative_path, mode, blob_hash)?;
    }

    let root_hash = persist_tree(&tree, object_store)?;
    Ok((root_hash, SnapshotIndex { files }))
}

pub fn capture_virtual_entries(
    entries: &[(PathBuf, FileMode, Vec<u8>)],
    object_store: &dyn ObjectStore,
) -> Result<(ObjectHash, SnapshotIndex)> {
    let mut tree = TreeNode::default();
    let mut files = BTreeMap::new();

    for (relative_path, mode, bytes) in entries {
        let blob = StoredBlob {
            mode: mode.clone(),
            bytes: bytes.clone(),
        };
        let blob_bytes = serde_json::to_vec(&blob)?;
        let blob_hash = object_store.put_bytes(&blob_bytes)?;
        let relative_string = normalize_path(relative_path);
        files.insert(relative_string.clone(), blob_hash.clone());
        insert_file(&mut tree, relative_path, mode.clone(), blob_hash)?;
    }

    let root_hash = persist_tree(&tree, object_store)?;
    Ok((root_hash, SnapshotIndex { files }))
}

pub fn materialize_snapshot(
    root_hash: &str,
    destination: &Path,
    object_store: &dyn ObjectStore,
) -> Result<()> {
    fs::create_dir_all(destination)?;
    materialize_tree(root_hash, destination, object_store)
}

pub fn snapshot_index(root_hash: &str, object_store: &dyn ObjectStore) -> Result<SnapshotIndex> {
    let mut files = BTreeMap::new();
    load_tree_index(root_hash, Path::new(""), object_store, &mut files)?;
    Ok(SnapshotIndex { files })
}

pub fn diff_snapshot_indices(base: &SnapshotIndex, next: &SnapshotIndex) -> Vec<PatchOp> {
    let mut ops = Vec::new();

    for (path, before_hash) in &base.files {
        match next.files.get(path) {
            Some(after_hash) if after_hash != before_hash => ops.push(PatchOp {
                path: path.clone(),
                kind: PatchOpKind::Modify,
                old_path: None,
                before_hash: Some(before_hash.clone()),
                after_hash: Some(after_hash.clone()),
            }),
            None => ops.push(PatchOp {
                path: path.clone(),
                kind: PatchOpKind::Delete,
                old_path: None,
                before_hash: Some(before_hash.clone()),
                after_hash: None,
            }),
            _ => {}
        }
    }

    for (path, after_hash) in &next.files {
        if !base.files.contains_key(path) {
            ops.push(PatchOp {
                path: path.clone(),
                kind: PatchOpKind::Add,
                old_path: None,
                before_hash: None,
                after_hash: Some(after_hash.clone()),
            });
        }
    }

    ops.sort_by(|left, right| left.path.cmp(&right.path));
    ops
}

fn collect_paths(
    root: &Path,
    current: &Path,
    paths: &mut Vec<PathBuf>,
) -> Result<()> {
    for entry in fs::read_dir(current)? {
        let entry = entry?;
        let path = entry.path();
        let relative = path
            .strip_prefix(root)
            .map_err(|_| anyhow!("failed to strip root prefix"))?;
        if path.is_dir() {
            collect_paths(root, &path, paths)?;
        } else {
            paths.push(relative.to_path_buf());
        }
    }
    paths.sort();
    Ok(())
}

fn file_mode(path: &Path) -> Result<FileMode> {
    let metadata = fs::symlink_metadata(path)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = metadata.permissions().mode();
        if metadata.file_type().is_symlink() {
            return Ok(FileMode::Symlink);
        }
        if mode & 0o111 != 0 {
            return Ok(FileMode::Executable);
        }
    }
    Ok(FileMode::File)
}

fn insert_file(
    node: &mut TreeNode,
    relative_path: &Path,
    mode: FileMode,
    hash: ObjectHash,
) -> Result<()> {
    let mut components = relative_path.components().peekable();
    let mut current = node;

    while let Some(component) = components.next() {
        let name = component.as_os_str().to_string_lossy().to_string();
        if components.peek().is_none() {
            current.files.insert(name, (mode.clone(), hash.clone()));
        } else {
            current = current.dirs.entry(name).or_default();
        }
    }

    Ok(())
}

fn persist_tree(node: &TreeNode, object_store: &dyn ObjectStore) -> Result<ObjectHash> {
    let mut entries = Vec::new();

    for (name, (mode, hash)) in &node.files {
        entries.push(TreeEntry {
            name: name.clone(),
            mode: mode.clone(),
            hash: hash.clone(),
        });
    }

    for (name, child) in &node.dirs {
        let child_hash = persist_tree(child, object_store)?;
        entries.push(TreeEntry {
            name: name.clone(),
            mode: FileMode::Directory,
            hash: child_hash,
        });
    }

    entries.sort_by(|left, right| left.name.cmp(&right.name));
    let tree = TreeObject { entries };
    let bytes = serde_json::to_vec(&tree)?;
    object_store.put_bytes(&bytes)
}

fn materialize_tree(
    root_hash: &str,
    destination: &Path,
    object_store: &dyn ObjectStore,
) -> Result<()> {
    let tree = read_tree_object(root_hash, object_store)?;
    for entry in tree.entries {
        let path = destination.join(&entry.name);
        match entry.mode {
            FileMode::Directory => {
                fs::create_dir_all(&path)?;
                materialize_tree(&entry.hash, &path, object_store)?;
            }
            _ => {
                let blob_bytes = object_store
                    .get_bytes(&entry.hash)?
                    .ok_or_else(|| anyhow!("missing blob {}", entry.hash))?;
                let blob: StoredBlob = serde_json::from_slice(&blob_bytes)?;
                if let Some(parent) = path.parent() {
                    fs::create_dir_all(parent)?;
                }
                fs::write(&path, blob.bytes)?;
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    if matches!(blob.mode, FileMode::Executable) {
                        let mut permissions = fs::metadata(&path)?.permissions();
                        permissions.set_mode(0o755);
                        fs::set_permissions(&path, permissions)?;
                    }
                }
            }
        }
    }
    Ok(())
}

fn load_tree_index(
    root_hash: &str,
    base: &Path,
    object_store: &dyn ObjectStore,
    files: &mut BTreeMap<String, ObjectHash>,
) -> Result<()> {
    let tree = read_tree_object(root_hash, object_store)?;
    for entry in tree.entries {
        let path = base.join(&entry.name);
        match entry.mode {
            FileMode::Directory => load_tree_index(&entry.hash, &path, object_store, files)?,
            _ => {
                files.insert(normalize_path(&path), entry.hash.clone());
            }
        }
    }
    Ok(())
}

fn read_tree_object(hash: &str, object_store: &dyn ObjectStore) -> Result<TreeObject> {
    let bytes = object_store
        .get_bytes(hash)?
        .ok_or_else(|| anyhow!("missing tree {}", hash))?;
    serde_json::from_slice(&bytes).context("failed to deserialize tree object")
}

fn normalize_path(path: &Path) -> String {
    path.components()
        .filter_map(|component| match component {
            Component::Normal(value) => Some(value.to_string_lossy().to_string()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("/")
}

fn normalize_exclude_rule(value: &str) -> Option<String> {
    let normalized = Path::new(value)
        .components()
        .filter_map(|component| match component {
            Component::CurDir => None,
            Component::Normal(value) => Some(value.to_string_lossy().to_string()),
            _ => Some(String::new()),
        })
        .collect::<Vec<_>>();

    if normalized.is_empty() || normalized.iter().any(|value| value.is_empty()) {
        return None;
    }

    Some(normalized.join("/"))
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::Path;

    use tempfile::tempdir;

    use super::*;
    use syft_store::FsObjectStore;

    #[test]
    fn path_matching_supports_defaults_and_relative_prefixes() {
        let excludes = effective_capture_excludes(&[
            ".cache/build".to_string(),
            "dist/app.js".to_string(),
        ]);

        assert!(path_is_excluded(Path::new("target/debug/app"), &excludes));
        assert!(path_is_excluded(Path::new(".syft/state/head"), &excludes));
        assert!(path_is_excluded(Path::new(".cache/build/output.txt"), &excludes));
        assert!(path_is_excluded(Path::new("dist/app.js"), &excludes));
        assert!(!path_is_excluded(Path::new("targeted/debug/app"), &excludes));
        assert!(!path_is_excluded(Path::new(".cache/build-output.txt"), &excludes));
        assert!(!path_is_excluded(Path::new("dist/app.js.map"), &excludes));
    }

    #[test]
    fn capture_directory_respects_excludes() {
        let src = tempdir().unwrap();
        let out = tempdir().unwrap();
        fs::write(
            src.path().join("Cargo.toml"),
            "[package]\nname = \"fixture\"\n",
        )
        .unwrap();
        fs::create_dir_all(src.path().join("src")).unwrap();
        fs::write(src.path().join("src/main.rs"), "fn main() {}\n").unwrap();
        fs::create_dir_all(src.path().join("target/debug")).unwrap();
        fs::write(src.path().join("target/debug/app"), "compiled").unwrap();

        let store_dir = tempdir().unwrap();
        let store = FsObjectStore::new(store_dir.path());
        let (hash, index) =
            capture_directory(src.path(), &store, &effective_capture_excludes(&[])).unwrap();
        assert_eq!(index.files.len(), 2);
        assert!(!index.files.contains_key("target/debug/app"));

        materialize_snapshot(&hash, out.path(), &store).unwrap();
        let restored = fs::read_to_string(out.path().join("src/main.rs")).unwrap();
        assert_eq!(restored, "fn main() {}\n");
        assert!(!out.path().join("target").exists());
    }

    #[test]
    fn remove_excluded_paths_cleans_materialized_tree() {
        let root = tempdir().unwrap();
        fs::create_dir_all(root.path().join("target/debug")).unwrap();
        fs::write(root.path().join("target/debug/app"), "compiled").unwrap();
        fs::create_dir_all(root.path().join(".cache/build")).unwrap();
        fs::write(root.path().join(".cache/build/output"), "cached").unwrap();
        fs::write(root.path().join("src.rs"), "fn main() {}\n").unwrap();

        remove_excluded_paths(
            root.path(),
            &effective_capture_excludes(&[".cache/build".to_string()]),
        )
        .unwrap();

        assert!(!root.path().join("target").exists());
        assert!(!root.path().join(".cache/build").exists());
        assert!(root.path().join("src.rs").exists());
    }

    #[test]
    fn capture_and_materialize_roundtrip() {
        let src = tempdir().unwrap();
        let out = tempdir().unwrap();
        fs::write(
            src.path().join("Cargo.toml"),
            "[package]\nname = \"fixture\"\n",
        )
        .unwrap();
        fs::create_dir_all(src.path().join("src")).unwrap();
        fs::write(src.path().join("src/main.rs"), "fn main() {}\n").unwrap();

        let store_dir = tempdir().unwrap();
        let store = FsObjectStore::new(store_dir.path());
        let (hash, index) = capture_directory(src.path(), &store, &Vec::<String>::new()).unwrap();
        assert_eq!(index.files.len(), 2);

        materialize_snapshot(&hash, out.path(), &store).unwrap();
        let restored = fs::read_to_string(out.path().join("src/main.rs")).unwrap();
        assert_eq!(restored, "fn main() {}\n");
    }
}
