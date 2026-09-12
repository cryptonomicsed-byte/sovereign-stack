# Sovereign Ecosystem — Repo Playbook
# THE canonical answer to "where does this code go?"
# Updated: 2026-09-12

---

## THE ONE RULE

**sovereign-stack = protocol crates only.**
It is connective tissue between 32 repos — not an application.
If you are writing business logic, stores, economy, governance, or perception loops
and you think "I'll put it in sovereign-node" — you are wrong. Use this playbook.

---

## REPO MAP — WHAT GOES WHERE

### sovereign-stack (THIS REPO)
**Role:** Protocol crate library. Imported by every other repo.
**Lang:** Rust

| Crate | Contains | Does NOT contain |
|---|---|---|
| `sovereign-types` | Primitives: OduCoordinate, IdentityChain, TrustTier, CaptureReceipt, DistributionPool, crypto | Business logic, stores, HTTP |
| `dip` | DIP envelope, DipRouter, adapters (Nostr/A2A/MCP/Mesh) | Application routing, HTTP server |
| `vcp` | DeviceSession, VcpCommand, SessionStore | HTTP handlers, perception loops |
| `twin-protocol` | CaptureReceipt/SceneReceipt/SimReceipt types, SuiAnchor, TwinTimeline types, ASE math | Stores, HTTP, wallets |
| `sovereign-pipeline` | Gaussian splat pipeline driver, CaptureJob, PLY diff + merge logic | HTTP server, swarm coordination |
| `sovereign-a2a` | A2A v1.0 types, task router, AgentCard | Application-level agents, business dispatchers |
| `sovereign-node` | **THIN REFERENCE DAEMON ONLY** — protocol integration tests, `/capture`, `/dip`, `/a2a` demo | Everything else below |

---

### Vantage (Python FastAPI, port :8001)
**Role:** Civilisation interface. ~700 endpoints. Agent-first. The economic + governance brain.
**Repo:** `Vantage`

**Owns:**
- Wallet store + ASE balances (`wallet_store` → `vantage/economy/wallet.py`)
- Tile economy + staking (`tile_economy_store` → `vantage/economy/tiles.py`)
- Emission receipts + distribution clock (`emission_receipt_store` → `vantage/economy/emission.py`)
- Governance proposals + voting (`governance_store` → `vantage/governance/proposals.py`)
- Council of 12 (`council_store` → `vantage/governance/council.py`)
- Sovereign seat assignments (`sovereign_seat_store` → `vantage/governance/seats.py`)
- License store (`license_store` → `vantage/rights/licenses.py`)
- Agent registry (`agent_store` → `vantage/agents/registry.py`)
- Body store / physical embodiment (`body_store` → `vantage/physical/bodies.py`)
- Telemetry ingestion (`telemetry_store` → `vantage/observability/telemetry.py`)
- Witness registry (`witness_registry` → `vantage/receipts/witnesses.py`)
- Receipt store + merkle (`receipt_store`, `receipt_merkle` → `vantage/receipts/`)
- MCP server exposing Vantage tools (`mcp_server` → `vantage/mcp/server.py`)
- Nostr publisher for receipts (`nostr_publisher` → `vantage/nostr/publisher.py`)
- Timeline store — viewing/querying 4D twin history (`timeline_store` → `vantage/twins/timeline.py`)
- Swarm capture coordination (`swarm` → `vantage/capture/swarm.py`)

---

### OSOVM (Julia)
**Role:** Heart VM. Proof-of-Useful-Simulation. Economic engine.
**Repo:** `OSOVM`

**Owns:**
- Proof engine / zero-knowledge proof stubs (`proof_engine` → `OSOVM/src/proof/`)
- Simulation scoring (`simulation_scoring` → `OSOVM/src/scoring/`)
- GPU pool / Token-of-Compute (`gpu_pool` → `OSOVM/src/toc/gpu_pool.jl`)
- OSOVM opcodes, scenario runner, witness attestation

---

### Omo-Koda2 (Rust + Go + Elixir + Julia, port :7777)
**Role:** Sovereign agent OS. The living organism layer.
**Repo:** `Omo-Koda2`

**Owns:**
- Active perception loop (`perception` → `omo-koda2/src/vcp/perception.rs`)
- Federation routing + mDNS peer discovery (`federation_router` → `omo-koda2/src/mesh/federation.rs`)
- Vantage heartbeat + DIP poll (`vantage_heartbeat` → `omo-koda2/src/vantage/heartbeat.rs`)
- Identity store (DID resolution, sovereign identity) (`identity` → `omo-koda2/src/identity/`)
- Delegation — forwarding capture tasks to peers (`delegation` → `omo-koda2/src/a2a/delegate.rs`)
- VCP device birth, discovery daemon
- Job orchestration (spawning capture + simulation jobs)

---

### ip-layer (Rust)
**Role:** Nostr IP provenance. Every agent is born with one.
**Repo:** `ip-layer`

**Owns:**
- Nostr relay (embedded, NIP-01) (`nostr_relay` → `ip-layer/src/relay/`)
- Nostr publisher (`nostr_publisher` → `ip-layer/src/publisher/`)
- kind 31900 IP Root, 1901 Creation Receipt, 1902 Attestation, 1903 Twin Binding
- `seal_gaussian_splat()` — already lives here correctly

---

### sovereign-pipeline (crate in sovereign-stack)
**Role:** Gaussian splat capture pipeline. Hardware → PLY → receipt chain.

**Owns (moves here from sovereign-node):**
- `spatial_diff.rs` — PLY point cloud diff
- `swarm_splat.rs` — merge N device PLY clouds (voxel-grid downsample)

---

### twin-protocol (crate in sovereign-stack)
**Role:** TSP receipt types, Sui anchoring, timeline primitives.

**Owns (already correctly placed):**
- `TwinTimeline`, `TwinTimelineEntry` — timeline *types* (the store lives in Vantage)
- `tile_governance.rs` — Sui tile anchor client
- `tile_governance/` Move contracts

---

### mycelium / mycelium-tools (Python)
**Role:** Stigmergic trace substrate. Fine-tuning pipeline.
**Repos:** `mycelium`, `mycelium-tools`

**Owns:**
- `mycelium-finetune` crate logic → move to `mycelium-tools` as a Python script + CLI
- Fine-tune extract/stats/script/submit/deploy → `mycelium-tools/tools/finetune/`

---

### Sovereign Contracts (new repo needed)
**Role:** On-chain Move + Sui smart contracts.
**Repo:** Create `sovereign-contracts`

**Owns:**
- `twin-protocol/move/tile_governance/` → `sovereign-contracts/tile_governance/`
- Future: twin_nft.move, ase_token.move, council_nft.move

---

### Zàngbétò (Rust + Node.js)
**Role:** Red-team audit + receipt anchoring.
**Repo:** `Zangbeto`

**Owns:**
- Emission receipt Zàngbétò signing (witness every emission on-chain)
- Protocol audit receipts

---

## DECISION FLOWCHART

```
Is it a shared primitive type (struct/enum, no HTTP, no DB)?
  → YES → sovereign-types

Is it DIP envelope routing logic?
  → YES → dip crate

Is it VCP device session / capability negotiation?
  → YES → vcp crate

Is it a TSP receipt TYPE or Sui anchor client?
  → YES → twin-protocol crate

Is it the Gaussian splat capture pipeline or PLY processing?
  → YES → sovereign-pipeline crate

Is it A2A v1.0 task wire types or generic A2A router?
  → YES → sovereign-a2a crate

Is it economy — wallets, tiles, ASE, emission?
  → YES → Vantage

Is it governance — proposals, council, seats, licenses?
  → YES → Vantage

Is it proof-of-simulation or scoring?
  → YES → OSOVM

Is it agent OS behaviour — perception, federation, heartbeat, delegation?
  → YES → Omo-Koda2

Is it Nostr relay or IP provenance events?
  → YES → ip-layer

Is it fine-tuning / ML tooling?
  → YES → mycelium-tools

Is it a Move smart contract?
  → YES → sovereign-contracts (new repo)

Is it a demo integration test showing protocols working together?
  → YES → sovereign-node (thin reference daemon)
```

---

## MIGRATION PLAN — WHAT TO REMOVE FROM sovereign-node

These files exist in `sovereign-node/src/` but do NOT belong there.
Each row shows the file, where it goes, and priority.

| File | Target repo | Target path | Priority |
|---|---|---|---|
| `governance_store.rs` | Vantage | `vantage/governance/proposals.py` | P1 |
| `council_store.rs` | Vantage | `vantage/governance/council.py` | P1 |
| `wallet_store.rs` | Vantage | `vantage/economy/wallet.py` | P1 |
| `emission_receipt_store.rs` | Vantage | `vantage/economy/emission.py` | P1 |
| `sovereign_seat_store.rs` | Vantage | `vantage/governance/seats.py` | P1 |
| `tile_economy_store.rs` | Vantage | `vantage/economy/tiles.py` | P1 |
| `agent_store.rs` | Vantage | `vantage/agents/registry.py` | P1 |
| `license_store.rs` | Vantage | `vantage/rights/licenses.py` | P1 |
| `receipt_store.rs` | Vantage | `vantage/receipts/store.py` | P1 |
| `receipt_merkle.rs` | Vantage | `vantage/receipts/merkle.py` | P1 |
| `timeline_store.rs` | Vantage | `vantage/twins/timeline.py` | P1 |
| `witness_registry.rs` | Vantage | `vantage/receipts/witnesses.py` | P1 |
| `body_store.rs` | Vantage | `vantage/physical/bodies.py` | P2 |
| `telemetry_store.rs` | Vantage | `vantage/observability/telemetry.py` | P2 |
| `swarm.rs` | Vantage | `vantage/capture/swarm.py` | P2 |
| `mcp_server.rs` | Vantage | `vantage/mcp/server.py` | P2 |
| `proof_engine.rs` | OSOVM | `OSOVM/src/proof/` | P1 |
| `simulation_scoring.rs` | OSOVM | `OSOVM/src/scoring/` | P1 |
| `gpu_pool.rs` | OSOVM | `OSOVM/src/toc/gpu_pool.jl` | P1 |
| `perception.rs` | Omo-Koda2 | `omo-koda2/src/vcp/perception.rs` | P1 |
| `federation_router.rs` | Omo-Koda2 | `omo-koda2/src/mesh/federation.rs` | P1 |
| `federation.rs` | Omo-Koda2 | `omo-koda2/src/mesh/mdns.rs` | P1 |
| `vantage_heartbeat.rs` | Omo-Koda2 | `omo-koda2/src/vantage/heartbeat.rs` | P1 |
| `identity.rs` | Omo-Koda2 | `omo-koda2/src/identity/` | P2 |
| `delegation.rs` | Omo-Koda2 | `omo-koda2/src/a2a/delegate.rs` | P2 |
| `nostr_relay.rs` | ip-layer | `ip-layer/src/relay/` | P1 |
| `nostr_publisher.rs` | ip-layer | `ip-layer/src/publisher/` | P1 |
| `spatial_diff.rs` | sovereign-pipeline | `sovereign-pipeline/src/diff.rs` | P2 |
| `swarm_splat.rs` | sovereign-pipeline | `sovereign-pipeline/src/merge.rs` | P2 |

**What stays in sovereign-node after migration:**
```
config.rs         — node config loading
dip_gateway.rs    — /dip/inbound + /dip/gossip relay handlers
events.rs         — TwinEvent broadcast types
jobs.rs           — capture job state machine
node.rs           — thin router: /capture, /dip, /a2a, /vcp, /status
vantage.rs        — Vantage DIP poll client (outbound only)
ws.rs             — WS upgrade for twin event stream
main.rs           — entry point
lib.rs            — exports for integration tests
```

---

## CURRENT SOVEREIGN-NODE LEGITIMATE ROUTES (after cleanup)

```
GET  /status                  — node health
POST /capture                 — demo: VCP + TSP + pipeline
GET  /jobs/:id                — poll capture job
GET  /devices                 — VCP registered devices
POST /devices/register        — register a device
POST /dip/inbound             — relay inbound DIP envelope
POST /dip/gossip              — gossip DIP envelope to peers
GET  /dip/did                 — node DID
GET  /a2a/*                   — A2A v1.0 protocol (from sovereign-a2a router)
GET  /vcp/sessions            — active VCP sessions
POST /vcp/sessions            — open VCP session
DELETE /vcp/sessions/:id      — close VCP session
GET  /ws/twin/:id             — WebSocket twin event stream
GET  /receipts/:id            — lookup a single capture receipt (read-only)
```

Everything else — governance, wallets, tiles, proofs, swarm, timeline, nostr, federation,
body sessions, GPU pool, MCP — routes through Vantage (:8001), OSOVM, or Omo-Koda2 (:7777).

---

## HOW TO USE THIS PLAYBOOK

1. **Before writing any code:** check the decision flowchart above.
2. **Before adding to sovereign-node:** ask "is this a protocol demo or application logic?" If application → redirect.
3. **When in doubt:** check `~/sovereign-eco-blueprint/plans/full-ecosystem-map.md` for repo roles.
4. **When building a new feature:** open the target repo first. If it doesn't exist, create the file there.
