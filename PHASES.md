# Sovereign Stack — Phase Checklist
> Canonical inventory. All phases marked ✅ are compiled + tested green.
> **Workspace total: all tests passing as of 2026-09-12.**

---

## Foundation

### Phase 01 ✅ — Cargo workspace
- `sovereign-stack/Cargo.toml` workspace with 9 members
- Members: sovereign-types, dip, vcp, twin-protocol, sovereign-pipeline, sovereign-a2a, sovereign-node, sovereign-cli, mycelium-finetune

### Phase 02 ✅ — sovereign-types crate
- `CaptureRequest`, `CaptureReceipt`, `Quality`, `Modality`
- Odù coordinate system (`OduCoordinate`, `OduBounds`, GPS→tile, tile→bounds)
- `SceneReceipt`, `SimulationReceipt`, region types
- Tests: odu math, coordinate bounds, GPS roundtrip

### Phase 03 ✅ — DIP crate (Decentralized Identity Protocol)
- `DipEnvelope` with Ed25519 signing
- `DipRouter` — route envelopes to registered handlers
- Tests: sign/verify, routing

### Phase 04 ✅ — VCP crate (Vantage Capability Protocol)
- `DeviceSession`, `SessionStore` (in-memory + file-backed)
- `DeviceCapabilities`, `VcpCommand`, `VcpEvent`
- Tests: session lifecycle, store persistence

### Phase 05 ✅ — twin-protocol crate
- `CaptureReceipt` (kind 31020), `SceneReceipt` (kind 31030)
- `SuiAnchor` — Ed25519 sign + Sui JSON-RPC submit stub
- `TwinTimeline`, `TwinTimelineEntry`, `entry_from_capture()`
- `TileEconomy`, `AseMintRequest`, `AseMintResult`, `mint_ase()`, Éṣù 3.69% tithe
- `GrantProposal` governance type
- Tests: receipt construction, anchor signing, timeline window/span, ASE mint math

### Phase 06 ✅ — sovereign-pipeline crate
- `PipelineJob`, `PipelineStore`
- Colmap + gaussian-splatting subprocess driver
- `SplatConfig` with quality presets
- Tests: job lifecycle, config serialization

### Phase 07 ✅ — sovereign-a2a crate (A2A v1.0)
- `AgentCard`, `A2aTask`, `TaskStatus` lifecycle
- `A2aState` with broadcast dispatch channel
- `a2a_router::<S>()` generic Axum router (FromRef pattern)
- Tests: task create/poll/complete, AgentCard fetch

---

## Sovereign Node — HTTP Layer

### Phase 08 ✅ — Axum node skeleton
- `NodeState` with FromRef extraction for all sub-states
- `/status` endpoint, health check
- Port config, graceful shutdown

### Phase 09 ✅ — Device registry endpoints
- `POST /devices/register` — register device with capabilities
- `GET /devices` — list all devices
- `GET /devices/:id` — get single device
- `DELETE /devices/:id` — remove device

### Phase 10 ✅ — Capture job endpoints
- `POST /capture` — submit capture job (device_id, hint, twin_id)
- `GET /jobs` — list jobs with status
- `GET /jobs/:id` — poll single job
- Background `run_capture_job` task: pipeline → receipt → Sui anchor

### Phase 11 ✅ — Receipt store + endpoints
- `ReceiptStore` (disk-backed HashMap at `{data_dir}/receipts/`)
- `GET /receipts` — list all receipts
- `GET /receipts/:id` — get single receipt
- `GET /receipts/:id/verify` — verify receipt in merkle tree

### Phase 12 ✅ — VCP session endpoints
- `POST /vcp/sessions` — open device session
- `GET /vcp/sessions` — list sessions
- `DELETE /vcp/sessions/:id` — close session → queues capture job
- Tests: session close triggers job

### Phase 13 ✅ — DIP gateway endpoints
- `POST /dip/send` — send DIP envelope
- `GET /dip/did` — get node DID
- `POST /dip/register` — register peer DID

---

## Sovereign Node — Real-time Layer

### Phase 14 ✅ — WebSocket twin events (`/ws/twin/:id`)
- `tokio::sync::broadcast::Sender<TwinEvent>` fan-out
- WS upgrade handler — subscribes to twin_events, streams JSON frames
- `TwinEvent` variants: CaptureStarted, CaptureComplete, AnchorConfirmed, StatusUpdate

### Phase 15 ✅ — SSE job stream (`/events/jobs`)
- SSE endpoint streaming job status changes
- `EventSource` compatible format

### Phase 16 ✅ — SSE receipt stream (`/events/receipts`)
- SSE endpoint streaming new receipt arrivals

### Phase 17 ✅ — WS PLY splat stream (`/ws/splat/:twin_id`)
- Streams PLY bytes in 64KiB binary WebSocket frames
- File path: `{splat_output_dir}/{safe_twin_id}/export/splat.ply`
- 426 upgrade required if no real TCP (tower test pattern)

---

## Sovereign Node — Spatial Economy

### Phase 18 ✅ — Odù tile system
- 256 tiles (16×16 grid), `"odu:XY"` IDs
- `GET /tiles` — list all 256 tiles
- `POST /tiles/:id/claim` — claim tile ownership (DID)
- `GET /tiles/:id/receipts` — receipts captured in tile

### Phase 19 ✅ — Tile economy store
- `TileEconomyStore` — tracks `TileEconomy` per tile
- `GET /tiles/:id/economy` — get economy state
- `POST /tiles/:id/stake` — stake ASE tokens
- Tests: claim, economy 404 initially, receipts in tile

### Phase 20 ✅ — Wallet store
- `WalletStore` — DID → balance (disk-backed)
- `GET /wallets` — list wallets
- `GET /wallets/:did` — get balance
- `POST /wallets/:did/credit` — credit tokens
- Tests: credit creates wallet, unknown returns 404

### Phase 21 ✅ — ASE mint flow
- `MintApproved` TwinEvent variant with `eshu_tithe`, `net_minted`, `owner_fee`
- Mint triggered on AnchorConfirmed → calculate_mint_amount → mint_ase
- Éṣù 3.69% tithe routed to governance wallet

---

## Sovereign Node — Governance + Identity

### Phase 22 ✅ — Agent store
- `AgentStore` — DID-keyed agent registry
- `POST /agents` — register agent
- `GET /agents` — list agents
- `GET /agents/:did` — get agent

### Phase 23 ✅ — Identity module
- `IdentityStore` — sovereign identity records
- `POST /identity` — register identity
- `GET /identity/:did` — resolve DID

### Phase 24 ✅ — Governance store + Council
- `GovernanceStore` — proposals, votes
- `CouncilStore` — 12-seat council (staggered quarterly)
- `POST /governance/proposals` — submit GrantProposal
- `POST /governance/proposals/:id/vote` — council vote
- `GET /governance/proposals` — list proposals

### Phase 25 ✅ — Sovereign seat store
- `SovereignSeatStore` — 1,440 office seats (24 sectors × 60 seats)
- `GET /seats` — list seats
- `POST /seats/:id/assign` — assign DID to seat

### Phase 26 ✅ — License store
- `LicenseStore` — data usage licenses
- `POST /licenses` — issue license
- `GET /licenses/:id` — get license
- `POST /licenses/:id/verify` — verify license validity

### Phase 27 ✅ — Witness registry
- `WitnessRegistry` — receipt witness records
- `POST /witnesses` — register witness for receipt
- `GET /witnesses/:receipt_id` — list witnesses

### Phase 28 ✅ — Delegation
- `delegate_capture()` — submits A2A task to peer node
- `POST /capture/delegate` — delegate capture to peer
- `PeersSection` in NodeConfig with peer A2A URLs
- Tests: unknown peer returns 502, delegate routes to peer

---

## Sovereign Node — Simulation + Proof

### Phase 29 ✅ — Simulation scoring
- `SimulationScore`, `ScoreStore`
- `POST /simulation/score` — submit simulation score
- `GET /simulation/scores` — list scores

### Phase 30 ✅ — Proof engine
- `ProofEngine` — zero-knowledge proof stubs
- `POST /proofs/submit` — submit proof for evaluation
- `GET /proofs/:id` — get proof status
- Tests: submit returns evaluation

### Phase 31 ✅ — GPU pool
- `GpuPool` — GPU resource registry
- `POST /gpu/register` — register GPU node
- `GET /gpu` — list available GPUs
- `POST /gpu/allocate` — allocate GPU for job

### Phase 32 ✅ — Emission receipt store
- `EmissionReceiptStore` — Zàngbétò receipts for every emission
- `POST /emissions` — record emission receipt
- `GET /emissions` — list emission receipts

---

## Sovereign Node — Swarm + Timeline

### Phase 33 ✅ — Swarm capture
- `SwarmStore`, `SwarmJob`, `SwarmStatus`
- `POST /capture/swarm` — multi-device swarm capture
- `GET /swarm` — list swarm jobs
- `GET /swarm/:id` — get swarm job status
- `monitor_swarm()` — polls child jobs, broadcasts CaptureComplete per child
- Tests: requires device_ids, creates job, 404 for unknown, poll after submit

### Phase 34 ✅ — Timeline store
- `TimelineStore` (disk-backed, `{data_dir}/timelines/`)
- `GET /twins/:id/timeline` — get twin timeline
- `GET /timelines` — list all timelines
- Background appender: subscribes to TwinEvent → appends on CaptureComplete
- Tests: 404 initially, list empty initially

### Phase 35 ✅ — Receipt merkle tree
- `build_receipt_tree(records)` — sha256 binary tree over receipt_ids
- `verify_receipt_in_root()` — linear scan verification
- `GET /receipts/root` — compute current merkle root
- Tests: empty stable, single deterministic, order-independent, different-receipts-differ

---

## Sovereign Node — Observability

### Phase 36 ✅ — Telemetry store
- `TelemetryStore` — metrics time-series
- `POST /telemetry` — ingest metric
- `GET /telemetry` — query metrics

### Phase 37 ✅ — Body store
- `BodyStore` — physical body/object registry (for spatial twins)
- `POST /bodies` — register body
- `GET /bodies` — list bodies
- `GET /bodies/:id` — get body

### Phase 38 ✅ — Active perception loop
- `spawn_perception_loop()` — tokio background task
- Scans device health periodically → broadcasts StatusUpdate
- `DeviceHealth` enum: Online, Stale, Offline

### Phase 39 ✅ — Nostr publisher
- `NostrPublisher` — publishes events to Nostr relay
- Receipts + twin events → Nostr kind 31020/31030

### Phase 40 ✅ — Nostr relay (embedded)
- `NostrRelay` — in-process Nostr relay (WebSocket)
- `GET /nostr` — WS upgrade to Nostr relay
- NIP-01 basic protocol (REQ, EVENT, CLOSE)

### Phase 41 ✅ — Federation
- `FederationStore` — peer node registry
- `POST /federation/peers` — register peer
- `GET /federation/peers` — list federation peers
- `POST /federation/sync` — trigger sync with peer

---

## Protocol Additions

### Phase 42 ✅ — MCP server
- `mcp_server.rs` — Model Context Protocol server
- Exposes node capabilities as MCP tools (capture, query receipts, list devices)
- `GET /mcp` — MCP tool manifest

### Phase 43 ✅ — Vantage heartbeat
- `vantage_heartbeat.rs` — periodic heartbeat to Vantage DIP
- Background task polls Vantage, sends node status
- Wired into node startup

---

## CLI (sovereign-cli)

### Phase 44 ✅ — Basic CLI
- `devices` — list/register/remove
- `capture` — submit capture job
- `jobs` — list/poll jobs
- `receipts` — list/get/verify

### Phase 45 ✅ — Extended CLI
- `watch` — WebSocket live event stream
- `delegate` — delegate capture to peer
- `a2a` — agent/task/poll subcommands
- `finetune` — extract/stats/script subcommands
- `--format table` for device/job listing

---

## Mycelium Fine-tuning

### Phase 46 ✅ — Fine-tune pipeline
- `mycelium-finetune/src/main.rs`
- `extract` — parse Claude JSONL, deduplicate by SHA-256
- `stats` — show trace counts
- `script` — generate unsloth bash + train.py
- Outputs alpaca-format JSONL for QLoRA (Qwen/Llama 3B)

---

## Timeline Diff

### Phase 47 ✅ — Timeline diff endpoint
- `GET /twins/:id/timeline/diff` — compare two timeline snapshots
- Returns quality delta, modality changes, capture count diff
- Tests: 404 for unknown, requires two snapshots, computes delta

---

## Status

| Crate | Tests |
|---|---|
| sovereign-types | 19 ✅ |
| dip | 14 ✅ |
| vcp | 26 ✅ |
| twin-protocol | 6 ✅ |
| sovereign-pipeline | 12 ✅ |
| sovereign-a2a | — |
| sovereign-node | 98 ✅ |
| sovereign-cli | — |
| mycelium-finetune | — |

**All workspace tests green. No phases repeat beyond this line.**

---

## Unbuilt — Next Phases

### Phase 48 ✅ — Spatial twin diff (3D change detection)
- `spatial_diff.rs`: parse ASCII PLY, compute centroid/bbox/density per cloud
- `POST /twins/:id/splat/diff` — body: `{ snapshot_a: <base64 PLY>, snapshot_b: <base64 PLY> }`
- Returns: `point_count_delta`, `centroid_delta_m`, `volume_delta_m3`, `density_delta`, `change_magnitude` [0-1]
- 5 unit tests in spatial_diff.rs + 5 integration tests in api.rs

### Phase 49 ✅ — Swarm Gaussian splatting aggregation
- `swarm_splat.rs`: parse PLY, voxel-grid downsample, merge N clouds, write PLY
- `POST /swarm/:id/merge-splat` — body: `{ splats: [{device_id, ply_b64},...], voxel_size }`
- Returns merged PLY (base64), `input_points`, `output_points`, `reduction_pct`, `centroid`, `bbox_min/max`
- 8 unit tests in swarm_splat.rs + 6 integration tests in api.rs

### Phase 50 ✅ — On-chain tile governance (Sui Move)
- `twin-protocol/move/tile_governance/` — Move package: `claim_tile`, `stake_tile`, `record_capture`, `release_tile`
- `twin-protocol/src/tile_governance.rs` — `TileGovernanceAnchor` + `TileGovernanceConfig`
- `POST /tiles/:id/claim` — anchors claim on Sui, returns `tx_digest` + `object_id`
- `POST /tiles/:id/stake` — stakes ASE tokens, anchors on Sui
- Real Ed25519 Sui signing via `sui_rpc.rs`; graceful stub fallback when key absent
- 4 unit tests in tile_governance.rs + 5 integration tests in api.rs

### Phase 51 ✅ — OSOVM Token-of-Compute (ToC) — 3 wires
- Wire 1: `POST /osovm/gpu/contribute` → EmissionReceipt in `Simulation` pool
- Wire 2: `POST /osovm/gpu/burn` → EmissionReceipt in `Research` pool (Synapse = agent slice)
- Wire 3: `POST /osovm/gpu/decay` → EmissionReceipt in `LotteryBurn` pool; idempotent per epoch_day
- `GET /osovm/contributions` — list all GPU contribution records
- 5 integration tests (contribute receipt, burn receipt, decay idempotency, contributions list, receipt retrieval)

### Phase 52 ✅ — Mycelium QLoRA training run
- Enhanced `script` command: GGUF export block (merge LoRA → quantize via llama.cpp); `--quant q4_k_m|q5_k_m|q8_0|f16`; `--no-gguf` flag
- New `submit` subcommand: GPU.ai A40 job spec (`api.gpu.ai/v1/jobs`); `--dry-run` prints curl; `GPUAI_API_KEY` env support
- New `deploy` subcommand: generates Ollama Modelfile; `--dry-run` prints Modelfile; `--system` for system prompt; live path prints curl
- 6 new tests (gguf block, no_gguf flag, model/steps embed, Modelfile, quote escaping, job spec JSON)

### Phase 53 ✅ — A2A federation routing
- `federation_router.rs`: `FederationRouter` (round-robin + health-aware), `FederationPeer`, `PeerHealth`
- `POST /federation/peers` — register peer; `DELETE /federation/peers/:id` — remove
- `POST /federation/tasks` — route A2A task to best peer; 503 no peers, 404 unknown prefer, 502 all failed
- `POST /federation/health` — probe all peers, update health status
- 8 unit tests in federation_router.rs + 7 integration tests in api.rs
