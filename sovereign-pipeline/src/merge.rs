// Phase 49 — Swarm Gaussian splatting aggregation.
// Merges N per-device ASCII PLY point clouds into one unified scene.
// Uses a voxel-grid filter to deduplicate overlapping points across devices.

use serde::{Deserialize, Serialize};

// ── types ─────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SplatInput {
    pub device_id: String,
    pub ply_b64:   String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MergeRequest {
    /// Per-device PLY blobs (base64-encoded ASCII PLY).
    pub splats:    Vec<SplatInput>,
    /// Voxel grid cell size in metres. 0 = no downsampling.
    #[serde(default = "default_voxel_size")]
    pub voxel_size: f64,
}

fn default_voxel_size() -> f64 { 0.05 }

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MergeResult {
    pub merged_ply_b64:  String,
    pub source_count:    usize,
    pub input_points:    usize,
    pub output_points:   usize,
    pub reduction_pct:   f64,
    pub centroid:        [f64; 3],
    pub bbox_min:        [f64; 3],
    pub bbox_max:        [f64; 3],
}

#[derive(Debug, thiserror::Error)]
pub enum MergeError {
    #[error("base64 decode error for device {device_id}: {detail}")]
    Base64 { device_id: String, detail: String },
    #[error("PLY parse error for device {device_id}: {detail}")]
    Ply { device_id: String, detail: String },
    #[error("no valid splats provided")]
    Empty,
}

// ── PLY I/O ───────────────────────────────────────────────────────────────────

/// Parse ASCII PLY → raw vertex list (x,y,z only; ignores other properties).
pub fn parse_ply_points(bytes: &[u8]) -> Result<Vec<[f64; 3]>, String> {
    let text = std::str::from_utf8(bytes).map_err(|e| e.to_string())?;
    let header_end = text.find("end_header").ok_or("missing end_header")?;
    let header = &text[..header_end];

    let vertex_count: usize = header
        .lines()
        .find(|l| l.starts_with("element vertex"))
        .and_then(|l| l.split_whitespace().nth(2))
        .and_then(|n| n.parse().ok())
        .unwrap_or(0);

    let data = &text[header_end + "end_header".len()..].trim_start_matches('\n');
    let mut pts = Vec::with_capacity(vertex_count.min(2_000_000));

    for line in data.lines().take(vertex_count) {
        let mut cols = line.split_ascii_whitespace();
        let x = cols.next().and_then(|v| v.parse::<f64>().ok());
        let y = cols.next().and_then(|v| v.parse::<f64>().ok());
        let z = cols.next().and_then(|v| v.parse::<f64>().ok());
        if let (Some(x), Some(y), Some(z)) = (x, y, z) {
            pts.push([x, y, z]);
        }
    }
    Ok(pts)
}

/// Serialise a point list back to ASCII PLY bytes.
pub fn write_ply(points: &[[f64; 3]]) -> Vec<u8> {
    let mut out = format!(
        "ply\nformat ascii 1.0\nelement vertex {}\nproperty float x\nproperty float y\nproperty float z\nend_header\n",
        points.len()
    );
    for p in points {
        out.push_str(&format!("{:.6} {:.6} {:.6}\n", p[0], p[1], p[2]));
    }
    out.into_bytes()
}

// ── voxel-grid downsampling ───────────────────────────────────────────────────

/// Keep one representative point per voxel cell (centroid of all points in cell).
/// cell_size ≤ 0 skips filtering entirely.
pub fn voxel_downsample(points: &[[f64; 3]], cell_size: f64) -> Vec<[f64; 3]> {
    if cell_size <= 0.0 || points.is_empty() {
        return points.to_vec();
    }

    use std::collections::HashMap;

    // Map each point to its voxel key (i64 triple).
    let mut cells: HashMap<(i64, i64, i64), ([f64; 3], usize)> = HashMap::new();
    for p in points {
        let key = (
            (p[0] / cell_size).floor() as i64,
            (p[1] / cell_size).floor() as i64,
            (p[2] / cell_size).floor() as i64,
        );
        let entry = cells.entry(key).or_insert(([0.0; 3], 0));
        entry.0[0] += p[0];
        entry.0[1] += p[1];
        entry.0[2] += p[2];
        entry.1 += 1;
    }

    cells.values().map(|(sum, n)| {
        let n = *n as f64;
        [sum[0] / n, sum[1] / n, sum[2] / n]
    }).collect()
}

// ── merge ─────────────────────────────────────────────────────────────────────

pub fn merge_splats(req: &MergeRequest) -> Result<MergeResult, MergeError> {
    use base64::{Engine as _, engine::general_purpose::STANDARD};

    if req.splats.is_empty() {
        return Err(MergeError::Empty);
    }

    let mut all_points: Vec<[f64; 3]> = Vec::new();

    for inp in &req.splats {
        let bytes = STANDARD.decode(&inp.ply_b64).map_err(|e| MergeError::Base64 {
            device_id: inp.device_id.clone(),
            detail:    e.to_string(),
        })?;
        let pts = parse_ply_points(&bytes).map_err(|e| MergeError::Ply {
            device_id: inp.device_id.clone(),
            detail:    e,
        })?;
        all_points.extend_from_slice(&pts);
    }

    let input_points = all_points.len();
    let merged = voxel_downsample(&all_points, req.voxel_size);
    let output_points = merged.len();

    let reduction_pct = if input_points > 0 {
        100.0 * (1.0 - output_points as f64 / input_points as f64)
    } else {
        0.0
    };

    // Stats on merged cloud.
    let (centroid, bbox_min, bbox_max) = if merged.is_empty() {
        ([0.0; 3], [0.0; 3], [0.0; 3])
    } else {
        let n = merged.len() as f64;
        let mut sum = [0.0f64; 3];
        let mut mn  = [f64::MAX; 3];
        let mut mx  = [f64::MIN; 3];
        for p in &merged {
            for i in 0..3 {
                sum[i] += p[i];
                if p[i] < mn[i] { mn[i] = p[i]; }
                if p[i] > mx[i] { mx[i] = p[i]; }
            }
        }
        ([sum[0]/n, sum[1]/n, sum[2]/n], mn, mx)
    };

    let ply_bytes = write_ply(&merged);
    let merged_ply_b64 = STANDARD.encode(&ply_bytes);

    Ok(MergeResult {
        merged_ply_b64,
        source_count: req.splats.len(),
        input_points,
        output_points,
        reduction_pct,
        centroid,
        bbox_min,
        bbox_max,
    })
}

// ── unit tests ────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use base64::{Engine as _, engine::general_purpose::STANDARD};

    fn b64_ply(points: &[[f64; 3]]) -> String {
        STANDARD.encode(write_ply(points))
    }

    fn inp(device_id: &str, pts: &[[f64; 3]]) -> SplatInput {
        SplatInput { device_id: device_id.into(), ply_b64: b64_ply(pts) }
    }

    #[test]
    fn merge_two_non_overlapping_clouds() {
        let req = MergeRequest {
            splats: vec![
                inp("dev:a", &[[0.0,0.0,0.0],[1.0,0.0,0.0]]),
                inp("dev:b", &[[5.0,0.0,0.0],[6.0,0.0,0.0]]),
            ],
            voxel_size: 0.0, // no downsampling
        };
        let r = merge_splats(&req).unwrap();
        assert_eq!(r.source_count, 2);
        assert_eq!(r.input_points, 4);
        assert_eq!(r.output_points, 4);
    }

    #[test]
    fn voxel_downsample_removes_duplicates() {
        // 4 points all in the same 1m voxel → should collapse to 1.
        let pts: Vec<[f64;3]> = vec![[0.1,0.1,0.1],[0.2,0.2,0.2],[0.3,0.3,0.3],[0.4,0.4,0.4]];
        let out = voxel_downsample(&pts, 1.0);
        assert_eq!(out.len(), 1);
        // Centroid should be average.
        assert!((out[0][0] - 0.25).abs() < 1e-9);
    }

    #[test]
    fn voxel_downsample_zero_passthrough() {
        let pts = vec![[0.0,0.0,0.0],[0.01,0.0,0.0]];
        let out = voxel_downsample(&pts, 0.0);
        assert_eq!(out.len(), 2);
    }

    #[test]
    fn merge_empty_returns_error() {
        let req = MergeRequest { splats: vec![], voxel_size: 0.05 };
        assert!(matches!(merge_splats(&req), Err(MergeError::Empty)));
    }

    #[test]
    fn merged_ply_roundtrip() {
        let pts = vec![[1.0,2.0,3.0],[4.0,5.0,6.0]];
        let bytes = write_ply(&pts);
        let parsed = parse_ply_points(&bytes).unwrap();
        assert_eq!(parsed.len(), 2);
        assert!((parsed[0][0] - 1.0).abs() < 1e-4);
        assert!((parsed[1][2] - 6.0).abs() < 1e-4);
    }

    #[test]
    fn merge_reduction_pct_correct() {
        // 4 colocated points in 1m voxel → 1 out → 75% reduction.
        let req = MergeRequest {
            splats: vec![
                inp("dev:a", &[[0.1,0.1,0.1],[0.2,0.2,0.2]]),
                inp("dev:b", &[[0.3,0.3,0.3],[0.4,0.4,0.4]]),
            ],
            voxel_size: 1.0,
        };
        let r = merge_splats(&req).unwrap();
        assert_eq!(r.input_points, 4);
        assert_eq!(r.output_points, 1);
        assert!((r.reduction_pct - 75.0).abs() < 1e-6);
    }

    #[test]
    fn merge_stats_centroid_correct() {
        let req = MergeRequest {
            splats: vec![
                inp("dev:a", &[[0.0,0.0,0.0]]),
                inp("dev:b", &[[2.0,0.0,0.0]]),
            ],
            voxel_size: 0.0,
        };
        let r = merge_splats(&req).unwrap();
        assert!((r.centroid[0] - 1.0).abs() < 1e-9, "centroid x should be 1.0, got {}", r.centroid[0]);
    }

    #[test]
    fn bad_base64_returns_error() {
        let req = MergeRequest {
            splats: vec![SplatInput { device_id: "dev:bad".into(), ply_b64: "!!!bad!!!".into() }],
            voxel_size: 0.05,
        };
        assert!(matches!(merge_splats(&req), Err(MergeError::Base64 { .. })));
    }
}
