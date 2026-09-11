//! CowrieOracle — deterministic daily Odù tile selector.
//!
//! Implements a NIST-Beacon-inspired approach for picking the "active Odù tile"
//! for a given day + domain pair, using pure SHA-256 as the PRNG (no external
//! chacha20 dependency).
//!
//! Algorithm:
//!   seed  = sha256(day_number_le_bytes || domain_tag_bytes)   // 32-byte seed
//!   tile  = sha256(seed || 0u64_le)[0]                         // first byte of expanded output
//!   tile_index = tile % 256 (trivially u8, already 0..=255)
//!
//! The 256-step Odù grid uses the same tile_id format as `OduCoordinate`:
//!   "odu:{x:x}{y:x}" where x = tile_index >> 4, y = tile_index & 0xF

use sha2::{Sha256, Digest};
use serde::{Deserialize, Serialize};

// ─── Constants ────────────────────────────────────────────────────────────────

/// ≈ 10_000_000_000 µ-Àṣẹ / 365 days.
pub const BASE_DAILY_EMISSION: u64 = 27_397_260;

// ─── Public types ─────────────────────────────────────────────────────────────

/// The result of a CowrieOracle query for a specific day + domain.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OracleResult {
    /// Unix day number (unix_ts_secs / 86400).
    pub day: u32,
    /// Domain tag, e.g. "spatial" | "economy" | "temporal".
    pub domain: String,
    /// Chosen tile index in 0..=255.
    pub tile_index: u8,
    /// Canonical tile identifier: "odu:{x:x}{y:x}".
    pub tile_id: String,
    /// Hex-encoded 32-byte seed hash used for selection.
    pub seed_hash: String,
}

/// Daily emission record for the Àṣẹ token economy.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DailyEmission {
    /// Unix day number.
    pub day: u32,
    /// Canonical tile identifier for the active tile.
    pub active_tile_id: String,
    /// Index of the active tile (0..=255).
    pub active_tile_idx: u8,
    /// Emission cap in micro-Àṣẹ; BASE = 10_000_000_000 / 365.
    pub emission_cap: u64,
    /// Domain tag that was used to derive the tile.
    pub domain: String,
}

impl DailyEmission {
    /// Compute everything from `day` + `domain`.
    pub fn for_day(day: u32, domain: &str) -> Self {
        let result = CowrieOracle::query(day, domain);
        Self {
            day,
            active_tile_id: result.tile_id,
            active_tile_idx: result.tile_index,
            emission_cap: BASE_DAILY_EMISSION,
            domain: domain.to_owned(),
        }
    }
}

// ─── Oracle ───────────────────────────────────────────────────────────────────

/// Deterministic daily Odù tile oracle.
///
/// Uses SHA-256 as a pure PRNG — no external chacha20 dependency needed.
/// Results are fully reproducible given the same (day, domain) inputs.
pub struct CowrieOracle;

impl CowrieOracle {
    /// Query the oracle for a given day number and domain tag.
    pub fn query(day: u32, domain: &str) -> OracleResult {
        let seed = Self::derive_seed(day, domain);
        let tile_index = Self::expand_seed(&seed);
        let tile_id = tile_id_from_index(tile_index);
        let seed_hash = hex::encode(seed);

        OracleResult {
            day,
            domain: domain.to_owned(),
            tile_index,
            tile_id,
            seed_hash,
        }
    }

    /// Convenience: returns just the tile index for a (day, domain) pair.
    pub fn tile_index(day: u32, domain: &str) -> u8 {
        let seed = Self::derive_seed(day, domain);
        Self::expand_seed(&seed)
    }

    // ── Private helpers ────────────────────────────────────────────────────

    /// Derive 32-byte seed: sha256(day_le_bytes || domain_bytes).
    fn derive_seed(day: u32, domain: &str) -> [u8; 32] {
        let mut hasher = Sha256::new();
        hasher.update(day.to_le_bytes());
        hasher.update(domain.as_bytes());
        hasher.finalize().into()
    }

    /// Expand seed using sha256(seed || counter) as a stream cipher substitute.
    /// Returns the first byte of the first block (counter = 0u64 LE).
    fn expand_seed(seed: &[u8; 32]) -> u8 {
        let mut hasher = Sha256::new();
        hasher.update(seed);
        hasher.update(0u64.to_le_bytes()); // counter = 0
        let out: [u8; 32] = hasher.finalize().into();
        out[0]
    }
}

// ─── Helpers ──────────────────────────────────────────────────────────────────

/// Convert a tile index (0..=255) to the canonical tile_id string.
///
/// Format: "odu:{x:x}{y:x}" where x = index >> 4, y = index & 0xF.
/// This matches the `OduCoordinate` tile_id format from the odu module.
#[inline]
fn tile_id_from_index(index: u8) -> String {
    let x = index >> 4;
    let y = index & 0x0F;
    format!("odu:{:x}{:x}", x, y)
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── Determinism ──────────────────────────────────────────────────────────

    #[test]
    fn same_day_and_domain_always_returns_same_tile_id() {
        let day = 19_600u32; // arbitrary fixed day
        let domain = "spatial";
        let r1 = CowrieOracle::query(day, domain);
        let r2 = CowrieOracle::query(day, domain);
        assert_eq!(r1.tile_id, r2.tile_id);
        assert_eq!(r1.tile_index, r2.tile_index);
        assert_eq!(r1.seed_hash, r2.seed_hash);
    }

    // ── Consecutive days produce different tiles ──────────────────────────────

    #[test]
    fn seven_consecutive_days_produce_different_tile_ids() {
        let domain = "economy";
        let base_day = 20_000u32;
        let tile_ids: Vec<String> = (base_day..base_day + 7)
            .map(|d| CowrieOracle::query(d, domain).tile_id)
            .collect();

        // All 7 must differ from one another.
        for i in 0..tile_ids.len() {
            for j in (i + 1)..tile_ids.len() {
                assert_ne!(
                    tile_ids[i], tile_ids[j],
                    "days {} and {} produced the same tile_id: {}",
                    base_day + i as u32,
                    base_day + j as u32,
                    tile_ids[i]
                );
            }
        }
    }

    // ── Emission cap ─────────────────────────────────────────────────────────

    #[test]
    fn emission_cap_is_base_daily_emission() {
        let e = DailyEmission::for_day(19_600, "spatial");
        assert_eq!(e.emission_cap, BASE_DAILY_EMISSION);
        assert_eq!(e.emission_cap, 27_397_260u64);
    }

    // ── Tile ID format ────────────────────────────────────────────────────────

    #[test]
    fn tile_id_has_valid_odu_format() {
        // Check all 256 possible tile indices produce valid "odu:XY" strings.
        for idx in 0u8..=255 {
            let id = tile_id_from_index(idx);
            assert!(
                id.starts_with("odu:"),
                "tile_id '{}' for index {} does not start with 'odu:'", id, idx
            );
            let hex_part = id.strip_prefix("odu:").unwrap();
            assert_eq!(
                hex_part.len(), 2,
                "tile_id '{}' hex part should be 2 chars, got {}", id, hex_part.len()
            );
            // Each char must be a valid lowercase hex nibble.
            for ch in hex_part.chars() {
                assert!(
                    ch.is_ascii_hexdigit(),
                    "non-hex char '{}' in tile_id '{}'", ch, id
                );
            }
        }
    }

    #[test]
    fn oracle_result_tile_id_has_valid_format() {
        let r = CowrieOracle::query(19_600, "temporal");
        assert!(r.tile_id.starts_with("odu:"), "tile_id must start with 'odu:'");
        let hex_part = r.tile_id.strip_prefix("odu:").unwrap();
        assert_eq!(hex_part.len(), 2);
        for ch in hex_part.chars() {
            assert!(ch.is_ascii_hexdigit());
        }
    }

    // ── Different domains on the same day → different tiles ───────────────────

    #[test]
    fn spatial_and_economy_on_same_day_differ() {
        let day = 19_600u32;
        let spatial = CowrieOracle::query(day, "spatial");
        let economy = CowrieOracle::query(day, "economy");
        assert_ne!(
            spatial.tile_id, economy.tile_id,
            "domains 'spatial' and 'economy' should select different tiles on the same day"
        );
        // seed hashes must also differ
        assert_ne!(spatial.seed_hash, economy.seed_hash);
    }

    // ── DailyEmission round-trip consistency ─────────────────────────────────

    #[test]
    fn daily_emission_matches_oracle_query() {
        let day = 20_100u32;
        let domain = "temporal";
        let emission = DailyEmission::for_day(day, domain);
        let oracle = CowrieOracle::query(day, domain);

        assert_eq!(emission.day, day);
        assert_eq!(emission.domain, domain);
        assert_eq!(emission.active_tile_id, oracle.tile_id);
        assert_eq!(emission.active_tile_idx, oracle.tile_index);
    }

    // ── Tile index → tile_id inverse ─────────────────────────────────────────

    #[test]
    fn tile_id_from_index_boundary_values() {
        assert_eq!(tile_id_from_index(0x00), "odu:00");
        assert_eq!(tile_id_from_index(0xFF), "odu:ff");
        // 0xAB → x=0xA, y=0xB
        assert_eq!(tile_id_from_index(0xAB), "odu:ab");
        // 0x10 → x=1, y=0
        assert_eq!(tile_id_from_index(0x10), "odu:10");
    }

    // ── Seed hash is 64-char hex (32 bytes) ──────────────────────────────────

    #[test]
    fn seed_hash_is_64_char_hex() {
        let r = CowrieOracle::query(1, "spatial");
        assert_eq!(r.seed_hash.len(), 64);
        assert!(r.seed_hash.chars().all(|c| c.is_ascii_hexdigit()));
    }

    // ── BASE_DAILY_EMISSION constant ─────────────────────────────────────────

    #[test]
    fn base_daily_emission_value() {
        assert_eq!(BASE_DAILY_EMISSION, 27_397_260u64);
    }
}
