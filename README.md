# Sovereign Stack

A Rust workspace implementing the full Vantage sovereign-node architecture — physical-world capture, 4D digital twin provenance, Proof-of-Useful-Simulation, and a decentralized spatial economy on Sui.

---

## Architecture

```
sovereign-types        — shared primitives (Odù tiles, GaussianProof, RTS, tier system)
sovereign-runtime      — embodiment model, capability chain, safety gates, act receipts
ip-layer               — IP root, creation receipts, attestation, Nostr publishing
dip                    — Decentralized Identity Protocol (peer-to-peer agent messaging)
vcp                    — Vantage Capture Protocol (device transport, mDNS discovery, body store)
twin-protocol          — TwinTimeline 4D provenance, Àṣẹ mint engine, Sui RPC anchor, ỌSỌVM bridge
sovereign-pipeline     — COLMAP → Gaussian splatting → export, proof chain
sovereign-a2a          — A2A v1.0 agent card, task lifecycle, dispatch router, client
sovereign-node         — HTTP node server (all routes, background jobs, event bus)
sovereign-cli          — CLI for interacting with a running sovereign node
mycelium-finetune      — Claude JSONL → alpaca QLoRA fine-tuning pipeline
sovereign-os           — Sovereign OS layer (identity, memory, tool hooks, act receipts)
```

---

## Crate Overview

### `sovereign-types`
Shared data types used across the workspace.
- **Odù tile grid** — 16×16 = 256 spatial tiles (`OduCoordinate`, `tile_id`, neighbor traversal)
- **Tier system** — `TrustTier` (T0–T5), `ProofDomain` (Simulation / Spatial / Physical), `ProofEvaluation`, `ProofVector`
- **Spatial types** — `GaussianQualityMetrics`, `GaussianProof`, `RealityTransferScore`
- **Receipts, Merkle, crypto, identity** primitives

### `sovereign-runtime`
Defines what it means for an agent to inhabit a device.
- Embodiment model with capability chain and safety gate enforcement
- Device integration (Go2, Omarchy hardware tiers)
- Power budget tracking and execution receipts

### `ip-layer`
Intellectual property provenance for every artifact produced in the network.
- `IPRoot` — canonical provenance record anchored on Sui
- `CreationReceipt` — cryptographically signed record of authorship
- `AttestationRecord` — witness co-signatures
- Nostr event publishing and twin binding

### `dip`
Decentralized Identity Protocol — peer-to-peer messaging between sovereign nodes.
- Signed message envelopes over WebSocket / HTTP / mesh
- Inbound webhook handler, peer registry

### `vcp`
Vantage Capture Protocol — manages physical capture sessions.
- Device discovery via mDNS
- Body store for raw capture data
- VCP session lifecycle and transport

### `twin-protocol`
4D digital twin provenance and spatial economy.
- **`TwinTimeline`** — ordered snapshot history per device with `window()`, `tile_id()`, `device_id()` filters
- **Àṣẹ mint engine** — `calculate_mint_amount(quality, novelty)` using `BASE_RATE_MICRO × quality × novelty`
- **Sui RPC anchor** — on-chain object anchoring for twin states
- **ỌSỌVM bridge** — routes twin events into the simulation VM

### `sovereign-pipeline`
Turns raw sensor data into a Gaussian splat and proof chain.
- Runs COLMAP for camera pose estimation
- Invokes `gaussian_splatting` trainer
- Exports `.ply` / `.splat` output
- Builds `GaussianProof` with quality metrics and `ProofChain`

### `sovereign-a2a`
A2A v1.0 agent-to-agent protocol.
- `AgentCard` — capability advertisement
- `Task` lifecycle (submitted → working → completed / failed)
- Dispatch router and async client

### `sovereign-node`
The sovereign node HTTP server — the operational core.

**Key background services**
| Service | Purpose |
|---|---|
| Active Perception Loop | Monitors `DeviceHealth` (Online / Stale / Offline), pings Go2 |
| TwinTimeline Appender | Subscribes to `TwinEvent` bus, appends `CaptureComplete` to timeline |
| Swarm Monitor | Aggregates child-job status for multi-device swarm captures |

**Routes**
| Method | Path | Description |
|---|---|---|
| `POST` | `/capture` | Start a single VCP capture job |
| `POST` | `/capture/swarm` | Start a multi-device swarm capture |
| `POST` | `/capture/delegate` | Delegate capture to peer via A2A |
| `GET` | `/jobs/:job_id` | Job status |
| `GET` | `/receipts` | All receipts |
| `GET` | `/receipts/root` | SHA-256 Merkle root over all receipts |
| `GET` | `/twins/:twin_id/timeline` | 4D timeline for a twin |
| `GET` | `/timelines` | All timelines |
| `GET` | `/tiles` | All 256 Odù tiles |
| `GET` | `/tiles/:tile_id/receipts` | Receipts by tile |
| `GET` | `/swarm` | All swarm jobs |
| `GET` | `/swarm/:swarm_id` | Swarm status |
| `GET` | `/events/receipts` | SSE stream of `TwinEvent`s |
| `GET` | `/ws/twin/:twin_id` | WebSocket — real-time twin events |
| `GET` | `/ws/splat/:twin_id` | WebSocket — binary PLY splat stream (64 KiB chunks) |
| `POST` | `/proof/simulation` | Evaluate a `SimulationProof` |
| `POST` | `/proof/gaussian` | Evaluate a `GaussianProof` |
| `POST` | `/proof/physical` | Evaluate a `RealityTransferScore` |
| `GET` | `/devices` | Registered devices |
| `POST` | `/devices` | Register device |
| `GET` | `/health` | Node health |
| `GET` | `/a2a/*` | A2A agent card and task endpoints |

### `sovereign-cli`
Command-line interface for a running sovereign node.
```
sovereign capture [--device <id>]
sovereign swarm --devices <id,id,...>
sovereign proof sim --file <proof.json>
sovereign tiles
sovereign receipts root
sovereign timeline <twin_id>
```

### `mycelium-finetune`
Converts Claude conversation JSONL exports into alpaca-format training data for QLoRA fine-tuning a local 3B model (Qwen / Llama) into the Mycelium sovereign brain.

### `sovereign-os`
The agent operating system layer.
- Persistent identity (DID + keypair)
- Memory store with semantic tagging
- Tool hook dispatch
- `ActReceipt` — signed record of every tool invocation

---

## Proof-of-Useful-Simulation

The network rewards agents that produce *useful, novel, real-world-transferable* work.

| Proof Domain | Source | Key metric |
|---|---|---|
| **Simulation** | `SimulationProof` | gates cleared, controller stability, novelty |
| **Spatial** | `GaussianProof` | area coverage, pose quality, witness count |
| **Physical** | `RealityTransferScore` | sim→real policy transfer (geometric mean of 6 axes) |

Each proof is scored by `ProofEngine::evaluate_*` → `ProofEvaluation`:
- `proof_value` — composite score
- `mint_eligible` — whether this proof earns Àṣẹ tokens
- `novelty` — decays with repeated `environment_hash` / `splat_hash` submissions (anti-farming)

---

## Àṣẹ Spatial Economy

Receipt-based token minting anchored to the Odù 16×16 world grid.

```
tokens = BASE_RATE_MICRO × quality_score × novelty_factor
```

- Tile owners earn a configurable fee on every mint in their tile
- `GET /tiles/:tile_id/receipts` shows economic activity per tile
- Mint results are anchored on Sui via `twin-protocol/src/sui_rpc.rs`

---

## Getting Started

### Prerequisites
- Rust 1.80+
- (Optional) COLMAP + gaussian_splatting for real capture pipeline
- (Optional) Sui RPC endpoint for on-chain anchoring

### Build
```bash
cargo build --workspace
```

### Test
```bash
cargo test --workspace
```

### Run a node
```bash
cp sovereign-node/config.example.toml config.toml
# edit config.toml — set device endpoints, pipeline paths, witness peers
cargo run -p sovereign-node -- --config config.toml
```

### CLI
```bash
cargo run -p sovereign-cli -- --node http://localhost:3000 health
cargo run -p sovereign-cli -- --node http://localhost:3000 capture
cargo run -p sovereign-cli -- --node http://localhost:3000 receipts root
```

---

## Workspace Status

| Crate | Tests | Status |
|---|---|---|
| sovereign-types | 26 | ✓ |
| sovereign-runtime | 64 | ✓ |
| ip-layer | 36 | ✓ |
| dip | 12 | ✓ |
| vcp | 17 | ✓ |
| twin-protocol | 22 | ✓ |
| sovereign-pipeline | 18 | ✓ |
| sovereign-a2a | 19 | ✓ |
| sovereign-node | 56 | ✓ |
| sovereign-os | 36 | ✓ |

**Total: 306 tests passing, 0 failures.**

---

## License

Proprietary — Vantage / Sovereign Intelligence Network.
