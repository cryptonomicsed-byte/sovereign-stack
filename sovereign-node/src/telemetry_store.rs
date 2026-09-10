/// Inbound telemetry buffer for active body sessions.
///
/// StampFly (or any VCP body) streams `FlightTelemetry` frames over Wi-Fi.
/// This store buffers them per session and can close a session into a
/// `FlightReceipt` with a trajectory hash.
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

use sha2::{Sha256, Digest};
use vcp::{FlightTelemetry, FlightReceipt};

#[derive(Clone, Default)]
pub struct TelemetryStore(Arc<RwLock<HashMap<String, Vec<FlightTelemetry>>>>);

impl TelemetryStore {
    pub fn new() -> Self { Self::default() }

    /// Append a telemetry frame to a session.
    pub async fn push(&self, session_id: &str, frame: FlightTelemetry) {
        self.0.write().await
            .entry(session_id.to_string())
            .or_default()
            .push(frame);
    }

    /// Return all frames for a session.
    pub async fn frames(&self, session_id: &str) -> Vec<FlightTelemetry> {
        self.0.read().await
            .get(session_id)
            .cloned()
            .unwrap_or_default()
    }

    /// Close the session: compute trajectory hash over all frames and produce a
    /// FlightReceipt.  Frames are removed from the buffer.
    pub async fn close_session(
        &self,
        session_id: &str,
        agent_id: &str,
        body_id: &str,
        sim_proof_id: Option<String>,
        witness_ids: Vec<String>,
        mission_success: bool,
    ) -> FlightReceipt {
        let frames = {
            let mut map = self.0.write().await;
            map.remove(session_id).unwrap_or_default()
        };

        let trajectory_hash = hash_trajectory(&frames);
        let duration_ms = trajectory_duration_ms(&frames);
        let max_altitude = frames.iter().map(|f| f.altitude_m).fold(0.0_f64, f64::max);
        let battery_start = frames.first().map(|f| f.battery_pct).unwrap_or(100.0);
        let battery_end   = frames.last().map(|f| f.battery_pct).unwrap_or(battery_start);
        let battery_consumed = (battery_start - battery_end).max(0.0);

        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;

        FlightReceipt {
            receipt_id:           format!("rcpt:flight:{}", uuid::Uuid::new_v4()),
            session_id:           session_id.to_string(),
            agent_id:             agent_id.to_string(),
            body_id:              body_id.to_string(),
            sim_proof_id,
            trajectory_hash,
            telemetry_count:      frames.len() as u32,
            duration_ms,
            max_altitude_m:       max_altitude,
            battery_consumed_pct: battery_consumed,
            mission_success,
            witness_ids,
            timestamp:            now,
            signature:            String::new(),
        }
    }
}

fn hash_trajectory(frames: &[FlightTelemetry]) -> String {
    let mut hasher = Sha256::new();
    for f in frames {
        hasher.update(f.timestamp_ms.to_le_bytes());
        for v in &f.position  { hasher.update(v.to_le_bytes()); }
        for v in &f.orientation { hasher.update(v.to_le_bytes()); }
        hasher.update(f.altitude_m.to_le_bytes());
        hasher.update(f.velocity_ms.to_le_bytes());
    }
    hex::encode(hasher.finalize())
}

fn trajectory_duration_ms(frames: &[FlightTelemetry]) -> u64 {
    match (frames.first(), frames.last()) {
        (Some(first), Some(last)) => last.timestamp_ms.saturating_sub(first.timestamp_ms),
        _ => 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame(t: u64, alt: f64, bat: f64) -> FlightTelemetry {
        FlightTelemetry {
            timestamp_ms:    t,
            position:        [0.0, 0.0, alt],
            orientation:     [0.0, 0.0, 0.0, 1.0],
            altitude_m:      alt,
            battery_pct:     bat,
            velocity_ms:     0.5,
            obstacle_dist_m: None,
        }
    }

    #[tokio::test]
    async fn push_and_retrieve() {
        let store = TelemetryStore::new();
        store.push("s1", frame(0, 0.0, 100.0)).await;
        store.push("s1", frame(500, 1.5, 98.0)).await;
        let frames = store.frames("s1").await;
        assert_eq!(frames.len(), 2);
    }

    #[tokio::test]
    async fn close_session_produces_receipt() {
        let store = TelemetryStore::new();
        store.push("s2", frame(0,    0.0, 100.0)).await;
        store.push("s2", frame(1000, 2.0,  98.5)).await;
        store.push("s2", frame(2000, 2.0,  97.0)).await;
        let rcpt = store.close_session(
            "s2", "agent:1", "stampfly:1", None, vec![], true
        ).await;
        assert!(!rcpt.trajectory_hash.is_empty());
        assert_eq!(rcpt.telemetry_count, 3);
        assert_eq!(rcpt.duration_ms, 2000);
        assert!((rcpt.max_altitude_m - 2.0).abs() < 1e-9);
        assert!((rcpt.battery_consumed_pct - 3.0).abs() < 1e-9);
        assert!(rcpt.mission_success);
        // buffer cleared
        assert!(store.frames("s2").await.is_empty());
    }

    #[tokio::test]
    async fn close_empty_session_returns_zero_receipt() {
        let store = TelemetryStore::new();
        let rcpt = store.close_session(
            "s3", "agent:1", "body:1", None, vec![], false
        ).await;
        assert_eq!(rcpt.telemetry_count, 0);
        assert_eq!(rcpt.duration_ms, 0);
    }

    #[tokio::test]
    async fn trajectory_hash_deterministic() {
        let store = TelemetryStore::new();
        for session in ["sa", "sb"] {
            store.push(session, frame(0, 0.0, 100.0)).await;
            store.push(session, frame(500, 1.5, 98.0)).await;
        }
        let ra = store.close_session("sa", "a", "b", None, vec![], true).await;
        let rb = store.close_session("sb", "a", "b", None, vec![], true).await;
        assert_eq!(ra.trajectory_hash, rb.trajectory_hash, "same frames → same hash");
    }
}
