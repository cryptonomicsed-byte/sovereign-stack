use sha2::{Sha256, Digest};
use std::collections::BTreeMap;
use serde_json::Value;

/// Compute a merkle root over a set of named fields.
/// Field order is alphabetical (BTreeMap). Each leaf = sha256("key:value").
/// Pairs are combined up the tree: sha256(left || right).
pub fn merkle_root(fields: &BTreeMap<&str, Value>) -> String {
    if fields.is_empty() {
        return format!("sha256:{}", hex::encode(Sha256::digest(b"")));
    }

    let leaves: Vec<[u8; 32]> = fields.iter().map(|(k, v)| {
        let leaf = format!("{}:{}", k, v);
        let hash = Sha256::digest(leaf.as_bytes());
        hash.into()
    }).collect();

    let root = combine_leaves(&leaves);
    format!("sha256:{}", hex::encode(root))
}

fn combine_leaves(leaves: &[[u8; 32]]) -> [u8; 32] {
    if leaves.len() == 1 {
        return leaves[0];
    }
    let mid = leaves.len().next_power_of_two() / 2;
    let left  = combine_leaves(&leaves[..mid.min(leaves.len())]);
    let right = if mid < leaves.len() {
        combine_leaves(&leaves[mid..])
    } else {
        left // pad with self if odd
    };
    let mut hasher = Sha256::new();
    hasher.update(left);
    hasher.update(right);
    hasher.finalize().into()
}

/// Hash bytes to sha256:<hex> string.
pub fn hash_bytes(data: &[u8]) -> String {
    format!("sha256:{}", hex::encode(Sha256::digest(data)))
}

/// Hash a canonical JSON value.
pub fn hash_json(value: &Value) -> String {
    hash_bytes(value.to_string().as_bytes())
}

/// Hash a string.
pub fn hash_str(s: &str) -> String {
    hash_bytes(s.as_bytes())
}

/// Verify a stored hash matches recomputed hash.
pub fn verify_hash(stored: &str, data: &[u8]) -> bool {
    stored == hash_bytes(data)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn deterministic() {
        let mut fields = BTreeMap::new();
        fields.insert("a", json!("hello"));
        fields.insert("b", json!(42));
        let r1 = merkle_root(&fields);
        let r2 = merkle_root(&fields);
        assert_eq!(r1, r2);
        assert!(r1.starts_with("sha256:"));
    }

    #[test]
    fn different_data_different_root() {
        let mut f1 = BTreeMap::new();
        f1.insert("a", json!("hello"));
        let mut f2 = BTreeMap::new();
        f2.insert("a", json!("world"));
        assert_ne!(merkle_root(&f1), merkle_root(&f2));
    }
}
