// Phase 48 — 3D spatial diff between two PLY point-cloud snapshots.
// Parses ASCII PLY vertex data, computes centroid/bbox/density, returns delta.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlyStats {
    pub point_count: usize,
    pub centroid:    [f64; 3],
    pub bbox_min:    [f64; 3],
    pub bbox_max:    [f64; 3],
}

impl PlyStats {
    pub fn volume(&self) -> f64 {
        let dx = (self.bbox_max[0] - self.bbox_min[0]).max(0.0);
        let dy = (self.bbox_max[1] - self.bbox_min[1]).max(0.0);
        let dz = (self.bbox_max[2] - self.bbox_min[2]).max(0.0);
        dx * dy * dz
    }

    pub fn density(&self) -> f64 {
        let v = self.volume();
        if v < 1e-9 { 0.0 } else { self.point_count as f64 / v }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpatialDiff {
    pub snapshot_a:        String,
    pub snapshot_b:        String,
    pub stats_a:           PlyStats,
    pub stats_b:           PlyStats,
    pub point_count_delta: i64,
    pub centroid_delta_m:  f64,
    pub volume_delta_m3:   f64,
    pub density_delta:     f64,
    pub change_magnitude:  f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SplatDiffRequest {
    pub snapshot_a: String,
    pub snapshot_b: String,
}

#[derive(Debug, thiserror::Error)]
pub enum DiffError {
    #[error("PLY parse error: {0}")]
    Parse(String),
    #[error("snapshot not found: {0}")]
    NotFound(String),
}

/// Parse minimal ASCII or binary PLY: read vertex count from header, then
/// consume x y z floats from each vertex line. Binary PLY yields zero points
/// (callers treat it as an opaque blob with known point_count from header).
pub fn parse_ply_stats(id: &str, ply_bytes: &[u8]) -> Result<PlyStats, DiffError> {
    let text = std::str::from_utf8(ply_bytes)
        .map_err(|e| DiffError::Parse(e.to_string()))?;

    // --- header parsing ---
    let header_end = text
        .find("end_header")
        .ok_or_else(|| DiffError::Parse("missing end_header".into()))?;
    let header = &text[..header_end];

    let vertex_count: usize = header
        .lines()
        .find(|l| l.starts_with("element vertex"))
        .and_then(|l| l.split_whitespace().nth(2))
        .and_then(|n| n.parse().ok())
        .unwrap_or(0);

    // --- data parsing (ASCII only) ---
    let data = &text[header_end + "end_header".len()..].trim_start_matches('\n');

    let mut points: Vec<[f64; 3]> = Vec::with_capacity(vertex_count.min(1_000_000));
    for line in data.lines().take(vertex_count) {
        let mut cols = line.split_ascii_whitespace();
        let x = cols.next().and_then(|v| v.parse::<f64>().ok());
        let y = cols.next().and_then(|v| v.parse::<f64>().ok());
        let z = cols.next().and_then(|v| v.parse::<f64>().ok());
        if let (Some(x), Some(y), Some(z)) = (x, y, z) {
            points.push([x, y, z]);
        }
    }

    if points.is_empty() {
        // Binary PLY or empty cloud — return header count, zero centroid/bbox.
        return Ok(PlyStats {
            point_count: vertex_count,
            centroid:    [0.0; 3],
            bbox_min:    [0.0; 3],
            bbox_max:    [0.0; 3],
        });
    }

    let n = points.len() as f64;
    let mut sum = [0.0f64; 3];
    let mut mn  = [f64::MAX; 3];
    let mut mx  = [f64::MIN; 3];

    for p in &points {
        for i in 0..3 {
            sum[i] += p[i];
            if p[i] < mn[i] { mn[i] = p[i]; }
            if p[i] > mx[i] { mx[i] = p[i]; }
        }
    }

    let _ = id; // used for error context only
    Ok(PlyStats {
        point_count: points.len(),
        centroid:    [sum[0] / n, sum[1] / n, sum[2] / n],
        bbox_min:    mn,
        bbox_max:    mx,
    })
}

pub fn diff_stats(id_a: &str, id_b: &str, a: PlyStats, b: PlyStats) -> SpatialDiff {
    let point_count_delta = b.point_count as i64 - a.point_count as i64;

    let centroid_delta_m = {
        let dx = b.centroid[0] - a.centroid[0];
        let dy = b.centroid[1] - a.centroid[1];
        let dz = b.centroid[2] - a.centroid[2];
        (dx * dx + dy * dy + dz * dz).sqrt()
    };

    let volume_delta_m3 = b.volume() - a.volume();
    let density_delta   = b.density() - a.density();

    // Normalised 0-1 magnitude combining all axes.
    let count_norm = if a.point_count > 0 {
        (point_count_delta.unsigned_abs() as f64) / (a.point_count as f64)
    } else {
        1.0
    };
    let change_magnitude = (count_norm * 0.4 + centroid_delta_m.tanh() * 0.4
        + volume_delta_m3.abs().tanh() * 0.2)
        .clamp(0.0, 1.0);

    SpatialDiff {
        snapshot_a:        id_a.to_owned(),
        snapshot_b:        id_b.to_owned(),
        stats_a:           a,
        stats_b:           b,
        point_count_delta,
        centroid_delta_m,
        volume_delta_m3,
        density_delta,
        change_magnitude,
    }
}

// ── unit tests ────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn make_ply(points: &[[f64; 3]]) -> Vec<u8> {
        let mut s = format!(
            "ply\nformat ascii 1.0\nelement vertex {}\nproperty float x\nproperty float y\nproperty float z\nend_header\n",
            points.len()
        );
        for p in points {
            s.push_str(&format!("{} {} {}\n", p[0], p[1], p[2]));
        }
        s.into_bytes()
    }

    #[test]
    fn parse_single_point() {
        let ply = make_ply(&[[1.0, 2.0, 3.0]]);
        let s = parse_ply_stats("t", &ply).unwrap();
        assert_eq!(s.point_count, 1);
        assert!((s.centroid[0] - 1.0).abs() < 1e-9);
        assert!((s.centroid[1] - 2.0).abs() < 1e-9);
        assert!((s.centroid[2] - 3.0).abs() < 1e-9);
    }

    #[test]
    fn parse_four_points_centroid() {
        let pts = [[0.0,0.0,0.0],[2.0,0.0,0.0],[2.0,2.0,0.0],[0.0,2.0,0.0]];
        let ply = make_ply(&pts);
        let s = parse_ply_stats("t", &ply).unwrap();
        assert_eq!(s.point_count, 4);
        assert!((s.centroid[0] - 1.0).abs() < 1e-9);
        assert!((s.centroid[1] - 1.0).abs() < 1e-9);
    }

    #[test]
    fn diff_identical_clouds_zero_delta() {
        let pts = [[0.0,0.0,0.0],[1.0,1.0,1.0]];
        let ply = make_ply(&pts);
        let sa = parse_ply_stats("a", &ply).unwrap();
        let sb = parse_ply_stats("b", &ply).unwrap();
        let d = diff_stats("a", "b", sa, sb);
        assert_eq!(d.point_count_delta, 0);
        assert!(d.centroid_delta_m < 1e-9);
        assert!(d.change_magnitude < 1e-9);
    }

    #[test]
    fn diff_shifted_cloud_detects_centroid_change() {
        let a_pts = [[0.0,0.0,0.0],[1.0,0.0,0.0]];
        let b_pts = [[10.0,0.0,0.0],[11.0,0.0,0.0]];
        let sa = parse_ply_stats("a", &make_ply(&a_pts)).unwrap();
        let sb = parse_ply_stats("b", &make_ply(&b_pts)).unwrap();
        let d = diff_stats("a", "b", sa, sb);
        assert!((d.centroid_delta_m - 10.0).abs() < 1e-6);
        assert!(d.change_magnitude > 0.0);
    }

    #[test]
    fn diff_growing_cloud_positive_count_delta() {
        let sa = parse_ply_stats("a", &make_ply(&[[0.0,0.0,0.0]])).unwrap();
        let sb = parse_ply_stats("b", &make_ply(&[[0.0,0.0,0.0],[1.0,0.0,0.0],[2.0,0.0,0.0]])).unwrap();
        let d = diff_stats("a", "b", sa, sb);
        assert_eq!(d.point_count_delta, 2);
    }

    #[test]
    fn bbox_volume_correct() {
        let pts = [[0.0,0.0,0.0],[2.0,3.0,4.0]];
        let s = parse_ply_stats("t", &make_ply(&pts)).unwrap();
        assert!((s.volume() - 24.0).abs() < 1e-9);
    }

    #[test]
    fn missing_end_header_returns_error() {
        let bad = b"ply\nformat ascii 1.0\nelement vertex 1\n";
        assert!(parse_ply_stats("t", bad).is_err());
    }
}
