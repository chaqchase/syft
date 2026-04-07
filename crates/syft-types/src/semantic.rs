use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::EntityId;

#[derive(Debug, Clone, Serialize, Deserialize, Hash, Eq, PartialEq)]
pub enum Language {
    Rust,
    TypeScript,
    Python,
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize, Hash, Eq, PartialEq)]
pub enum Visibility {
    Public,
    Internal,
    Private,
}

#[derive(Debug, Clone, Serialize, Deserialize, Hash, Eq, PartialEq)]
pub enum SymbolCategory {
    Type,
    Callable,
    Namespace,
    Value,
    Member,
    Macro,
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize, Hash, Eq, PartialEq)]
pub struct SymbolId {
    pub language: Language,
    pub namespace: String,
    pub path: String,
    pub local_name: String,
    pub disambiguator: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SpanRef {
    pub start_line: u32,
    pub start_col: u32,
    pub end_line: u32,
    pub end_col: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SymbolSource {
    pub file_path: String,
    pub span: SpanRef,
    pub visibility: Visibility,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SymbolRef {
    pub id: SymbolId,
    pub display_name: String,
    pub source: SymbolSource,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SymbolDescriptor {
    pub symbol: SymbolRef,
    pub category: SymbolCategory,
    pub tags: Vec<String>,
    pub attributes: BTreeMap<String, serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SemanticEdge {
    pub from: SymbolId,
    pub to: SymbolTarget,
    pub kind: EdgeKind,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SymbolTarget {
    Symbol(SymbolId),
    External(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum EdgeKind {
    Contains,
    References,
    Implements,
    DependsOn,
    Exports,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SemanticDelta {
    pub touched_symbols: Vec<SymbolRef>,
    pub added_symbols: Vec<SymbolRef>,
    pub removed_symbols: Vec<SymbolRef>,
    pub changed_public_api: bool,
    pub changed_dependencies: Vec<DependencyEdgeChange>,
    pub changed_files: Vec<String>,
    pub summary: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DependencyEdgeChange {
    pub from: EntityId,
    pub to: EntityId,
    pub kind: DependencyEdgeChangeKind,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum DependencyEdgeChangeKind {
    Added,
    Removed,
}
