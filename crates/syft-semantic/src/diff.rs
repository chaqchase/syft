use std::collections::{BTreeMap, BTreeSet};

use anyhow::Result;
use syft_objects::{diff_snapshot_indices, materialize_snapshot, snapshot_index};
use syft_store::ObjectStore;
use syft_types::{
    DependencyEdgeChange, DependencyEdgeChangeKind, SemanticDelta, Snapshot, SnapshotIndex,
    SymbolDescriptor, SymbolRef, Visibility,
};
use tempfile::tempdir;

use crate::extract::index_rust_directory;
use crate::SemanticIndexResult;

pub fn index_snapshot(
    snapshot: &Snapshot,
    object_store: &dyn ObjectStore,
) -> Result<SemanticIndexResult> {
    let temp = tempdir()?;
    materialize_snapshot(&snapshot.root_tree_hash, temp.path(), object_store)?;
    index_rust_directory(temp.path())
}

pub fn diff_snapshots(
    base: &Snapshot,
    next: &Snapshot,
    object_store: &dyn ObjectStore,
) -> Result<SemanticDelta> {
    let base_index = snapshot_index(&base.root_tree_hash, object_store)?;
    let next_index = snapshot_index(&next.root_tree_hash, object_store)?;
    let changed_files = changed_files(&base_index, &next_index);

    let base_semantics = index_snapshot(base, object_store)?;
    let next_semantics = index_snapshot(next, object_store)?;
    let base_map = descriptor_map(&base_semantics.symbols);
    let next_map = descriptor_map(&next_semantics.symbols);

    let base_ids: BTreeSet<String> = base_map.keys().cloned().collect();
    let next_ids: BTreeSet<String> = next_map.keys().cloned().collect();

    let added_symbols = next_ids
        .difference(&base_ids)
        .filter_map(|key| next_map.get(key).map(|descriptor| descriptor.symbol.clone()))
        .collect::<Vec<_>>();
    let removed_symbols = base_ids
        .difference(&next_ids)
        .filter_map(|key| base_map.get(key).map(|descriptor| descriptor.symbol.clone()))
        .collect::<Vec<_>>();

    let mut touched_symbols = Vec::new();
    for key in base_ids.intersection(&next_ids) {
        if let (Some(left), Some(right)) = (base_map.get(key), next_map.get(key))
            && descriptor_signature(left) != descriptor_signature(right)
        {
            touched_symbols.push(right.symbol.clone());
        }
    }

    let changed_public_api = public_api_changed(
        &added_symbols,
        &removed_symbols,
        &base_ids,
        &next_ids,
        &base_map,
        &next_map,
    );

    let changed_dependencies = dependency_changes(&base_index, &next_index, object_store)?;
    let public_api_summary = public_api_summary(
        &added_symbols,
        &removed_symbols,
        &touched_symbols,
        &next_map,
        &base_map,
    );
    let summary = format!(
        "{} files changed, {} symbols added, {} removed, {} modified{}",
        changed_files.len(),
        added_symbols.len(),
        removed_symbols.len(),
        touched_symbols.len(),
        public_api_summary
    );

    Ok(SemanticDelta {
        touched_symbols,
        added_symbols,
        removed_symbols,
        changed_public_api,
        changed_dependencies,
        changed_files,
        summary,
    })
}

fn descriptor_map(symbols: &[SymbolDescriptor]) -> BTreeMap<String, SymbolDescriptor> {
    symbols
        .iter()
        .map(|descriptor| (descriptor.symbol.id.path.clone(), descriptor.clone()))
        .collect()
}

fn descriptor_signature(descriptor: &SymbolDescriptor) -> String {
    format!(
        "{:?}:{:?}:{}",
        descriptor.category,
        descriptor.tags,
        serde_json::to_string(&descriptor.attributes).unwrap_or_default()
    )
}

fn public_api_summary(
    added_symbols: &[SymbolRef],
    removed_symbols: &[SymbolRef],
    touched_symbols: &[SymbolRef],
    next_map: &BTreeMap<String, SymbolDescriptor>,
    base_map: &BTreeMap<String, SymbolDescriptor>,
) -> String {
    let public_added = collect_public_paths(added_symbols, next_map);
    let public_removed = collect_public_paths(removed_symbols, base_map);
    let public_modified = touched_symbols
        .iter()
        .filter(|symbol| matches!(symbol.source.visibility, Visibility::Public))
        .map(|symbol| symbol.id.path.clone())
        .collect::<Vec<_>>();

    if public_added.is_empty() && public_removed.is_empty() && public_modified.is_empty() {
        String::new()
    } else {
        let mut parts = Vec::new();
        if !public_added.is_empty() {
            parts.push(format!("added [{}]", public_added.join(", ")));
        }
        if !public_removed.is_empty() {
            parts.push(format!("removed [{}]", public_removed.join(", ")));
        }
        if !public_modified.is_empty() {
            parts.push(format!("modified [{}]", public_modified.join(", ")));
        }
        format!("; public API {}", parts.join("; "))
    }
}

fn public_api_changed(
    added_symbols: &[SymbolRef],
    removed_symbols: &[SymbolRef],
    base_ids: &BTreeSet<String>,
    next_ids: &BTreeSet<String>,
    base_map: &BTreeMap<String, SymbolDescriptor>,
    next_map: &BTreeMap<String, SymbolDescriptor>,
) -> bool {
    if added_symbols
        .iter()
        .chain(removed_symbols.iter())
        .any(|symbol| matches!(symbol.source.visibility, Visibility::Public))
    {
        return true;
    }

    base_ids.intersection(next_ids).any(|key| {
        let Some(left) = base_map.get(key) else {
            return false;
        };
        let Some(right) = next_map.get(key) else {
            return false;
        };
        matches!(right.symbol.source.visibility, Visibility::Public)
            && public_api_signature(left) != public_api_signature(right)
    })
}

fn collect_public_paths(
    symbols: &[SymbolRef],
    descriptors: &BTreeMap<String, SymbolDescriptor>,
) -> Vec<String> {
    symbols
        .iter()
        .filter(|symbol| {
            descriptors
                .get(&symbol.id.path)
                .map(|descriptor| matches!(descriptor.symbol.source.visibility, Visibility::Public))
                .unwrap_or(matches!(symbol.source.visibility, Visibility::Public))
        })
        .map(|symbol| symbol.id.path.clone())
        .collect()
}

fn public_api_signature(descriptor: &SymbolDescriptor) -> String {
    descriptor
        .attributes
        .get("signature")
        .map(|value| value.to_string())
        .unwrap_or_else(|| descriptor_signature(descriptor))
}

fn changed_files(base: &SnapshotIndex, next: &SnapshotIndex) -> Vec<String> {
    diff_snapshot_indices(base, next)
        .into_iter()
        .map(|op| op.path)
        .collect()
}

fn dependency_changes(
    base: &SnapshotIndex,
    next: &SnapshotIndex,
    object_store: &dyn ObjectStore,
) -> Result<Vec<DependencyEdgeChange>> {
    let mut changes = Vec::new();

    let cargo_toml = ("Cargo.toml", "cargo");
    let cargo_lock = ("Cargo.lock", "cargo-lock");
    for (path, label) in [cargo_toml, cargo_lock] {
        let before = read_blob_text(base, path, object_store)?;
        let after = read_blob_text(next, path, object_store)?;
        match (before, after) {
            (None, Some(_)) => changes.push(DependencyEdgeChange {
                from: "repo".to_string(),
                to: label.to_string(),
                kind: DependencyEdgeChangeKind::Added,
            }),
            (Some(_), None) => changes.push(DependencyEdgeChange {
                from: "repo".to_string(),
                to: label.to_string(),
                kind: DependencyEdgeChangeKind::Removed,
            }),
            (Some(left), Some(right)) if left != right => changes.push(DependencyEdgeChange {
                from: "repo".to_string(),
                to: label.to_string(),
                kind: DependencyEdgeChangeKind::Added,
            }),
            _ => {}
        }
    }

    Ok(changes)
}

fn read_blob_text(
    index: &SnapshotIndex,
    path: &str,
    object_store: &dyn ObjectStore,
) -> Result<Option<String>> {
    let Some(hash) = index.files.get(path) else {
        return Ok(None);
    };
    let Some(bytes) = object_store.get_bytes(hash)? else {
        return Ok(None);
    };
    let blob: serde_json::Value = serde_json::from_slice(&bytes)?;
    let text = blob
        .get("bytes")
        .and_then(|value| value.as_array())
        .map(|values| {
            values
                .iter()
                .filter_map(|value| value.as_u64().map(|byte| byte as u8))
                .collect::<Vec<u8>>()
        })
        .and_then(|raw| String::from_utf8(raw).ok());
    Ok(text)
}
