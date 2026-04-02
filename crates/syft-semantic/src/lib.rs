use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;

use anyhow::{Context, Result};
use quote::ToTokens;
use syft_objects::{diff_snapshot_indices, materialize_snapshot, snapshot_index};
use syft_store::ObjectStore;
use syft_types::{
    DependencyEdgeChange, DependencyEdgeChangeKind, EdgeKind, Language, SemanticDelta,
    SemanticEdge, Snapshot, SnapshotIndex, SpanRef, SymbolCategory, SymbolDescriptor, SymbolId,
    SymbolRef, SymbolSource, SymbolTarget, Visibility,
};
use syn::spanned::Spanned;
use syn::{File, Item, Visibility as SynVisibility};
use tempfile::tempdir;

#[derive(Debug, Clone)]
pub struct SemanticIndexResult {
    pub symbols: Vec<SymbolDescriptor>,
    pub public_api_symbols: Vec<SymbolDescriptor>,
    pub edges: Vec<SemanticEdge>,
}

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
        .filter_map(|key| {
            next_map
                .get(key)
                .map(|descriptor| descriptor.symbol.clone())
        })
        .collect::<Vec<_>>();
    let removed_symbols = base_ids
        .difference(&next_ids)
        .filter_map(|key| {
            base_map
                .get(key)
                .map(|descriptor| descriptor.symbol.clone())
        })
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

pub fn index_rust_directory(root: &Path) -> Result<SemanticIndexResult> {
    let mut symbols = Vec::new();
    let mut edges = Vec::new();
    walk_directory(root, root, &mut symbols, &mut edges)?;
    let public_api_symbols = symbols
        .iter()
        .filter(|descriptor| matches!(descriptor.symbol.source.visibility, Visibility::Public))
        .cloned()
        .collect();

    Ok(SemanticIndexResult {
        symbols,
        public_api_symbols,
        edges,
    })
}

pub fn extract_rust_symbols(path: &Path, content: &str) -> Result<Vec<SymbolDescriptor>> {
    let parsed = syn::parse_file(content)
        .with_context(|| format!("failed to parse Rust source {}", path.display()))?;
    let relative = normalize_path(path);
    let module_path = module_path_from_file(path);
    let mut symbols = Vec::new();
    let mut edges = Vec::new();
    collect_items(
        &parsed,
        &relative,
        &module_path,
        &mut symbols,
        &mut edges,
        None,
    );
    Ok(symbols)
}

fn walk_directory(
    root: &Path,
    current: &Path,
    symbols: &mut Vec<SymbolDescriptor>,
    edges: &mut Vec<SemanticEdge>,
) -> Result<()> {
    for entry in fs::read_dir(current)? {
        let entry = entry?;
        let path = entry.path();
        if path.file_name().is_some_and(|name| name == "target") {
            continue;
        }
        if path.is_dir() {
            walk_directory(root, &path, symbols, edges)?;
        } else if path.extension().is_some_and(|ext| ext == "rs") {
            let source = fs::read_to_string(&path)?;
            let relative = path.strip_prefix(root).unwrap_or(&path);
            let parsed = syn::parse_file(&source)?;
            let file_path = normalize_path(relative);
            let module_path = module_path_from_file(relative);
            collect_items(&parsed, &file_path, &module_path, symbols, edges, None);
        }
    }
    Ok(())
}

fn collect_items(
    parsed: &File,
    file_path: &str,
    module_path: &str,
    symbols: &mut Vec<SymbolDescriptor>,
    edges: &mut Vec<SemanticEdge>,
    parent: Option<SymbolId>,
) {
    for item in &parsed.items {
        collect_item(item, file_path, module_path, symbols, edges, parent.clone());
    }
}

fn collect_item(
    item: &Item,
    file_path: &str,
    module_path: &str,
    symbols: &mut Vec<SymbolDescriptor>,
    edges: &mut Vec<SemanticEdge>,
    parent: Option<SymbolId>,
) {
    match item {
        Item::Fn(item_fn) => {
            let symbol = descriptor(
                file_path,
                module_path,
                item_fn.sig.ident.to_string(),
                span_ref(item_fn),
                visibility(&item_fn.vis),
                SymbolCategory::Callable,
                vec!["rust".to_string(), "fn".to_string()],
                [
                    (
                        "signature".to_string(),
                        serde_json::Value::String(normalize_signature(&item_fn.sig)),
                    ),
                    (
                        "body".to_string(),
                        serde_json::Value::String(normalize_signature(&item_fn.block)),
                    ),
                ]
                .into_iter()
                .collect(),
            );
            link_parent(edges, parent, &symbol.symbol.id);
            symbols.push(symbol);
        }
        Item::Struct(item_struct) => {
            let symbol = descriptor(
                file_path,
                module_path,
                item_struct.ident.to_string(),
                span_ref(item_struct),
                visibility(&item_struct.vis),
                SymbolCategory::Type,
                vec!["rust".to_string(), "struct".to_string()],
                [
                    (
                        "fields".to_string(),
                        serde_json::Value::from(item_struct.fields.len() as u64),
                    ),
                    (
                        "signature".to_string(),
                        serde_json::Value::String(normalize_signature(item_struct)),
                    ),
                ]
                .into_iter()
                .collect(),
            );
            link_parent(edges, parent, &symbol.symbol.id);
            symbols.push(symbol);
        }
        Item::Enum(item_enum) => {
            let symbol = descriptor(
                file_path,
                module_path,
                item_enum.ident.to_string(),
                span_ref(item_enum),
                visibility(&item_enum.vis),
                SymbolCategory::Type,
                vec!["rust".to_string(), "enum".to_string()],
                [
                    (
                        "variants".to_string(),
                        serde_json::Value::from(item_enum.variants.len() as u64),
                    ),
                    (
                        "signature".to_string(),
                        serde_json::Value::String(normalize_signature(item_enum)),
                    ),
                ]
                .into_iter()
                .collect(),
            );
            link_parent(edges, parent, &symbol.symbol.id);
            symbols.push(symbol);
        }
        Item::Trait(item_trait) => {
            let symbol = descriptor(
                file_path,
                module_path,
                item_trait.ident.to_string(),
                span_ref(item_trait),
                visibility(&item_trait.vis),
                SymbolCategory::Type,
                vec!["rust".to_string(), "trait".to_string()],
                [
                    (
                        "items".to_string(),
                        serde_json::Value::from(item_trait.items.len() as u64),
                    ),
                    (
                        "signature".to_string(),
                        serde_json::Value::String(normalize_signature(item_trait)),
                    ),
                ]
                .into_iter()
                .collect(),
            );
            link_parent(edges, parent, &symbol.symbol.id);
            symbols.push(symbol);
        }
        Item::Type(item_type) => {
            let symbol = descriptor(
                file_path,
                module_path,
                item_type.ident.to_string(),
                span_ref(item_type),
                visibility(&item_type.vis),
                SymbolCategory::Type,
                vec!["rust".to_string(), "type-alias".to_string()],
                [(
                    "signature".to_string(),
                    serde_json::Value::String(normalize_signature(item_type)),
                )]
                .into_iter()
                .collect(),
            );
            link_parent(edges, parent, &symbol.symbol.id);
            symbols.push(symbol);
        }
        Item::Const(item_const) => {
            let symbol = descriptor(
                file_path,
                module_path,
                item_const.ident.to_string(),
                span_ref(item_const),
                visibility(&item_const.vis),
                SymbolCategory::Value,
                vec!["rust".to_string(), "const".to_string()],
                [(
                    "signature".to_string(),
                    serde_json::Value::String(normalize_signature(item_const)),
                )]
                .into_iter()
                .collect(),
            );
            link_parent(edges, parent, &symbol.symbol.id);
            symbols.push(symbol);
        }
        Item::Static(item_static) => {
            let symbol = descriptor(
                file_path,
                module_path,
                item_static.ident.to_string(),
                span_ref(item_static),
                visibility(&item_static.vis),
                SymbolCategory::Value,
                vec!["rust".to_string(), "static".to_string()],
                [(
                    "signature".to_string(),
                    serde_json::Value::String(normalize_signature(item_static)),
                )]
                .into_iter()
                .collect(),
            );
            link_parent(edges, parent, &symbol.symbol.id);
            symbols.push(symbol);
        }
        Item::Mod(item_mod) => {
            let module_descriptor = descriptor(
                file_path,
                module_path,
                item_mod.ident.to_string(),
                span_ref(item_mod),
                visibility(&item_mod.vis),
                SymbolCategory::Namespace,
                vec!["rust".to_string(), "module".to_string()],
                [(
                    "signature".to_string(),
                    serde_json::Value::String(normalize_signature(item_mod)),
                )]
                .into_iter()
                .collect(),
            );
            let current_id = module_descriptor.symbol.id.clone();
            link_parent(edges, parent, &current_id);
            symbols.push(module_descriptor);
            if let Some((_, items)) = &item_mod.content {
                let nested_file = File {
                    shebang: None,
                    attrs: Vec::new(),
                    items: items.clone(),
                };
                let next_module_path = join_module_path(module_path, &item_mod.ident.to_string());
                collect_items(
                    &nested_file,
                    file_path,
                    &next_module_path,
                    symbols,
                    edges,
                    Some(current_id),
                );
            }
        }
        _ => {}
    }
}

fn descriptor(
    file_path: &str,
    module_path: &str,
    local_name: String,
    span: SpanRef,
    visibility: Visibility,
    category: SymbolCategory,
    tags: Vec<String>,
    attributes: BTreeMap<String, serde_json::Value>,
) -> SymbolDescriptor {
    let qualified = join_module_path(module_path, &local_name);
    SymbolDescriptor {
        symbol: SymbolRef {
            id: SymbolId {
                language: Language::Rust,
                namespace: module_path.to_string(),
                path: qualified.clone(),
                local_name: local_name.clone(),
                disambiguator: None,
            },
            display_name: qualified,
            source: SymbolSource {
                file_path: file_path.to_string(),
                span,
                visibility,
            },
        },
        category,
        tags,
        attributes,
    }
}

fn visibility(vis: &SynVisibility) -> Visibility {
    match vis {
        SynVisibility::Public(_) => Visibility::Public,
        SynVisibility::Restricted(_) => Visibility::Internal,
        SynVisibility::Inherited => Visibility::Private,
    }
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

fn link_parent(edges: &mut Vec<SemanticEdge>, parent: Option<SymbolId>, child: &SymbolId) {
    if let Some(parent) = parent {
        edges.push(SemanticEdge {
            from: parent,
            to: SymbolTarget::Symbol(child.clone()),
            kind: EdgeKind::Contains,
        });
    }
}

fn module_path_from_file(path: &Path) -> String {
    let mut parts = path
        .components()
        .map(|component| component.as_os_str().to_string_lossy().to_string())
        .collect::<Vec<_>>();
    if parts.first().is_some_and(|part| part == "src") {
        parts.remove(0);
    }

    let mut normalized = Vec::new();
    for part in parts {
        if let Some(stem) = part.strip_suffix(".rs") {
            if stem == "lib" || stem == "main" || stem == "mod" {
                continue;
            }
            normalized.push(stem.to_string());
        } else {
            normalized.push(part);
        }
    }

    normalized.join("::")
}

fn join_module_path(module_path: &str, local_name: &str) -> String {
    if module_path.is_empty() {
        local_name.to_string()
    } else {
        format!("{module_path}::{local_name}")
    }
}

fn normalize_path(path: &Path) -> String {
    path.components()
        .map(|component| component.as_os_str().to_string_lossy().to_string())
        .collect::<Vec<_>>()
        .join("/")
}

fn span_ref<T: Spanned>(value: &T) -> SpanRef {
    let span = value.span();
    let start = span.start();
    let end = span.end();
    SpanRef {
        start_line: start.line as u32,
        start_col: (start.column + 1) as u32,
        end_line: end.line as u32,
        end_col: (end.column + 1) as u32,
    }
}

fn normalize_signature<T: ToTokens>(value: &T) -> String {
    normalize_signature_text(&value.to_token_stream().to_string())
}

fn normalize_signature_text(raw: &str) -> String {
    let mut normalized = String::new();
    let mut pending_space = false;
    let punctuation = [
        '(', ')', '{', '}', '[', ']', ',', ':', ';', '<', '>', '&', '=', '-', '+', '!', '|', '?',
    ];

    for ch in raw.chars() {
        if ch.is_whitespace() {
            pending_space = true;
            continue;
        }

        if punctuation.contains(&ch) {
            if normalized.ends_with(' ') {
                normalized.pop();
            }
            normalized.push(ch);
            pending_space = false;
            continue;
        }

        if pending_space && !normalized.is_empty() && !normalized.ends_with(' ') {
            normalized.push(' ');
        }
        normalized.push(ch);
        pending_space = false;
    }

    normalized.trim().to_string()
}

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::tempdir;

    use super::*;
    use syft_objects::capture_directory;
    use syft_store::FsObjectStore;
    use syft_types::{SnapshotMetadata, SnapshotSource, new_entity_id, now_utc};

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

        let base_snapshot = Snapshot {
            id: new_entity_id(),
            parent_snapshot_ids: Vec::new(),
            root_tree_hash: base_hash,
            created_at: now_utc(),
            metadata: SnapshotMetadata {
                repo_id: "repo".to_string(),
                source: SnapshotSource::MaterializedByHuman,
                labels: Vec::new(),
            },
        };
        let next_snapshot = Snapshot {
            id: new_entity_id(),
            parent_snapshot_ids: Vec::new(),
            root_tree_hash: next_hash,
            created_at: now_utc(),
            metadata: SnapshotMetadata {
                repo_id: "repo".to_string(),
                source: SnapshotSource::MaterializedByHuman,
                labels: Vec::new(),
            },
        };

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

        let base_snapshot = Snapshot {
            id: new_entity_id(),
            parent_snapshot_ids: Vec::new(),
            root_tree_hash: base_hash,
            created_at: now_utc(),
            metadata: SnapshotMetadata {
                repo_id: "repo".to_string(),
                source: SnapshotSource::MaterializedByHuman,
                labels: Vec::new(),
            },
        };
        let next_snapshot = Snapshot {
            id: new_entity_id(),
            parent_snapshot_ids: Vec::new(),
            root_tree_hash: next_hash,
            created_at: now_utc(),
            metadata: SnapshotMetadata {
                repo_id: "repo".to_string(),
                source: SnapshotSource::MaterializedByHuman,
                labels: Vec::new(),
            },
        };

        let delta = diff_snapshots(&base_snapshot, &next_snapshot, &store).unwrap();
        assert_eq!(delta.touched_symbols[0].id.path, "greet");
        assert!(!delta.changed_public_api);
    }
}
