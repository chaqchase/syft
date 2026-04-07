mod change;
mod ids;
mod query;
mod repo;
mod semantic;
mod task;

pub use change::*;
pub use ids::*;
pub use query::*;
pub use repo::*;
pub use semantic::*;
pub use task::*;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn entity_ids_are_non_empty() {
        let id = new_entity_id();
        assert!(!id.is_empty());
    }

    #[test]
    fn hashing_is_stable() {
        let first = hash_bytes(b"hello");
        let second = hash_bytes(b"hello");
        assert_eq!(first, second);
    }

    #[test]
    fn semantic_delta_defaults_are_empty() {
        let delta = SemanticDelta::default();
        assert!(delta.touched_symbols.is_empty());
        assert!(delta.changed_files.is_empty());
    }
}
