use std::fs;
use std::path::Path;

use anyhow::{Context, Result};
use syft_types::{
    SemanticEdge, SymbolCategory, SymbolDescriptor, SymbolId, Visibility,
};
use syn::{File, Item, Visibility as SynVisibility};

use crate::SemanticIndexResult;
use crate::support::{
    descriptor, join_module_path, link_parent, module_path_from_file, normalize_path,
    normalize_signature, span_ref,
};

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

fn visibility(vis: &SynVisibility) -> Visibility {
    match vis {
        SynVisibility::Public(_) => Visibility::Public,
        SynVisibility::Restricted(_) => Visibility::Internal,
        SynVisibility::Inherited => Visibility::Private,
    }
}
