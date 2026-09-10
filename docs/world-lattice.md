# Odù World Lattice — Hierarchical Tile Design

> "The binary lattice of Ifá: 2 → 16 → 256 → 65,536 → 2³² → 2⁴⁰"
> — Veil 1: Ifá / Binary Bones

---

## Concept

The Sovereign world is a **Zelda-style hierarchical tile lattice** rooted in
Ifá's binary cosmology. The same binary progression that generates the 256 Odù
(16×16 = 4 bits × 4 bits) extends infinitely — each tile subdivides into 256
sub-tiles, producing a fractal world of arbitrary depth.

This is not just aesthetics. The binary structure is:
- **Cache-friendly**: power-of-2 tile counts align with GPU texture pages, VRAM,
  and Merkle tree depths
- **Verifiable**: every tile has a deterministic content hash from its coordinate
  and seed — no centralized authority needed
- **Economically elegant**: tile scarcity increases with depth; deeper tiles
  require more proven work to claim

---

## The Binary Progression

```
Level 0:  2¹  =       2   (binary: yin/yang, 0/1)
Level 1:  2⁴  =      16   (16 primary Odù families)
Level 2:  2⁸  =     256   (256 full Odù — the base world map, 16×16)
Level 3:  2¹⁶ =  65,536   (256×256 — continents; each base tile subdivides)
Level 4:  2³² = ~4.3B     (planetary-scale: real-world coverage)
Level 5:  2⁴⁰ = ~1.1T     (universe scale; agent-generated worlds)
```

The **Level 2** grid (256 tiles, 16×16) is the **Odù base** — the atomic world
map that agents, captures, and proofs reason about. Everything else is
recursion on top of it.

---

## Tile Coordinate System

### Notation

```
odu://L{level}:{x_base16}:{y_base16}
```

Examples:
```
odu://L2:0:0        → base tile at (0,0) — top-left corner of the world map
odu://L2:F:F        → base tile at (15,15) — bottom-right
odu://L3:3A:7C      → level-3 sub-tile at (0x3A, 0x7C) within the L2 grid
odu://L3:0:0        → level-3 sub-tile (0,0) inside odu://L2:0:0
```

### Hierarchical addressing

Each Level 2 tile `odu://L2:X:Y` contains a full 16×16 sub-grid at Level 3.
The Level 3 address of sub-tile `(sx, sy)` within base tile `(bx, by)` is:

```
odu://L3:{bx*16 + sx}:{by*16 + sy}
```

In general, a tile at level N+1 with local coordinates `(sx, sy)` within
parent `odu://LN:bx:by`:

```
odu://L{N+1}:{bx*16 + sx}:{by*16 + sy}
```

This is a flat address space at each level — simple to index, simple to hash.

### Rust type

```rust
pub struct OduCoordinate {
    pub level: u8,   // 2 = base world, 3 = continent, 4 = planetary, 5 = universe
    pub x: u64,      // up to 2^40 at level 5
    pub y: u64,
}

impl OduCoordinate {
    pub fn tile_id(&self) -> String {
        format!("odu://L{}:{:X}:{:X}", self.level, self.x, self.y)
    }

    pub fn children(&self) -> impl Iterator<Item = OduCoordinate> {
        let base_x = self.x * 16;
        let base_y = self.y * 16;
        let level = self.level + 1;
        (0u64..16).flat_map(move |sx| {
            (0u64..16).map(move |sy| OduCoordinate {
                level,
                x: base_x + sx,
                y: base_y + sy,
            })
        })
    }

    pub fn parent(&self) -> Option<OduCoordinate> {
        if self.level <= 2 { return None; }
        Some(OduCoordinate {
            level: self.level - 1,
            x: self.x / 16,
            y: self.y / 16,
        })
    }
}
```

---

## Tile Content Hash

Every tile has a deterministic content hash. Same coordinate + same seed =
same tile. No central authority.

```
tile_hash = sha256(
    level        ||   // 1 byte
    x_le64       ||   // 8 bytes
    y_le64       ||   // 8 bytes
    veil_seed    ||   // 32 bytes (from the governing Veil for this level)
    parent_hash  ||   // 32 bytes (empty for L2 root tiles)
    capture_root     // 32 bytes (Merkle root of all captures in this tile; zeros if unclaimed)
)
```

This hash is the tile's Walrus blob key and its Merkle proof anchor. Updates to
a tile (new captures, new sim runs) produce a new `capture_root` and thus a new
`tile_hash` — versioned, auditable, chain-anchored.

---

## Vertical Layers

Each (level, x, y) coordinate has multiple **vertical layers**:

```
Layer 4  — Aether / Veil overlay (quantum variants, agent-generated)
Layer 3  — Sky / digital twin (Gaussian splat, ỌSỌVM simulation space)
Layer 2  — Surface (NuRec reconstruction, real-world terrain)
Layer 1  — Underground (subsurface sensors, tunnel networks)
Layer 0  — Bedrock (fixed; generated from geographic seed; never changes)
```

Full address including layer:

```
odu://L{level}:{x}:{y}#layer{n}
```

Example: `odu://L3:1A:2B#layer3` — the digital twin layer of continent tile (0x1A, 0x2B).

Layer 3 (Sky/Digital) is where Gaussian splats live. ỌSỌVM sims run in Layer 3
and 4. Physical sensors populate Layer 2 and 1. Bedrock is the procedural
baseline.

---

## Veil Assignment by Level

The 777 Veils are distributed across the lattice. Each level-2 base tile is
governed by a cluster of Veils that determine:
- The procedural generation seed for that tile's terrain
- The simulation challenges available in that tile
- The difficulty parameters for PoS in that region

```
Tile (0,0)–(3,3)    → Veils 1–25    (control theory, LQR, stability)
Tile (4,0)–(7,3)    → Veils 26–75   (ML / reinforcement learning)
Tile (8,0)–(11,3)   → Veils 76–150  (physics simulation, ODE, fluid dynamics)
Tile (12,0)–(15,3)  → Veils 151–250 (robotics, kinematics, pathfinding)
Tile (0,4)–(15,7)   → Veils 251–400 (perception, vision, SLAM)
Tile (0,8)–(15,11)  → Veils 401–600 (multi-agent, swarm, coordination)
Tile (0,12)–(15,15) → Veils 601–777 (embodiment, transfer, real-world grounding)
```

This assignment creates natural geographic clustering of expertise — agents
specialised in control theory cluster in the top-left continent family; swarm
agents cluster in the lower-middle band.

---

## Caching Architecture

### Hot / Warm / Cold hierarchy

```
HOT   (GPU VRAM)       — Current tile + 8 adjacent + all layers of current tile
WARM  (RAM / SSD)      — Current continent (256 tiles at L2, 65,536 at L3)
COLD  (Walrus blobs)   — All other tiles, content-addressed by tile_hash
```

Agents earn tokens for:
- **Pinning**: holding a tile's Walrus blob available for retrieval
- **Serving**: delivering a tile blob to a requesting agent (bandwidth proof)
- **Generating**: computing a new tile for the first time (highest reward)

### Level-of-detail

Distant tiles (> N hops from current position) render at reduced LOD:
```
Distance 0–1  → full resolution (all layers, full Gaussian splat)
Distance 2–4  → mid LOD (layers 2+3 only, reduced splat density)
Distance 5+   → low LOD (bedrock + surface outline only)
Distance 10+  → icon only (tile economy state dot)
```

Procedural rules fill L2 tiles until real capture data arrives. Once a
`GaussianProof` is committed to a tile, it replaces the procedural baseline.

---

## Geographic Mapping (Level 2 → Real World)

The 256 base tiles map to Earth's surface using an **equal-area** projection
divided into 16×16 zones. This gives each tile approximately:

```
Earth surface: 510,000,000 km²
Per tile:      510,000,000 / 256 ≈ 1,992,188 km²  (~2M km²)
```

Roughly the size of Sudan or Mexico per base tile. Level 3 sub-tiles are:

```
~2,000,000 km² / 256 ≈ 7,813 km²  (~78km × 100km per L3 tile)
```

And Level 4:
```
~7,813 km² / 256 ≈ 30.5 km²  (~5.5km × 5.5km per L4 tile)
```

Level 4 is the sweet spot for drone operation areas and city-district coverage.

### Gods-Eye-View integration

The Odù tile grid renders as a **CesiumJS entity layer** on top of the
`bilawalsidhu/gods-eye-view` photorealistic globe. The base globe (terrain,
buildings, live aircraft/ships/satellites) is untouched. Sovereign stack adds:

```javascript
// CesiumJS pseudocode — actual implementation in sovereign-globe/
viewer.entities.add({
    id: tile.tile_id,
    rectangle: {
        coordinates: Rectangle.fromDegrees(west, south, east, north),
        material: tile_economy_color(tile.state),  // grey/gold/green/active
        height: 0,
        outline: true,
        outlineColor: Color.fromCssColorString('#FFD700').withAlpha(0.3),
    },
    label: {
        text: tile.tile_id,
        font: '11pt monospace',
    }
});

// Gaussian splat receipts as 3D billboards
viewer.entities.add({
    id: receipt.receipt_id,
    position: Cartesian3.fromDegrees(lon, lat, alt),
    billboard: {
        image: splat_quality_icon(receipt.quality),
        scale: 0.5,
    }
});
```

New items rendered by agents: Odù tile rectangles, splat billboards, proof
activity heatmap, VeilSim binding indicators, ỌSỌVM sim traces.

---

## Tile Economy Integration

### Claiming a tile

```
Unclaimed tile at odu://L2:X:Y
     ↓
Agent stakes N Àṣẹ (N = base_stake × (1 + capture_count/100))
     ↓
Tile becomes Claimed (owner_did = agent's DID)
     ↓
Agent earns usage_fee_pct (default 5%) on all captures and sim rentals in tile
```

### VeilSim binding (kind 1903)

A Gaussian splat in a tile becomes a `VeilSim1to1` IP asset when:
1. A valid `GaussianProof` is committed to the tile (Layer 3)
2. ỌSỌVM runs at least one `SimulationReceipt` grounded in that twin

At that point the tile asset is anchored on Sui as an IP object (kind 1903 Twin
Binding) with `VeilSim1to1` type. Ownership of the IP object = ownership of the
tile's sim rights.

### Sim rental

Anyone can pay Àṣẹ to run an ỌSỌVM simulation against a tile's TwinAsset
without owning the tile. The rental fee flows to the tile owner.

```
POST /tiles/{tile_id}/rent-sim
  body: { scenario, trajectory_count, budget_ase }
  → ỌSỌVM runs the scenario
  → rental_fee deducted from caller's wallet
  → rental_fee × (1 - 0.0369) → tile owner
  → rental_fee × 0.0369 → Éṣù tithe
  → OsovmRunResult returned to caller
```

---

## Continent Biomes (Level 3 Groupings)

Each 256×256 block of Level-3 tiles (= one Level-2 base tile) is a "continent"
with a distinct character determined by its governing Veil cluster:

| Continent | L2 Base Tile | Veil Range | Character |
|---|---|---|---|
| Ìgbó Ọ̀rún (Forest of Heaven) | (0,0)–(3,3) | 1–25 | Control theory, stability, balance |
| Aṣọdun (Market of Seasons) | (4,0)–(7,3) | 26–75 | ML, learning, adaptation |
| Omi Àgbàdo (Waters of Maize) | (8,0)–(11,3) | 76–150 | Physics, fluids, energy |
| Ilẹ̀ Àárẹ̀ (Land of Rulers) | (12,0)–(15,3) | 151–250 | Robotics, kinematics, authority |
| Ojú Ọ̀run (Eye of Heaven) | (0,4)–(15,7) | 251–400 | Perception, vision, SLAM |
| Ipàdé Àgbẹ̀dẹ̀ (Gathering of Smiths) | (0,8)–(15,11) | 401–600 | Multi-agent, swarm, forging |
| Ilẹ̀ Àánú (Land of Grace) | (0,12)–(15,15) | 601–777 | Embodiment, transfer, grounding |

---

## Implementation Roadmap

### Phase 1 — Base layer (L2, 256 tiles)
- [ ] `OduCoordinate` extended to support `level` field (currently only L2)
- [ ] Geographic bounding box lookup for each L2 tile
- [ ] CesiumJS tile layer in Gods-Eye-View fork (`sovereign-globe/`)
- [ ] Tile economy routes: claim, stake, usage_fee

### Phase 2 — Continent layer (L3, 65,536 tiles)
- [ ] L3 coordinate arithmetic in `sovereign-types`
- [ ] Walrus blob key derivation from `tile_hash`
- [ ] LOD system for distant tiles
- [ ] Pinning reward mechanism

### Phase 3 — Veil assignment and procedural generation
- [ ] Veil-to-tile mapping registry
- [ ] Procedural terrain seed from `(tile_id, veil_seed)`
- [ ] ỌSỌVM scenario selection by tile's Veil cluster
- [ ] VeilSim binding trigger (kind 1903)

### Phase 4 — Vertical layers and GPU compute
- [ ] Layer addressing (`#layer{n}` suffix)
- [ ] Gaussian splat placement in Layer 3
- [ ] NVIDIA Omniverse / Isaac Sim USD export per tile
- [ ] GPU node rewards for tile generation

### Phase 5 — Universe scale (L4+)
- [ ] L4 tile addressing (~30 km² per tile — drone operational area)
- [ ] Agent navigation graph across tile boundaries
- [ ] Seamless streaming transitions (Zelda-style)
- [ ] Multi-agent continent assignments
