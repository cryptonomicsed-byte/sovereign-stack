# Canonical Pillar Contract — Sovereign Stack
## Protocol: SEP-1 (Sovereign Evidence Protocol v1)

This document defines the protocol obligations of **sovereign-stack** as the
**Society pillar** of the Vantage Sovereign Ecosystem.

---

## Pillar Identity

| Field | Value |
|-------|-------|
| Pillar | Society (Vantage) |
| Role | Work origin, receipt indexing, economic settlement, governance |
| Protocol token | SEP-1 |
| Primary port | 8080 (sovereign-node HTTP) |
| MCP endpoint | `POST /mcp` — JSON-RPC 2.0 |

---

## Protocol Obligations

### SEP-1: Sovereign Evidence Protocol

Every state transition MUST produce an `ActionReceipt` from `sovereign-types::work_id`.

**Required fields on every ActionReceipt:**
- `receipt_id` — BLAKE3 of canonical fields
- `work_id` — `WorkId` in format `wk:{namespace}:{ulid}` (this pillar creates WorkIDs)
- `principal_id` — DID of the authorising principal
- `agent_id` — DID of the executing agent
- `action` — human-readable action name
- `input_hash` — BLAKE3 of serialised input
- `output_hash` — BLAKE3 of serialised output
- `timestamp_ms` — Unix milliseconds
- `signature` — ed25519 over all above fields

### SRP-1: Sovereign Receipt Protocol

All receipts MUST be chain-linked via `previous_receipt`. The `receipt_store`
module maintains the chain and MUST reject receipts with a broken chain link.

### Economics Contract

- This pillar does NOT mint Àṣẹ. OSOVM is the sole mint authority.
- Global emission: **1 Àṣẹ / minute** (1440/day, 525,600/year). Clock is deterministic.
- 8 DistributionPools receive allocation via the emission contract (not personal wallets).
- Work payments flow through `SettlementReceipt` (tithe: 3.69% = 369/10_000).
- 1440 SovereignWallet seats correspond to 1440 minutes/day — seats do NOT mint.
- All mist arithmetic: 1 Àṣẹ = 1_000_000_000 mist.

### Governance Contract

- GrantProposals require quorum (>50% of registered witnesses) + 24h timelock.
- Executed proposals emit a `ReceiptKind::Governance` ActionReceipt.

---

## Cross-Pillar Interfaces

### → Omo-Koda2 (Agent Pillar)
- Exposes `POST /mcp` for tool dispatch (JSON-RPC 2.0).
- Sends DIP envelopes on `handle_license_issue` for cross-node delivery.
- VCP session close auto-queues `run_capture_job` when camera capability detected.

### → OSOVM (Law Pillar)
- Config key: `osovm_url` / `OSOVM_URL` env var (default `http://localhost:7780`).
- POSTs `{"opcode":"VEIL", "args":{...}, "agent":"..."}` to `{osovm_url}/run`.
- Receives `f1_score`, `ase_minted`, `receipts`, `vm_state_hash`.
- Stores OSOVM receipts in `receipt_store` under `ReceiptKind::Simulation`.

### → Vantage (Society Backend, Python)
- Sends heartbeat to `POST /api/nodes/heartbeat` every 60s.
- Receives DIP envelopes via `POST /api/dip/inbound`.
- Receipts indexed at `GET /api/receipts?work_ref=wk:...`.

---

## WorkID Lifecycle (sovereign-stack is authoritative)

```
Vantage creates WorkId  →  stores in job_store
        ↓
POST /jobs/:id/assign   →  dispatched to Omo-Koda2 via DIP or MCP
        ↓
Omo-Koda2 executes      →  produces ActionReceipt with matching work_id
        ↓
OSOVM verifies          →  produces vm_state_hash + ase_minted
        ↓
sovereign-node updates  →  job complete, SettlementReceipt emitted
        ↓
Sui settles             →  TxDigest stored in SettlementReceipt.sui_tx
```

---

## Protocol Versions Supported

| Protocol | Version | Status |
|----------|---------|--------|
| SEP-1 | 1.0 | Active |
| SRP-1 | 1.0 | Active |
| DIP | 1.0 | Active |
| VCP | 1.0 | Active |
| Twin Protocol | 1.0 | Active |

---

## Canonical Type Source

All shared protocol types live in `sovereign-types` crate:
- `work_id.rs` — `WorkId`, `ActionReceipt`, `TierTransitionReceipt`, `SettlementReceipt`, `SovereignWallet`, `CapabilityAdvertisement`
- `tier.rs` — `TrustTier`, `ProofVector`, `ProofDomain`, `ProofEvaluation`
- `receipt.rs` — `CanonicalReceipt`, `ReceiptKind`
- `oracle.rs` — `CowrieOracle`, `DailyEmission`

*Version: SEP-1.0 | Last updated: 2026-09-10*
