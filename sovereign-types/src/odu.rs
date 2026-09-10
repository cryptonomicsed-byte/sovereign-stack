//! Odù spatial tile coordinate system.
//!
//! The Vantage physical world is divided into a 16×16 grid of 256 Odù tiles,
//! named after the 256 Odù Ifá — the corpus of knowledge in Yorùbá divination.
//!
//! Coordinate system:
//!   x ∈ [0, 15]  — longitude axis (West→East)
//!   y ∈ [0, 15]  — latitude axis  (South→North)
//!
//! Tile ID format: "odu:{x:1x}{y:1x}" — e.g. "odu:00", "odu:ff"
//!
//! Scale is deployment-defined: a tile may cover ~10km² (city) or ~100m² (building).
//! Receipts carry an optional tile_id so the provenance system is spatially indexed.

use serde::{Deserialize, Serialize};

/// A coordinate in the 16×16 Odù tile grid.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct OduCoordinate {
    /// Longitude axis: 0 (West) → 15 (East)
    pub x: u8,
    /// Latitude axis: 0 (South) → 15 (North)
    pub y: u8,
}

impl OduCoordinate {
    /// Construct a coordinate, clamping to [0, 15].
    pub fn new(x: u8, y: u8) -> Self {
        Self { x: x.min(15), y: y.min(15) }
    }

    /// Parse from tile_id string "odu:XY" where X, Y are hex nibbles.
    pub fn from_tile_id(tile_id: &str) -> Option<Self> {
        let hex = tile_id.strip_prefix("odu:")?;
        if hex.len() != 2 { return None; }
        let x = u8::from_str_radix(&hex[..1], 16).ok()?;
        let y = u8::from_str_radix(&hex[1..], 16).ok()?;
        Some(Self { x, y })
    }

    /// Canonical tile_id: "odu:XY" (single hex nibble each).
    pub fn tile_id(&self) -> String {
        format!("odu:{:x}{:x}", self.x, self.y)
    }

    /// Index in 0..255 (row-major: y * 16 + x).
    pub fn index(&self) -> u8 {
        self.y * 16 + self.x
    }

    /// Neighbors (cardinal directions), clamped to grid bounds.
    pub fn neighbors(&self) -> Vec<OduCoordinate> {
        let mut out = Vec::with_capacity(4);
        if self.x > 0  { out.push(Self::new(self.x - 1, self.y)); }
        if self.x < 15 { out.push(Self::new(self.x + 1, self.y)); }
        if self.y > 0  { out.push(Self::new(self.x, self.y - 1)); }
        if self.y < 15 { out.push(Self::new(self.x, self.y + 1)); }
        out
    }
}

impl std::fmt::Display for OduCoordinate {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.tile_id())
    }
}

/// A single Odù tile with optional geographic bounds.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OduTile {
    pub tile_id:    String,
    pub coordinate: OduCoordinate,
    /// Optional geographic bounds (WGS84 decimal degrees).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bounds:     Option<OduBounds>,
    /// Human label — defaults to tile_id if not set.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label:      Option<String>,
}

impl OduTile {
    pub fn new(x: u8, y: u8) -> Self {
        let coord = OduCoordinate::new(x, y);
        Self { tile_id: coord.tile_id(), coordinate: coord, bounds: None, label: None }
    }

    pub fn with_bounds(mut self, bounds: OduBounds) -> Self {
        self.bounds = Some(bounds);
        self
    }

    pub fn with_label(mut self, label: impl Into<String>) -> Self {
        self.label = Some(label.into());
        self
    }
}

/// Axis-aligned bounding box in WGS84 decimal degrees.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OduBounds {
    pub min_lat: f64,
    pub max_lat: f64,
    pub min_lon: f64,
    pub max_lon: f64,
}

impl OduBounds {
    pub fn new(min_lat: f64, max_lat: f64, min_lon: f64, max_lon: f64) -> Self {
        Self { min_lat, max_lat, min_lon, max_lon }
    }

    pub fn contains(&self, lat: f64, lon: f64) -> bool {
        lat >= self.min_lat && lat <= self.max_lat &&
        lon >= self.min_lon && lon <= self.max_lon
    }
}

/// Generate all 256 Odù tiles in row-major order (y=0..15, x=0..15).
pub fn all_tiles() -> impl Iterator<Item = OduCoordinate> {
    (0u8..16).flat_map(|y| (0u8..16).map(move |x| OduCoordinate::new(x, y)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tile_id_roundtrips() {
        let c = OduCoordinate::new(5, 11);
        let id = c.tile_id();
        assert_eq!(id, "odu:5b");
        let c2 = OduCoordinate::from_tile_id(&id).unwrap();
        assert_eq!(c, c2);
    }

    #[test]
    fn index_is_row_major() {
        assert_eq!(OduCoordinate::new(0, 0).index(), 0);
        assert_eq!(OduCoordinate::new(15, 0).index(), 15);
        assert_eq!(OduCoordinate::new(0, 1).index(), 16);
        assert_eq!(OduCoordinate::new(15, 15).index(), 255);
    }

    #[test]
    fn neighbors_at_corner() {
        let n = OduCoordinate::new(0, 0).neighbors();
        // corner: only right and up
        assert_eq!(n.len(), 2);
        assert!(n.contains(&OduCoordinate::new(1, 0)));
        assert!(n.contains(&OduCoordinate::new(0, 1)));
    }

    #[test]
    fn all_tiles_yields_256() {
        assert_eq!(all_tiles().count(), 256);
    }

    #[test]
    fn bounds_contains() {
        let b = OduBounds::new(37.0, 38.0, -122.5, -121.5);
        assert!(b.contains(37.5, -122.0));
        assert!(!b.contains(36.0, -122.0));
    }

    #[test]
    fn invalid_tile_id_returns_none() {
        assert!(OduCoordinate::from_tile_id("bad").is_none());
        assert!(OduCoordinate::from_tile_id("odu:xyz").is_none());
        assert!(OduCoordinate::from_tile_id("odu:5").is_none());
    }
}
