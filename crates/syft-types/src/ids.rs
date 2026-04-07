use blake3::Hasher;
use chrono::{DateTime, Utc};
use ulid::Ulid;

pub type EntityId = String;
pub type ObjectHash = String;

pub fn new_entity_id() -> EntityId {
    Ulid::new().to_string()
}

pub fn hash_bytes(bytes: &[u8]) -> ObjectHash {
    let mut hasher = Hasher::new();
    hasher.update(bytes);
    hasher.finalize().to_hex().to_string()
}

pub fn now_utc() -> DateTime<Utc> {
    Utc::now()
}
