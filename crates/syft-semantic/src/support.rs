use std::path::Path;

use quote::ToTokens;
use syft_types::{EdgeKind, Language, SemanticEdge, SpanRef, SymbolDescriptor, SymbolId, SymbolRef, SymbolSource, SymbolTarget, Visibility};
use syn::spanned::Spanned;

pub(crate) fn link_parent(edges: &mut Vec<SemanticEdge>, parent: Option<SymbolId>, child: &SymbolId) {
    if let Some(parent) = parent {
        edges.push(SemanticEdge {
            from: parent,
            to: SymbolTarget::Symbol(child.clone()),
            kind: EdgeKind::Contains,
        });
    }
}

pub(crate) fn module_path_from_file(path: &Path) -> String {
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

pub(crate) fn join_module_path(module_path: &str, local_name: &str) -> String {
    if module_path.is_empty() {
        local_name.to_string()
    } else {
        format!("{module_path}::{local_name}")
    }
}

pub(crate) fn normalize_path(path: &Path) -> String {
    path.components()
        .map(|component| component.as_os_str().to_string_lossy().to_string())
        .collect::<Vec<_>>()
        .join("/")
}

pub(crate) fn span_ref<T: Spanned>(value: &T) -> SpanRef {
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

pub(crate) fn normalize_signature<T: ToTokens>(value: &T) -> String {
    normalize_signature_text(&value.to_token_stream().to_string())
}

pub(crate) fn normalize_signature_text(raw: &str) -> String {
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

pub(crate) fn descriptor(
    file_path: &str,
    module_path: &str,
    local_name: String,
    span: SpanRef,
    visibility: Visibility,
    category: syft_types::SymbolCategory,
    tags: Vec<String>,
    attributes: std::collections::BTreeMap<String, serde_json::Value>,
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
