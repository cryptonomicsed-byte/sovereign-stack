//! Go2 WebSocket capture driver — real connection to a Unitree Go2 robot.
//!
//! The Go2 exposes a WebSocket at ws://{host}:{port}/ (default 192.168.123.161:8080).
//!
//! Protocol (Unitree SDK WebSocket v2):
//!   Subscribe: {"type":"subscribe","topic":"/camera/frame"}
//!   Subscribe: {"type":"subscribe","topic":"/lidar/cloud"}
//!   Subscribe: {"type":"subscribe","topic":"/state"}
//!   Receive:   {"type":"message","topic":"…","data":{…}}
//!
//! Capture flow:
//!   1. Connect → subscribe to camera + lidar + state
//!   2. Collect frames for `capture_duration_secs` seconds
//!   3. Disconnect
//!   4. Return RawCapture with real byte payloads
//!
//! Called via tokio::task::spawn_blocking — uses sync WebSocket (tungstenite).
//!
//! If the robot is unreachable or returns errors, falls back to the stub
//! RawCapture so the rest of the pipeline can still run in dev mode.

use std::time::{Duration, Instant};

use serde_json::{json, Value};
use tungstenite::{connect, Message, stream::MaybeTlsStream};
use tracing::{debug, info, warn};

use crate::capture_pipeline::RawCapture;

/// Configuration for a real Go2 WebSocket capture session.
#[derive(Debug, Clone)]
pub struct Go2CaptureConfig {
    pub host:                 String,
    pub port:                 u16,
    /// How long to collect frames (seconds).
    pub capture_duration_secs: u64,
    /// Maximum frames to collect (prevents unbounded memory).
    pub max_rgb_frames:       usize,
}

impl Default for Go2CaptureConfig {
    fn default() -> Self {
        Self {
            host:                  "192.168.123.161".into(),
            port:                  8080,
            capture_duration_secs: 30,
            max_rgb_frames:        300,
        }
    }
}

impl Go2CaptureConfig {
    pub fn from_device_id(device_id: &str) -> Self {
        // device_id format: "unitree:go2:{host}" or "unitree:go2:{host}:{port}"
        let parts: Vec<&str> = device_id.split(':').collect();
        let mut cfg = Self::default();
        if parts.len() >= 3 {
            cfg.host = parts[2].into();
        }
        if parts.len() >= 4 {
            cfg.port = parts[3].parse().unwrap_or(8080);
        }
        cfg
    }
}

/// Attempt a real WebSocket capture from the Go2.
///
/// Returns `Ok(RawCapture)` if successful, `Err(String)` if unreachable
/// (caller should fall back to stub).
pub fn capture_from_go2(cfg: &Go2CaptureConfig) -> Result<RawCapture, String> {
    let url = format!("ws://{}:{}/", cfg.host, cfg.port);
    info!(url = %url, "connecting to Go2 WebSocket");

    let (mut ws, _) = connect(&url)
        .map_err(|e| format!("Go2 WebSocket connect failed: {e}"))?;

    info!(url = %url, "Go2 WebSocket connected");

    // Subscribe to sensor topics
    for topic in ["/camera/frame", "/lidar/cloud", "/state"] {
        let sub = json!({ "type": "subscribe", "topic": topic });
        ws.send(Message::Text(sub.to_string()))
            .map_err(|e| format!("subscribe to {topic}: {e}"))?;
        debug!(topic = %topic, "subscribed");
    }

    let deadline  = Instant::now() + Duration::from_secs(cfg.capture_duration_secs);
    let mut rgb_frames:  Vec<Vec<u8>> = vec![];
    let mut lidar_cloud: Vec<u8>      = vec![];
    let mut imu_samples: Vec<u8>      = vec![];
    let mut frame_count: u32          = 0;

    // Set a short read timeout so we can check the deadline
    match ws.get_mut() {
        MaybeTlsStream::Plain(tcp) => { tcp.set_read_timeout(Some(Duration::from_millis(200))).ok(); }
        _ => {}
    }

    while Instant::now() < deadline {
        match ws.read() {
            Ok(Message::Binary(data)) => {
                // Binary frame (camera)
                if rgb_frames.len() < cfg.max_rgb_frames {
                    rgb_frames.push(data);
                    frame_count += 1;
                }
            }
            Ok(Message::Text(text)) => {
                if let Ok(msg) = serde_json::from_str::<Value>(&text) {
                    match msg.get("topic").and_then(|v| v.as_str()) {
                        Some("/camera/frame") => {
                            // JSON-encoded base64 frame
                            if let Some(b64) = msg["data"]["frame"].as_str() {
                                if let Ok(bytes) = base64_decode(b64) {
                                    if rgb_frames.len() < cfg.max_rgb_frames {
                                        rgb_frames.push(bytes);
                                        frame_count += 1;
                                    }
                                }
                            }
                        }
                        Some("/lidar/cloud") => {
                            // Point cloud data
                            if let Ok(bytes) = serde_json::to_vec(&msg["data"]) {
                                lidar_cloud.extend_from_slice(&bytes);
                            }
                        }
                        Some("/state") => {
                            // IMU / robot state
                            if let Ok(bytes) = serde_json::to_vec(&msg["data"]) {
                                imu_samples.extend_from_slice(&bytes);
                                imu_samples.push(b'\n');
                            }
                        }
                        _ => {}
                    }
                }
            }
            Ok(Message::Ping(payload)) => {
                let _ = ws.send(Message::Pong(payload));
            }
            Ok(Message::Close(_)) => {
                info!("Go2 WebSocket closed by robot");
                break;
            }
            Err(tungstenite::Error::Io(e))
                if e.kind() == std::io::ErrorKind::WouldBlock
                || e.kind() == std::io::ErrorKind::TimedOut => {
                // Timeout on read — deadline check next iteration
                continue;
            }
            Err(e) => {
                warn!(error = %e, "Go2 WebSocket read error");
                break;
            }
            _ => {}
        }
    }

    let _ = ws.close(None);

    info!(
        frames  = frame_count,
        lidar_b = lidar_cloud.len(),
        imu_b   = imu_samples.len(),
        "Go2 capture complete"
    );

    if frame_count == 0 && lidar_cloud.is_empty() {
        return Err("Go2 returned no sensor data".into());
    }

    // Flatten RGB frames into one buffer (real pipeline: pass Vec<Vec<u8>> to encoder)
    let rgb_flat: Vec<u8> = rgb_frames.into_iter().flatten().collect();

    let duration_ms = cfg.capture_duration_secs * 1000;
    let f1 = estimate_f1_from_frame_count(frame_count, cfg.capture_duration_secs);

    Ok(RawCapture {
        rgb_frames:     Some(rgb_flat),
        lidar_cloud:    if lidar_cloud.is_empty() { None } else { Some(lidar_cloud) },
        imu_log:        if imu_samples.is_empty() { None } else { Some(imu_samples) },
        splat:          None, // built post-capture by nerfstudio / gaussian-splatting
        frame_count,
        duration_ms,
        f1_score:       f1,
        coverage_pct:   (frame_count as f32 * 0.6).min(100.0),
        novelty_score:  0.7,
        delta_coverage: 12.0,
    })
}

/// Estimate F1 reconstruction quality from frame rate.
fn estimate_f1_from_frame_count(frames: u32, secs: u64) -> f32 {
    let fps = frames as f32 / secs.max(1) as f32;
    // 30 fps → ~0.92; 15 fps → ~0.85; 5 fps → ~0.78
    (0.78 + (fps / 30.0).min(1.0) * 0.14).min(0.95)
}

fn base64_decode(s: &str) -> Result<Vec<u8>, ()> {
    use std::io::Read;
    // Simple hand-rolled base64 decode to avoid adding another dependency
    // Production: use the base64 crate (already in workspace via dip)
    let cleaned: String = s.chars().filter(|c| !c.is_whitespace()).collect();
    let alphabet = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut table = [0u8; 256];
    for (i, &c) in alphabet.iter().enumerate() {
        table[c as usize] = i as u8;
    }

    let bytes = cleaned.as_bytes();
    let mut out = Vec::with_capacity(bytes.len() * 3 / 4);
    let mut i = 0;
    while i + 3 < bytes.len() {
        let b0 = table[bytes[i]     as usize] as u32;
        let b1 = table[bytes[i + 1] as usize] as u32;
        let b2 = if bytes[i + 2] == b'=' { 0 } else { table[bytes[i + 2] as usize] as u32 };
        let b3 = if bytes[i + 3] == b'=' { 0 } else { table[bytes[i + 3] as usize] as u32 };
        let n = (b0 << 18) | (b1 << 12) | (b2 << 6) | b3;
        out.push((n >> 16) as u8);
        if bytes[i + 2] != b'=' { out.push((n >> 8) as u8); }
        if bytes[i + 3] != b'=' { out.push(n as u8); }
        i += 4;
    }
    Ok(out)
}

