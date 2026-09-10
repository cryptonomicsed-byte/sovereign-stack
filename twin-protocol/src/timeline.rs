//! 4D Twin provenance timeline — ordered sequence of twin capture snapshots.
//!
//! A TwinTimeline groups all captures of the same physical space into a
//! temporal sequence. Each entry records what was captured, when, where (Odù tile),
//! and which devices contributed.
//!
//! Timeline ID convention:
//!   "timeline:{device_id}" — all captures by this device
//!   "timeline:{odu_tile}"  — all captures within an Odù tile (cross-device)
//!
//! The timeline enables 4D queries:
//!   "What did tile odu:5b look like between timestamps T1 and T2?"

use serde::{Deserialize, Serialize};

/// A single snapshot in a twin's provenance timeline.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TwinTimelineEntry {
    /// Unique ID for this timeline snapshot.
    pub snapshot_id:  String,
    /// The TwinAsset twin_id for this capture.
    pub twin_id:      String,
    /// SceneReceipt ID (kind 31030).
    pub receipt_id:   String,
    /// Primary capture device.
    pub device_id:    String,
    /// Unix milliseconds when the capture completed.
    pub timestamp:    u64,
    /// Odù tile where this capture occurred (None = location unknown).
    pub odu_tile:     Option<String>,
    /// Sui object ID if the TwinAsset was anchored on-chain.
    pub sui_object_id: Option<String>,
    /// F1 quality score from the reconstruction engine (0.0–1.0).
    pub quality:      f32,
    /// Data modalities present (e.g. ["rgb", "lidar", "splat"]).
    pub modalities:   Vec<String>,
}

/// Ordered sequence of captures for a physical space or device.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct TwinTimeline {
    /// Logical grouping key (e.g. "timeline:unitree:go2:192.168.1.10" or "timeline:odu:5b").
    pub timeline_id: String,
    /// Human label for this timeline (optional).
    pub label:       Option<String>,
    /// Entries in chronological order (oldest first).
    pub entries:     Vec<TwinTimelineEntry>,
    /// Monotonically increasing version counter (incremented on each append).
    pub version:     u32,
}

impl TwinTimeline {
    pub fn new(timeline_id: impl Into<String>) -> Self {
        Self {
            timeline_id: timeline_id.into(),
            label:       None,
            entries:     vec![],
            version:     0,
        }
    }

    pub fn with_label(mut self, label: impl Into<String>) -> Self {
        self.label = Some(label.into());
        self
    }

    /// Append a new snapshot entry (entries kept chronological by timestamp).
    pub fn append(&mut self, entry: TwinTimelineEntry) {
        self.version += 1;
        self.entries.push(entry);
        self.entries.sort_by_key(|e| e.timestamp);
    }

    /// Most recent entry.
    pub fn latest(&self) -> Option<&TwinTimelineEntry> {
        self.entries.last()
    }

    /// Earliest entry.
    pub fn earliest(&self) -> Option<&TwinTimelineEntry> {
        self.entries.first()
    }

    /// Time span in milliseconds (None if fewer than 2 entries).
    pub fn span_ms(&self) -> Option<u64> {
        if self.entries.len() < 2 {
            return None;
        }
        let t0 = self.entries.first()?.timestamp;
        let t1 = self.entries.last()?.timestamp;
        Some(t1.saturating_sub(t0))
    }

    /// Filter entries within a time window [from_ms, to_ms].
    pub fn window(&self, from_ms: u64, to_ms: u64) -> Vec<&TwinTimelineEntry> {
        self.entries.iter()
            .filter(|e| e.timestamp >= from_ms && e.timestamp <= to_ms)
            .collect()
    }

    /// Build a timeline_id from a device_id.
    pub fn device_id(device_id: &str) -> String {
        format!("timeline:{device_id}")
    }

    /// Build a timeline_id from an Odù tile_id.
    pub fn tile_id(tile_id: &str) -> String {
        format!("timeline:{tile_id}")
    }
}

/// Construct a TwinTimelineEntry from capture job outputs.
pub fn entry_from_capture(
    twin_id:      impl Into<String>,
    receipt_id:   impl Into<String>,
    device_id:    impl Into<String>,
    timestamp:    u64,
    odu_tile:     Option<String>,
    sui_object_id: Option<String>,
    quality:      f32,
    has_splat:    bool,
    has_lidar:    bool,
) -> TwinTimelineEntry {
    let mut modalities = vec!["rgb".into()];
    if has_lidar { modalities.push("lidar".into()); }
    if has_splat { modalities.push("splat".into()); }
    modalities.push("telemetry".into());
    modalities.push("imu".into());

    TwinTimelineEntry {
        snapshot_id:  format!("snap:{}", uuid::Uuid::new_v4()),
        twin_id:      twin_id.into(),
        receipt_id:   receipt_id.into(),
        device_id:    device_id.into(),
        timestamp,
        odu_tile,
        sui_object_id,
        quality,
        modalities,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_entry(twin_id: &str, ts: u64) -> TwinTimelineEntry {
        TwinTimelineEntry {
            snapshot_id:   format!("snap:{twin_id}"),
            twin_id:       twin_id.into(),
            receipt_id:    format!("rec:{twin_id}"),
            device_id:     "go2:1".into(),
            timestamp:     ts,
            odu_tile:      None,
            sui_object_id: None,
            quality:       0.8,
            modalities:    vec!["rgb".into(), "splat".into()],
        }
    }

    #[test]
    fn append_keeps_chronological_order() {
        let mut tl = TwinTimeline::new("timeline:go2:1");
        tl.append(make_entry("twin:b", 2000));
        tl.append(make_entry("twin:a", 1000));
        tl.append(make_entry("twin:c", 3000));
        assert_eq!(tl.entries[0].twin_id, "twin:a");
        assert_eq!(tl.entries[1].twin_id, "twin:b");
        assert_eq!(tl.entries[2].twin_id, "twin:c");
        assert_eq!(tl.version, 3);
    }

    #[test]
    fn span_ms_correct() {
        let mut tl = TwinTimeline::new("t");
        assert!(tl.span_ms().is_none());
        tl.append(make_entry("a", 1000));
        assert!(tl.span_ms().is_none());
        tl.append(make_entry("b", 4000));
        assert_eq!(tl.span_ms(), Some(3000));
    }

    #[test]
    fn window_filters_correctly() {
        let mut tl = TwinTimeline::new("t");
        tl.append(make_entry("twin:a", 1000));
        tl.append(make_entry("twin:b", 2000));
        tl.append(make_entry("twin:c", 3000));
        let w = tl.window(1500, 2500);
        assert_eq!(w.len(), 1);
        assert_eq!(w[0].twin_id, "twin:b");
    }

    #[test]
    fn entry_from_capture_includes_splat_when_present() {
        let e = entry_from_capture("t", "r", "d", 0, None, None, 0.9, true, true);
        assert!(e.modalities.contains(&"splat".to_string()));
        assert!(e.modalities.contains(&"lidar".to_string()));
    }

    #[test]
    fn timeline_id_helpers() {
        assert_eq!(TwinTimeline::device_id("go2:1"), "timeline:go2:1");
        assert_eq!(TwinTimeline::tile_id("odu:5b"), "timeline:odu:5b");
    }
}
