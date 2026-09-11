# ỌSỌVM Canonical Architecture
**LOCKED 2026-09-10 · Constitutional document. All implementations conform to this.**

---

## What ỌSỌVM Is

**ỌSỌVM = CORE + RUNTIME + VEIL + SETTLEMENT**

It is the single mint authority for Àṣẹ. No token enters existence without ỌSỌVM validation.

```
┌─────────────────────────────────────────────────────┐
│                      ỌSỌVM                          │
│                                                     │
│  ┌──────────┐  ┌──────────┐  ┌──────────────────┐  │
│  │  CORE    │  │ RUNTIME  │  │      VEIL        │  │
│  │ 155 ops  │  │ orchestr │  │    622 ops       │  │
│  │ determin │  │ receipts │  │  intelligence    │  │
│  └──────────┘  └──────────┘  └──────────────────┘  │
│                                                     │
│  ┌─────────────────────────────────────────────┐    │
│  │            SETTLEMENT (Sui)                 │    │
│  │  MintAuthorization → on-chain Àṣẹ           │    │
│  └─────────────────────────────────────────────┘    │
└─────────────────────────────────────────────────────┘
```

---

## Locked Layer Definitions

### CORE
- **155 deterministic law opcodes**
- Handles: consensus, receipt validation, proof verification, economic rules
- Deterministic: same inputs → same outputs, always, on every node
- Governs: what counts as valid work, tithe calculation, sector splits

### RUNTIME
- Orchestration layer — schedules proofs, manages job lifecycle
- Issues canonical receipts (JobReceipt, MintReceipt, AttestationReceipt)
- Hosts the **DailyEmissionAllocator** — the bridge between proof evaluation and mint authorization

### VEIL
- **622 intelligence opcodes**
- Non-deterministic reasoning, simulation execution, AI evaluation
- Produces: F1 score, ProofValue, trajectory quality metrics
- Does NOT mint. Does NOT issue receipts. VEIL outputs feed into CORE verification.

### SETTLEMENT
- Sui blockchain
- Receives **MintAuthorization** from RUNTIME → executes `mint_ase()`
- Final anchor for all economic outcomes

---

## The 777 Opcode Space

```
155 CORE opcodes
+ 622 VEIL opcodes
= 777 total VM opcode space
```

777 is the opcode count. It is NOT the number of Veil Challenges.

---

## Three Meanings of "Veil" — Do Not Conflate

| Term | What It Is |
|---|---|
| **VEIL (layer)** | 622 intelligence opcodes in the VM |
| **Veil Challenges** | 777 executable workloads (ODE / control / AI problems). Named `@veil(category, fn, modifier)`. |
| **VeilSim** | Software: execution/visualization worker for Veil Challenges |

---

## VeilSim Studio

**VeilSim Studio is NOT ỌSỌVM.**

It is an execution worker and developer interface for the VEIL layer.

```
VeilSim Studio role:
  - Accepts Veil Challenge assignments
  - Runs trajectory rollouts
  - Returns F1 score + ProofValue
  - FastAPI endpoint at :8788 during development

VeilSim Studio does NOT:
  - Issue MintAuthorizations
  - Write receipts
  - Access the Sui settlement layer
  - Replace or substitute for ỌSỌVM CORE
```

---

## Proof ≠ Mint

This is the most critical invariant in the system.

```
Proof     = evidence that real work occurred
Mint      = new Àṣẹ entering existence

A valid proof authorizes participation in the daily emission allocation.
A valid proof does NOT directly create tokens.
```

### MintEligibility ≠ MintAuthorization

```
MintEligibility:  proof quality ≥ threshold (F1 >= current_difficulty)
                  → the prover MAY receive a share of that minute's emission

MintAuthorization: ỌSỌVM RUNTIME has computed the prover's allocation
                   → SETTLEMENT executes the on-chain mint
```

`mint_eligible = true` is an input to the DailyEmissionAllocator.
It is NOT a mint command.

---

## The Canonical Proof → Emission Flow

```
Proof Submitted
     │
     ▼
ProofEvaluation
  ├── F1 score       (economic weight — participation share)
  └── ProofValue     (proof strength — tier progression)
     │
     ▼
MintEligibility Gate
  (F1 >= current_difficulty, currently 0.777)
     │ eligible
     ▼
DailyEmissionAllocator          ← lives in ỌSỌVM RUNTIME
  Input:  all eligible proofs for this minute
  Logic:  allocate 1 Àṣẹ proportional to (F1 × novelty_weight)
  Output: Allocation per prover
     │
     ▼
MintAuthorization
  (signed by ỌSỌVM, passed to Sui)
     │
     ▼
Sui Settlement → mint_ase()
     │
     ▼
MintReceipt (canonical, archived)
```

---

## F1 Score vs ProofValue — Separation of Concerns

| Field | Role |
|---|---|
| `f1_score` | Economic weight. Determines share of daily emission. |
| `proof_value` | Proof strength. Determines Trust Tier progression (T1→T5). |

These are computed together but serve different purposes. Never use one as a proxy for the other.

---

## Attestation Terminology

| Type | What It Attests |
|---|---|
| `SimulationAttestation` | Verifies a simulation commitment (hash + parameters) |
| `PhysicalAttestation` | Verifies a physical world event (GPS + sensor + device key) |
| `GaussianAttestation` | Verifies a Gaussian splat reconstruction |
| `JobAttestation` | Verifies a job pipeline completed |

**Proof-of-Simulation ≠ Physical Proof-of-Work.** These are separate proof domains with separate attestation paths.

---

## Spatial Foundation

**The Spatial Foundation sits ABOVE ỌSỌVM in the architecture.**

```
Physical World
     │  (GPS, sensors, devices, drones)
     ▼
Spatial Foundation
  (OduCoordinate tiles, Gaussian splats, reality-derived proofs)
     │  (proofs feed into)
     ▼
ỌSỌVM
  (validates proofs → emission allocation → settlement)
     │
     ▼
Sui (Àṣẹ)
```

ỌSỌVM does NOT own physical reality. It governs what happens when reality-derived work becomes proof and economic value.

---

## The Three Economic Flows

These are distinct. Do not conflate.

### Flow 1 — Daily Emission
```
1,440 Àṣẹ/day — FIXED FOREVER
  → main Éṣù wallet
  → Éṣù-Elegbára router → 8 sub-wallets
  (allocated by DailyEmissionAllocator per minute)
```

### Flow 2 — Universal Transaction Tithe
```
3.69% on every mint, payment, settlement — LOCKED
  → same Éṣù-Elegbára router → same 8 sub-wallets
```

### Flow 3 — 24-Sector Funding Split
```
When a sector wallet receives inflow (donations/investments/hardware/embodiment):
  50% Treasury · 25% Inheritance · 15% Council · 10% Executor
  (6 Great Houses × 4 categories = 24 sector wallets)
  Does NOT apply to daily emission or tithe
```

---

## The 4-Repo Organizational Boundary

| Repo | Domain | Contains |
|---|---|---|
| `sovereign-stack` | Reality | Spatial capture, device protocols, twin pipeline |
| `ScarabSwarm` / `VeilSim` | Laboratory | Simulation workers, Veil Challenge execution |
| `Omo-Koda2` | Agent | Agent runtime, trust tiers, manumission |
| `ỌSỌVM` | Law | CORE opcodes, RUNTIME orchestrator, settlement bridge |

Cross-repo calls go through defined protocol interfaces. No repo reaches into ỌSỌVM internals directly.

---

## StampFly Witness Beacon

StampFly drones are the **Witness Beacon network** for Physical Proof-of-Work.

```
WitnessPulse      — heartbeat proving the device is live and reachable
WitnessFlight     — 10–30 sec verification flight triggered by agent request
WitnessAttestation — signed evidence artifact produced after flight

Witness Policy Engine — safety gate; decides whether a WitnessFlight is allowed
  (battery, airspace, proximity, flight history)
```

A `PhysicalAttestation` requires a `WitnessAttestation` from a 3-of-7 Byzantine quorum.

---

## Supply Model — LOCKED

```
1,440 Àṣẹ/day — no halving, no cap, fixed forever
Annual: 525,600 Àṣẹ/year

Genesis supply: 2,880 tokens (2 days of emission)
  1 Àṣẹ  — transferable, genesis wallet #0001
  1,439 Ase — soul-bound flaw tokens, inheritance wallets #0002–#1440
```

---

## Invariant Commons

These fractions appear in **every** split path:

```
3.69%  Éṣù tithe       — universal, always first, always through Elegbára router
11.11% inheritance     — always present in downstream distributions
```

---

## Implementation Conformance

Any component that does any of the following is **non-conformant** and must be corrected:

- Calls `mint_ase()` directly from a proof handler (bypasses DailyEmissionAllocator)
- Uses `mint_eligible = true` as a direct mint instruction
- Treats VeilSim Studio as ỌSỌVM
- Uses "Veil" to mean both the opcode layer and the challenge workloads
- Uses F1 score as a proxy for ProofValue or vice versa
- Applies the 50/25/15/10 sector split to daily emission or per-transaction tithe
- Mints on Saturday UTC (Sabbath freeze)
- Mints without ỌSỌVM CORE validation
