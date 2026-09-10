# Àṣẹ Tokenomics — Sovereign Stack

> Àṣẹ (ah-SHAY) — Yoruba: the divine authority to make things happen. The power that causes reality to manifest.

---

## Overview

Àṣẹ is the utility token of the Sovereign / Vantage network. It is not speculative — it is earned by doing useful work that extends the network's knowledge of the physical world, or that trains agents to operate more safely within it.

**ỌSỌVM is the single source of truth.** No tokens are minted, allocated, or distributed without ỌSỌVM validation. Every claim passes through the simulation VM's verification layer before anything settles on-chain.

---

## Emission Schedule

**1,440 Àṣẹ per day** — one per minute of real time.

This is a fixed, predictable supply. There are no halving events, no inflation multipliers, no governance votes to change the rate. 1,440/day is simple enough to reason about and deliberately maps to the minutes in a day — the network breathes once per minute.

```
Daily emission:    1,440 Àṣẹ
Weekly emission:   10,080 Àṣẹ
Annual emission:   525,600 Àṣẹ
```

The 1,440 daily tokens are routed through **Éṣù wallets** before reaching participants.

---

## Éṣù Wallets — The Routing Layer

In Yoruba cosmology, Éṣù stands at the crossroads — the guardian between worlds, the one who routes messages between realms. In the Sovereign network, Éṣù wallets are the **distribution nodes** that sit between the emission pool and participant wallets.

Each Éṣù wallet corresponds to a work category. The daily 1,440 Àṣẹ is split across Éṣù wallets by allocation weight, then distributed to participants within each category based on their validated contributions.

```
Daily Emission: 1,440 Àṣẹ
        │
        ├──► Éṣù:Spatial       (30% = 432/day)   — Gaussian splat captures
        ├──► Éṣù:Simulation    (25% = 360/day)   — Valid ỌSỌVM simulation runs
        ├──► Éṣù:Witness       (20% = 288/day)   — Consensus attestation + staking
        ├──► Éṣù:Physical      (15% = 216/day)   — Real-world sim→reality transfer
        ├──► Éṣù:Bounty        (05% = 72/day)    — Posted bounties and tasks
        └──► Éṣù:Reserve       (05% = 72/day)    — Protocol treasury + slashing pool
```

These weights are the initial configuration. They can be adjusted by governance but the total daily emission stays at 1,440.

---

## Work Categories

### 1. Spatial Captures — Éṣù:Spatial (30%)

**Who earns:** Anyone who submits a `GaussianProof` (Gaussian splat of a real physical space) that passes ỌSỌVM quality validation.

**What ỌSỌVM validates:**
- `GaussianQualityMetrics.aggregate() >= 0.5` — minimum quality floor
- `capture_completeness`, `geometric_consistency`, `photometric_quality` must all be above individual thresholds
- Novelty check: same `splat_hash` never earns full credit twice (`NoveltyLedger` decay)
- Area coverage: captures under 10m² earn reduced allocation (prevents micro-farming)

**Mint formula:**
```
quality_multiplier = 1.0 + (quality × 2.0)        — 1× to 3× base
novelty_multiplier = 1.0 + (novelty × 0.5)        — 1× to 1.5×
tokens_earned      = (daily_spatial_pool / captures_today) × quality × novelty
```

**VeilSim binding:** A Gaussian splat becomes a `VeilSim1to1` asset (kind 1903 Twin Binding) when ỌSỌVM successfully runs at least one simulation scenario against it. The binding is what makes the spatial asset ownable and stakeable as an Odù tile.

**Tile economics:**
- Each splat maps to one of the 256 Odù tiles (`OduCoordinate`)
- Unclaimed tile: 100% of allocation goes to the capturer
- Claimed tile (staked by owner): capturer receives `(100 - usage_fee_pct)%`, tile owner receives `usage_fee_pct` (default 5%)
- Tile staking model: see [Odù Tile Economy](#odù-tile-economy) below

---

### 2. Valid Simulations — Éṣù:Simulation (25%)

**Who earns:** Operators who run ỌSỌVM simulation scenarios against an existing `TwinAsset` and produce a valid `SimulationReceipt`.

This was the original Proof-of-Useful-Simulation vision: tokens for running valid sims. The "useful" requirement is what separates this from mining — the simulation must produce genuinely novel policy data that benefits an agent operating in the physical world.

**What ỌSỌVM validates:**
- `>= 2 candidate policies` explored (no single-policy cherry-picking)
- `>= 2 independent witnesses` attested to the Merkle commitment (consensus required)
- `selected_policy` exists in the committed policy set
- `ProofEvaluation.mint_eligible == true` — composite score must clear the bar
- `ProofEvaluation.novelty > 0.1` — below this floor, no mint (the environment is exhausted)

**Quality scoring:**
ỌSỌVM's `selected_policy.risk` and `trajectory_success_rate` feed into the quality signal:
```
sim_quality = (1.0 - selected_policy.risk) × trajectory_success_rate × novelty
```

**Sim space rental:**
Operators can also *rent* sim space — pay Àṣẹ to run simulations against TwinAssets they don't own. This creates two complementary flows:
- **Earn path:** Submit a valid `SimulationReceipt` → receive Àṣẹ from Éṣù:Simulation
- **Rent path:** Pay Àṣẹ to access a specific TwinAsset's spatial data for your own simulation run

The rent pool flows to the TwinAsset owner (via their Odù tile). This means high-quality spatial captures earn passively from rented sim access, creating a flywheel:

```
Better captures → more useful for sim → more rental demand → more income for capturer
                                      ↓
                             more Àṣẹ in Éṣù:Simulation pool
                                      ↓
                         incentivizes more simulation operators
```

---

### 3. Witness Consensus — Éṣù:Witness (20%)

**Who earns:** Nodes that attest to ỌSỌVM Merkle commitments as part of the `ProofOfSimulation` two-phase witness protocol.

**The two-phase protocol (already implemented in `osovm.rs`):**
1. Operator calls `run_and_commitment()` → gets `(OsovmRunResult, commitment_hash)`
2. Commitment is broadcast to potential witnesses via DIP
3. Witnesses verify the commitment and sign it with their DID keypair
4. Operator calls `prove_with_attestations()` with collected `WitnessAttestation`s
5. `SimulationReceipt` is built — witnesses earn from Éṣù:Witness pool

**Staking requirements:**
- Witnesses must stake Àṣẹ to participate — stake is the economic commitment to honest attestation
- Minimum stake: TBD (governance parameter)
- Stake locks for an epoch (1 week suggested) before it can be withdrawn

**Slashing conditions:**
- Attesting to an invalid commitment (caught by ỌSỌVM re-validation)
- Signing a commitment that conflicts with a prior attestation for the same run_id
- Going offline during an active attestation round (partial slash)

**Slashing mechanics:**
Slashed tokens go to Éṣù:Reserve — they do not get re-distributed to the slasher's competitors, preventing griefing incentives. Reserve can be used for protocol-level bounties or burned.

**Witness rewards:**
```
per_attestation_reward = (daily_witness_pool / total_attestations) × stake_weight
stake_weight           = min(2.0, 1.0 + stake_amount / base_stake)
```

Larger stakes earn proportionally more per attestation, up to a 2× cap (prevents plutocratic dominance).

---

### 4. Physical Transfer — Éṣù:Physical (15%)

**Who earns:** Operators who demonstrate that a simulation policy transferred successfully to physical hardware — measured by `RealityTransferScore`.

This is the highest-value work category because it closes the sim→real loop. A policy that works in ỌSỌVM *and* works on a real Go2 robot is proof that the simulation was grounded in reality, not just optimised for the virtual environment.

**What ỌSỌVM validates:**
- `RealityTransferScore.physical_proof_eligible == true` — RTS must clear 0.6 minimum
- `sim_proof_id` must reference a valid, previously submitted `SimulationReceipt` (sim must come before real flight)
- All 6 RTS axes must be measured: position, orientation, altitude, energy, collision margin, mission completion

**Bonus multiplier:**
Physical proofs that reference a sim proof in the same Odù tile earn a 20% bonus — incentivizes local sim-first workflows.

**RTS → quality mapping:**
```
physical_quality = rts.rts × rts.mission_transfer
physical_reward  = (daily_physical_pool / eligible_proofs) × physical_quality
```

---

### 5. Bounties — Éṣù:Bounty (5%)

**What bounties are:**
Protocol-level tasks posted by the network (or by token holders) for specific capture targets, simulation scenarios, or physical deployments. Examples:

- "Capture the interior of [GPS bounding box] with >= 0.8 quality" — spatial bounty
- "Run 50 simulation trajectories for Go2 in snowy terrain" — sim bounty
- "Achieve RTS >= 0.85 on a specific mission profile" — physical bounty

**Bounty mechanics:**
- Bounties are posted with an Àṣẹ reward (from the 5% daily pool or from token holder deposits)
- First valid submission that meets the spec claims the full reward
- ỌSỌVM validates all bounty claims — same quality gates as normal work categories
- Expired unclaimed bounties return to Éṣù:Reserve

**Why this matters:**
Bounties let the network direct capture and simulation work to where it's most needed — specific geographic tiles, specific robot models, specific environmental conditions — without requiring centralised coordination.

---

### 6. Reserve — Éṣù:Reserve (5%)

Accumulates from:
- Protocol allocation (5% of daily emission)
- Slashed witness stakes
- Expired unclaimed bounties
- Novelty-floor rejections that would have minted if novelty were higher (the "near miss" pool)

Used for:
- Protocol-level bug bounties
- Emergency bridging during low-activity periods
- Future governance decisions (burn, redirect, or new work categories)

---

## Odù Tile Economy

The 256 Odù tiles (16×16 grid mapped to physical geography) are the spatial ownership layer.

### Tile States

```
Unclaimed  →  anyone captures, 100% of spatial mint goes to capturer
     │
     └─► Claim: stake N Àṣẹ → tile becomes Claimed (owner = staker's DID)
                                                │
                                                ├─► Captures in tile: capturer earns (100 - fee)%
                                                │   tile owner earns fee% passively
                                                ├─► Sim rental in tile: rental fee to tile owner
                                                └─► Hostile claim: new staker posts > current stake
                                                    old staker gets their stake returned
```

### Tile Staking Formula

```
claim_cost  = base_stake × (1 + capture_count / 100)
```

Tiles with more captures are more expensive to claim — this reflects their established value and prevents early squatting on inactive tiles.

### Tile Revenue

Tile owners earn passively from:
1. `usage_fee_pct` on every spatial capture in their tile (default 5%)
2. Sim rental fees when others access the tile's TwinAsset data
3. Bounty fee if a bounty is completed in their tile (optional bounty configuration)

---

## ỌSỌVM as Mint Authority

All paths route through ỌSỌVM validation before any Àṣẹ is minted. The implementation target (Task 9 from the architecture audit) is an event-channel architecture:

```
[Proof arrives at /proof/* endpoint]
          │
          ▼
  ProofEngine::evaluate_*()
          │
          ▼
  mint_eligible == true?
          │ yes
          ▼
  TwinEvent::MintApproved {
      proof_id, domain, quality,
      novelty, tile_id, minter_did
  }
          │
          ▼
  [Background subscriber — same pattern as TimelineAppender]
          │
          ▼
  Determine Éṣù wallet by ProofDomain
          │
          ├─► Simulation → Éṣù:Simulation
          ├─► Spatial    → Éṣù:Spatial
          └─► Physical   → Éṣù:Physical
          │
          ▼
  AseMintRequest::from_event(event, tile_economy)
  calculate_mint_amount(quality, novelty)
  calculate_owner_fee(tokens, tile.usage_fee_pct)
          │
          ▼
  mint_ase(request, sui_rpc_url)  [Sui settlement]
          │
          ▼
  update_tile_economy(tile, result)
  TileEconomyStore::apply_mint()
```

No proof bypasses ỌSỌVM. The simulation VM's output is what authorises the settlement, not just the node's local computation.

---

## Anti-Farming Measures

| Attack vector | Mitigation |
|---|---|
| Repeated same splat_hash | `NoveltyLedger` decay — 1/√n per submission |
| Tiny area captures | Area floor: < 10m² earns 0.1× multiplier |
| Fake witnesses | Staking requirement + slashing on invalid attestation |
| Cherry-picked single-policy sims | ỌSỌVM enforces >= 2 candidate policies |
| Solo witness | ỌSỌVM enforces >= 2 independent witnesses |
| Environment novelty exhaustion | Novelty floor < 0.1 = zero mint (hard stop) |
| Sim without spatial grounding | `ProofOfSimulation` requires a valid `TwinAsset` |
| Physical claim without prior sim | `physical_proof_eligible` requires `sim_proof_id` reference |

---

## Token Utility (Demand Side)

Without demand sinks, even a fixed emission becomes inflationary in practice. Planned utility:

| Usage | Àṣẹ consumed |
|---|---|
| Rent sim space against a TwinAsset | Per-run fee (to tile owner) |
| Stake a tile claim | Locked (refundable on release) |
| Post a bounty | Locked until claimed or expired |
| Stake as a witness | Locked for epoch duration |
| Purchase access to closed spatial data | To IP layer owner |
| Agent deployment (future) | Gas-equivalent for sovereign node compute |

---

## Open Questions

1. **Sim rewards vs. sim rental — should both exist?** Current recommendation: yes. Earners and renters are different users with different incentives. Rental creates passive income for quality captures; rewards create active incentives for sim operators. Both are needed.

2. **1440 initial allocation weights** — The 30/25/20/15/5/5 split is a starting hypothesis. Physical transfer work (15%) may need to be higher once RTS infrastructure is live — it's the hardest work and should pay the most. Consider 20% Physical / 20% Witness and reduce Simulation slightly.

3. **Witness minimum stake** — Needs to be high enough to make slashing hurt but low enough that new nodes can participate. Suggest governance parameter starting at 100 Àṣẹ (roughly 25 days of individual earnings at average participation rate).

4. **Tile count** — 256 tiles at 16×16 is good for a first world. The geographic mapping (each tile = what real-world bounding box?) needs to be defined. Options: equal-area hex grid, country-based, custom.

5. **Epoch length** — Weekly epochs (7 days) suggested for witness staking. Daily epoch for Éṣù distribution. Both can be changed by governance.

---

## Implementation Status

| Component | Status |
|---|---|
| `calculate_mint_amount()` | ✓ Implemented (`ase.rs`) |
| `TileEconomy` + staking | ✓ Implemented (`ase.rs`, `tile_economy_store.rs`) |
| `ProofEvaluation.mint_eligible` | ✓ Computed (`tier.rs`) |
| `NoveltyLedger` decay | ✓ Implemented (`proof_engine.rs`) |
| Two-phase witness protocol | ✓ Implemented (`osovm.rs`) |
| `TwinEvent` bus | ✓ Implemented (`events.rs`, `node.rs`) |
| Éṣù wallet routing | ✗ Not yet implemented |
| `mint_eligible` → `MintApproved` event | ✗ Dead end (Task 3) |
| Real `owner_fee` (non-zero) | ✗ Always 0 (Task 4) |
| GPS → `tile_id` derivation | ✗ Hardcoded `"odu:00"` (Task 2) |
| `novelty` from CaptureReceipt | ✗ Hardcoded `0.5` (Task 1) |
| `TileEconomyStore` disk persistence | ✗ In-memory only (Task 7) |
| Àṣẹ Move package on Sui | ✗ Stub only (Task 6) |
| Sim space rental endpoint | ✗ Not yet implemented |
| Bounty system | ✗ Not yet implemented |
| Witness slashing | ✗ Not yet implemented |
