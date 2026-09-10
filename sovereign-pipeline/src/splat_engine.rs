//! Gaussian Splatting / Nerfstudio reconstruction bridge.
//!
//! Converts raw RGB frames from a VCP capture into a .ply Gaussian splat
//! by invoking the nerfstudio or gsplat CLI as a subprocess.
//!
//! Supported methods:
//!   "nerfacto"           — ns-train nerfacto (photorealistic NeRF)
//!   "gaussian-splatting" — ns-train gaussian-splatting (real-time 3DGS)
//!   "gsplat"             — standalone gsplat train.py (alternative)
//!
//! Call flow:
//!   1. Write JPEG frames to a temp directory
//!   2. Run ns-train (or gsplat train.py) — blocks until done
//!   3. Run ns-export gaussian-splat to produce splat.ply
//!   4. Read and return the .ply bytes
//!
//! Falls back to None if the binary is not found or training fails.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::fs;
use tracing::{info, warn};

/// Configuration for the splat reconstruction engine.
#[derive(Debug, Clone)]
pub struct SplatConfig {
    /// Binary name or full path. "ns-train" for nerfstudio on PATH.
    pub bin:         String,
    /// Root output directory; twin-specific subdirs are created beneath it.
    pub output_dir:  PathBuf,
    /// Training iterations.
    pub steps:       u32,
    /// "nerfacto" | "gaussian-splatting" | "gsplat"
    pub method:      String,
}

impl Default for SplatConfig {
    fn default() -> Self {
        Self {
            bin:        "ns-train".into(),
            output_dir: PathBuf::from("/tmp/sovereign-splats"),
            steps:      1000,
            method:     "gaussian-splatting".into(),
        }
    }
}

/// Run the Gaussian Splatting pipeline from raw frame bytes.
///
/// `frames_bytes` — raw bytes containing one or more JPEG frames.
/// `frame_count`  — number of frames captured (used for logging).
/// `twin_id`      — used to name the output subdirectory.
/// `cfg`          — splat engine configuration.
///
/// Returns `Ok(ply_bytes)` on success, `Err(msg)` if training fails
/// (caller should fall back to stub PLY bytes).
pub fn run_splat(
    frames_bytes: &[u8],
    frame_count:  u32,
    twin_id:      &str,
    cfg:          &SplatConfig,
) -> Result<Vec<u8>, String> {
    let safe_id   = twin_id.replace([':', '/'], "_");
    let work_dir  = cfg.output_dir.join(&safe_id);
    let frames_dir = work_dir.join("images");
    let train_dir  = work_dir.join("train");
    let export_dir = work_dir.join("export");

    fs::create_dir_all(&frames_dir).map_err(|e| format!("mkdir frames: {e}"))?;
    fs::create_dir_all(&train_dir).map_err(|e| format!("mkdir train: {e}"))?;
    fs::create_dir_all(&export_dir).map_err(|e| format!("mkdir export: {e}"))?;

    // Split frames_bytes into individual JPEG files
    let written = write_jpeg_frames(frames_bytes, &frames_dir)?;
    info!(twin_id = %twin_id, frames = written, "frames written for splat training");

    if written == 0 {
        return Err("no frames to reconstruct".into());
    }

    // Run reconstruction
    match cfg.method.as_str() {
        "gsplat" => run_gsplat(&cfg.bin, &frames_dir, &train_dir, cfg.steps)?,
        _        => run_nerfstudio(&cfg.bin, &cfg.method, &frames_dir, &train_dir, cfg.steps)?,
    }

    // Export to PLY
    let ply_path = export_to_ply(&train_dir, &export_dir, &cfg.method)?;

    let bytes = fs::read(&ply_path)
        .map_err(|e| format!("read PLY {}: {e}", ply_path.display()))?;

    info!(
        twin_id = %twin_id,
        ply_bytes = bytes.len(),
        "Gaussian splat reconstruction complete"
    );
    Ok(bytes)
}

/// Split a raw byte buffer into JPEG files by scanning for SOI (FFD8) markers.
/// Falls back to writing the entire buffer as a single file if no JPEGs found.
fn write_jpeg_frames(data: &[u8], out_dir: &Path) -> Result<usize, String> {
    const SOI: [u8; 2] = [0xFF, 0xD8]; // JPEG Start of Image
    const EOI: [u8; 2] = [0xFF, 0xD9]; // JPEG End of Image

    // Collect SOI positions
    let soi_positions: Vec<usize> = data.windows(2)
        .enumerate()
        .filter(|(_, w)| w == &SOI)
        .map(|(i, _)| i)
        .collect();

    if soi_positions.is_empty() {
        // Not JPEG — write as single raw frame
        let path = out_dir.join("frame_00000.bin");
        fs::write(&path, data).map_err(|e| format!("write frame: {e}"))?;
        return Ok(1);
    }

    let mut written = 0;
    for (idx, &start) in soi_positions.iter().enumerate() {
        let end = if idx + 1 < soi_positions.len() {
            soi_positions[idx + 1]
        } else {
            // Find EOI from start
            data[start..].windows(2)
                .position(|w| w == EOI)
                .map(|p| start + p + 2)
                .unwrap_or(data.len())
        };

        let frame = &data[start..end];
        let path  = out_dir.join(format!("frame_{idx:05}.jpg"));
        fs::write(&path, frame).map_err(|e| format!("write frame {idx}: {e}"))?;
        written += 1;
    }

    Ok(written)
}

/// Invoke ns-train for nerfstudio methods.
fn run_nerfstudio(
    bin:        &str,
    method:     &str,
    frames_dir: &Path,
    train_dir:  &Path,
    steps:      u32,
) -> Result<(), String> {
    info!(method = %method, steps = steps, "starting nerfstudio training");

    let status = Command::new(bin)
        .args([
            method,
            "--data",                   &frames_dir.to_string_lossy(),
            "--output-dir",             &train_dir.to_string_lossy(),
            "--max-num-iterations",     &steps.to_string(),
            "--vis",                    "none",
            "--pipeline.model.eval-num-rays-per-chunk", "1024",
        ])
        .status()
        .map_err(|e| format!("ns-train exec failed: {e}"))?;

    if !status.success() {
        return Err(format!("ns-train exited with {status}"));
    }
    Ok(())
}

/// Invoke gsplat train.py.
fn run_gsplat(
    bin:        &str,
    frames_dir: &Path,
    train_dir:  &Path,
    steps:      u32,
) -> Result<(), String> {
    info!(steps = steps, "starting gsplat training");

    let status = Command::new("python3")
        .args([
            bin,
            "-s", &frames_dir.to_string_lossy(),
            "-m", &train_dir.to_string_lossy(),
            "--iterations", &steps.to_string(),
        ])
        .status()
        .map_err(|e| format!("gsplat exec failed: {e}"))?;

    if !status.success() {
        return Err(format!("gsplat exited with {status}"));
    }
    Ok(())
}

/// Export the trained model to a .ply Gaussian splat via ns-export.
/// Returns the path to the output PLY file.
fn export_to_ply(
    train_dir:  &Path,
    export_dir: &Path,
    method:     &str,
) -> Result<PathBuf, String> {
    // For gsplat the PLY is produced directly during training
    if method == "gsplat" {
        let ply = find_ply_in_dir(train_dir);
        return ply.ok_or_else(|| format!("no .ply found in {}", train_dir.display()));
    }

    // For nerfstudio: ns-export gaussian-splat --load-config {config.yml}
    let config_yml = find_config_yml(train_dir)
        .ok_or_else(|| format!("no config.yml found in {}", train_dir.display()))?;

    let ply_out = export_dir.join("splat.ply");

    let status = Command::new("ns-export")
        .args([
            "gaussian-splat",
            "--load-config", &config_yml.to_string_lossy(),
            "--output-dir",  &export_dir.to_string_lossy(),
        ])
        .status()
        .map_err(|e| format!("ns-export exec failed: {e}"))?;

    if !status.success() {
        return Err(format!("ns-export exited with {status}"));
    }

    // ns-export may produce splat.ply directly or in a subdirectory
    if ply_out.exists() {
        return Ok(ply_out);
    }

    find_ply_in_dir(export_dir)
        .ok_or_else(|| format!("ns-export succeeded but no .ply in {}", export_dir.display()))
}

/// Recursively find the first .ply file in a directory.
fn find_ply_in_dir(dir: &Path) -> Option<PathBuf> {
    for entry in fs::read_dir(dir).ok()?.flatten() {
        let path = entry.path();
        if path.is_dir() {
            if let Some(p) = find_ply_in_dir(&path) {
                return Some(p);
            }
        } else if path.extension().and_then(|e| e.to_str()) == Some("ply") {
            return Some(path);
        }
    }
    None
}

/// Find the config.yml produced by ns-train (nested in versioned subdirs).
fn find_config_yml(dir: &Path) -> Option<PathBuf> {
    for entry in fs::read_dir(dir).ok()?.flatten() {
        let path = entry.path();
        if path.is_dir() {
            if let Some(p) = find_config_yml(&path) {
                return Some(p);
            }
        } else if path.file_name().and_then(|n| n.to_str()) == Some("config.yml") {
            return Some(path);
        }
    }
    None
}
