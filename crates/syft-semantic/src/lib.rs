use syft_types::{SemanticEdge, SymbolDescriptor};

mod diff;
mod extract;
mod support;

#[cfg(test)]
mod tests;

pub use diff::{diff_snapshots, index_snapshot};
pub use extract::{extract_rust_symbols, index_rust_directory};

#[derive(Debug, Clone)]
pub struct SemanticIndexResult {
    pub symbols: Vec<SymbolDescriptor>,
    pub public_api_symbols: Vec<SymbolDescriptor>,
    pub edges: Vec<SemanticEdge>,
}
