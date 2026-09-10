//! Rolling Merkle tree over all completed receipts.
//!
//! Each receipt_id is a leaf node (sha256 of the receipt_id string).
//! Leaves are sorted lexicographically before tree construction for determinism.
//!
//! Exposed via: GET /receipts/root

use sha2::{Sha256, Digest};
use serde::{Deserialize, Serialize};

use crate::receipt_store::ReceiptRecord;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReceiptMerkleRoot {
    /// sha256:<hex> of the Merkle root, or sha256:<hex of "empty"> when no receipts.
    pub root:        String,
    /// Number of receipts included in this tree.
    pub count:       usize,
    /// Unix milliseconds when this root was computed.
    pub computed_at: u64,
    /// Algorithm used.
    pub algo:        &'static str,
}

/// Compute a Merkle root over all `records`.
/// Leaves are sorted by receipt_id for determinism.
pub fn build_receipt_tree(records: &[ReceiptRecord]) -> ReceiptMerkleRoot {
    let mut ids: Vec<&str> = records.iter().map(|r| r.receipt_id.as_str()).collect();
    ids.sort_unstable();

    let root = if ids.is_empty() {
        let h: [u8; 32] = Sha256::digest(b"empty").into();
        format!("sha256:{}", hex::encode(h))
    } else {
        let leaves: Vec<[u8; 32]> = ids.iter()
            .map(|id| {
                let h: [u8; 32] = Sha256::digest(id.as_bytes()).into();
                h
            })
            .collect();
        let root = combine(&leaves);
        format!("sha256:{}", hex::encode(root))
    };

    ReceiptMerkleRoot {
        root,
        count:       records.len(),
        computed_at: now_ms(),
        algo:        "sha256-binary-merkle",
    }
}

/// Verify that a single receipt_id is consistent with a stored root
/// (linear scan — for production use a Merkle proof).
pub fn verify_receipt_in_root(
    receipt_id: &str,
    all_records: &[ReceiptRecord],
    expected_root: &str,
) -> bool {
    let current = build_receipt_tree(all_records);
    if current.root != expected_root {
        return false;
    }
    all_records.iter().any(|r| r.receipt_id == receipt_id)
}

fn combine(leaves: &[[u8; 32]]) -> [u8; 32] {
    if leaves.len() == 1 {
        return leaves[0];
    }
    let mid = leaves.len().next_power_of_two() / 2;
    let left  = combine(&leaves[..mid.min(leaves.len())]);
    let right = if mid < leaves.len() {
        combine(&leaves[mid..])
    } else {
        left
    };
    let mut h = Sha256::new();
    h.update(left);
    h.update(right);
    h.finalize().into()
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rec(id: &str) -> ReceiptRecord {
        ReceiptRecord {
            kind: 31030, receipt_id: id.into(), twin_id: "t".into(),
            device_id: "d".into(), scene_receipt_id: id.into(),
            capture_receipt_id: "c".into(), sui_object_id: None,
            dip_message_count: 0, completed_at: 0, odu_tile: None,
        }
    }

    #[test]
    fn empty_tree_has_stable_root() {
        let r1 = build_receipt_tree(&[]);
        let r2 = build_receipt_tree(&[]);
        assert_eq!(r1.root, r2.root);
        assert_eq!(r1.count, 0);
        assert!(r1.root.starts_with("sha256:"));
    }

    #[test]
    fn single_leaf_deterministic() {
        let r = build_receipt_tree(&[rec("abc")]);
        let r2 = build_receipt_tree(&[rec("abc")]);
        assert_eq!(r.root, r2.root);
        assert_eq!(r.count, 1);
    }

    #[test]
    fn order_independent() {
        let a = build_receipt_tree(&[rec("b"), rec("a")]);
        let b = build_receipt_tree(&[rec("a"), rec("b")]);
        assert_eq!(a.root, b.root, "sorted leaves must produce same root");
    }

    #[test]
    fn different_receipts_different_root() {
        let a = build_receipt_tree(&[rec("aaa")]);
        let b = build_receipt_tree(&[rec("bbb")]);
        assert_ne!(a.root, b.root);
    }

    #[test]
    fn verify_present_receipt() {
        let records = vec![rec("r1"), rec("r2"), rec("r3")];
        let root = build_receipt_tree(&records).root;
        assert!(verify_receipt_in_root("r1", &records, &root));
        assert!(!verify_receipt_in_root("r4", &records, &root));
    }

    #[test]
    fn verify_rejects_tampered_root() {
        let records = vec![rec("r1")];
        assert!(!verify_receipt_in_root("r1", &records, "sha256:deadbeef"));
    }
}
