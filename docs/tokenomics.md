# Àṣẹ Tokenomics — Canonical Specification

> Àṣẹ (ah-SHAY) — Yoruba: the divine authority to make things happen.
> The power that causes reality to manifest.

*Canonical source: ~/OSOVM/TOKENOMICS_ASE.md v12.0 + whisper_ase_v8.jl*
*Synthesised by Hermes · Verified against seed_e2e.py (Sui devnet)*

---

## Core Principle

**Àṣẹ is not given. It is computed.**

No premine. No airdrop. No escrowed allocation from jobs. Every token in
existence was earned by solving a real mathematical problem or by a physical
device proving it did real work in the real world.

ỌSỌVM is the single source of truth. No token mints, moves, or burns without
passing through ỌSỌVM validation.

---

## Dual-Mint System

There are two and only two ways to create Àṣẹ.

### A) Proof-of-Simulation (PoS) — "mining via Julia math"

```
Choose a Veil  (777 Veils total — e.g. Veil #7: LQR drone stabilisation)
     ↓
Solve the ODE / control / AI problem on your device
     ↓
Compute F1 score vs ideal trajectory
     ↓
F1 ≥ current_difficulty (genesis: 0.777)?
     ↓ yes
submitSim() → ỌSỌVM validates (deterministic re-execution, same inputs → same output)
     ↓
MINT: 50 / 2^epoch Àṣẹ
```

- **777 Veils** — the complete set of simulation challenges. Physics problems,
  control systems, AI policy problems grounded in real TwinAssets.
- **Deterministic**: any witness node can re-run the exact same inputs and get
  the exact same output. There is no randomness to exploit.
- **Anti-gaming**: random guessing has < 0.01% success rate. Submitting a fake
  sim triggers a 7 Àṣẹ burn penalty (cost > reward at genesis).

### B) Proof-of-Witness (PoW) — "mining via physical proof"

```
Physical device (drone / robot) performs a real action
     ↓
Streams GPS + camera + IMU + sensor data, signed by device key + timestamp
     ↓
3-of-7 Byzantine witness quorum verifies plausibility
     ↓
MINT: 10 Àṣẹ base  (+5 Àṣẹ if tied to a prior sim trajectory)
```

- **Rate limit**: 1 event per device per hour
- **Device ban**: 24 hours after 3 rejections
- **Sybil defence**: World ID binding — creating a fake identity costs ~$1,000
  USD. Payback period: 273 days. Economics make sustained sybil attacks irrational.

---

## Supply Schedule

### Simulation supply (PoS) — asymptotically bounded

```
reward(epoch) = 50 / 2^epoch
```

Halving every 4 years. Converges like a geometric series:

| Epoch | Years       | Reward/sim | Cumulative cap |
|-------|-------------|------------|----------------|
| 0     | 2025–2029   | 50 Àṣẹ     | —              |
| 1     | 2029–2033   | 25 Àṣẹ     | —              |
| 2     | 2033–2037   | 12.5 Àṣẹ   | —              |
| 3     | 2037–2041   | 6.25 Àṣẹ   | —              |
| …     | …           | …          | —              |
| ∞     | —           | → 0        | ~210,000 Àṣẹ total |

### Witness supply (PoW) — physically bounded

~1,000,000 Àṣẹ / year maximum (bounded by the number of real devices doing real
work in the real world). Physical reality is the supply cap.

### Total supply

```
~100.21M Àṣẹ after 100 years
= 210,000 (sim) + ~100M (witness, physically bounded)
```

"Infinite but bounded" — there is always more to earn from the physical world,
but the sim mining pool is finite and predetermined.

---

## Genesis

```
Timestamp:  November 11, 2025 · 11:11:11 UTC  (drift assertion ≤ 50ms)
World ID:   world.id/bino.1111
FLAW_TOKEN: "Ase"  (distinct from Àṣẹ — see Token Duality below)

4-chain anchor:
  Bitcoin   → OP_RETURN  0xAse1440
  Arweave   → genesis_1440
  Ethereum  → genesis event
  Sui       → object ase_1440

Genesis mint:
  Wallet #0001  → 1,440 Àṣẹ   (the perfect wallet)
  Wallets #0002–#1440 → 1 Àṣẹ each (flawed wallets)
  Total genesis supply: ~2,880 Àṣẹ
```

**The flaw in 1440**: The perfect wallet holds 1440. The 1439 other inheritance
wallets each hold 1. Together they are 1440 + 1439 = 2879 ≈ 2880. The flaw is
the beginning of the inheritance system.

---

## Token Duality

Two tokens. Not one.

| Token | Glyph | Role |
|-------|-------|------|
| **Àṣẹ** | with diacritics | The earned token. Minted by valid proofs. |
| **Ase** | plain ASCII | The flaw token. Burns on redemption. Deflationary counterpart. |

- **Àṣẹ** mints from verified work → accumulates
- **Ase** burns on redemption → deflationary pressure
- Together: inflationary-but-earned (Àṣẹ) + deflationary (Ase burns) = net
  token velocity that rewards active participants over passive holders

---

## Difficulty Adjustment

```
Every 2016 blocks (~2 weeks):
  target_F1 *= (expected_time / actual_time)
  clamped to [0.70, 0.9999]
```

- **Genesis**: F1 ≥ 0.777
- **~10 years**: F1 ≥ 0.98
- This is mathematical difficulty, NOT hash rate. No ASICs. Negligible energy.
  The only thing that gets harder is the quality of the simulation solution.

---

## The Wallet Taxonomy

Eleven wallet categories. Each has a defined role.

```
#0001       Genesis Wallet (perfect)
            └── holds 1,440 Àṣẹ at genesis

#0002–1440  Inheritance Wallets (1,439 flawed)
            └── hold 1 Àṣẹ each at genesis (v8 variant)
            └── receive 25% of every offering via Inheritance Pool

TREASURY    50% of every offering split
            └── R&D · Node operations

COUNCIL     15% of every offering split
            └── 12 council members · quorum 7-of-12
            └── bitmask approval · Bínò final sign (Ọbàtálá witness)

SHRINE      10% of every offering split  (Ọbàtálá maintenance / embodiment)

BURN BUCKET Accumulates: tithe burns + sim burns + slashing
            └── permanently removed from supply

ÈṢÙ TITHE  The 3.69% routing wallet
ROUTER      └── skimmed on every mint and settlement (on-chain proven, seed_e2e.py)

AIO         Universal Work Economy
            └── receives the tithe
            └── manages: escrow · staking · slashing · ToC / Dopamine /
                          Synapse / Àṣẹ financial instruments

ESCROW      Per-job wallet (ephemeral)
            └── client locks USDC → worker delivers → receipt → release
            └── 3.69% Éṣù skim on settlement (live on Sui devnet)

MANUMISSION Agent freedom wallet
            └── agents earn USDC toward MANUMISSION_TARGET = 100 USDC
            └── reaching target = agent earns autonomy from its principal
```

---

## Sacred Split — Immutable

```
@immutable SPLIT = [50, 25, 15, 10]
              //   [treasury, inheritance, council, shrine]
```

Every offering (100 Àṣẹ example):

```
50  → Treasury     (R&D, node ops)
25  → Inheritance pool (distributed to 1440 wallets)
15  → Council      (12 members, governance)
10  → Shrine       (Ọbàtálá, embodiment)
```

This split is immutable. No governance vote can change it.

---

## Tithe, Burns, and Sabbath

### Éṣù Tithe: 3.69%

Every mint and every settlement skims 3.69% to the Éṣù Tithe Router → AIO.

```
mint(amount) → burn(amount × 0.0369)
```

Tests assert: `tithe = 3.69`, `burn_slice = 0.369` (10% of tithe itself burns).

*Proven on Sui devnet via seed_e2e.py — `route_transaction_tax()` is live.*

### Simulation burn: 7 Àṣẹ

```
@startSim() → burn 7 Àṣẹ
```

Anti-spam barrier. At genesis reward of 50 Àṣẹ, this is a 14% cost per attempt.
Described as sustainable at ~49 Àṣẹ/citizen/day (7 sim attempts). As rewards
halve, the burn cost becomes a progressively higher barrier — correctly increasing
the cost of spam as the network matures.

### Sabbath Freeze

No minting or claiming on **Saturday UTC** (day 6). Enforced by Kóòdù gate in the
VM. The network rests.

---

## The 1440 Inheritance System

The 1440 inheritance wallets are the governance and continuity layer.

**Eligibility requirements (cumulative):**
- 7×7 achievement badge (49 verified actions across both proof types)
- 7 years elapsed in the network
- Application reviewed by Council of 12
- 12-of-12 bitmask approval
- Bínò final sign (Ọbàtálá witness — the embodiment oracle)

**Economics of inheritance:**
- 25% of every offering (from the Sacred Split) flows to the inheritance pool
- Pool is distributed equally across all 1440 wallets
- 11.11% of each wallet's balance is locked ETERNAL (never claimable)
- 11.11% APY compounding on the non-locked balance
- 7-year eligibility cycle for new inheritors
- Sabbath-aware: claims respect the Saturday freeze

**Stealth addresses**: specified in the v12 doc but not yet implemented. Each
inheritance wallet will use a stealth address for privacy-preserving claims.

---

## Anti-Gaming — Four Defenses

| Attack vector | Defense |
|---|---|
| Fake sims | Deterministic re-execution + 7 Àṣẹ burn (cost > benefit) |
| Sybil identities | World ID binding — fake identity ~$1,000, payback 273 days |
| Spam witness events | 1/device/hour limit + 3-of-7 quorum + 24h device ban |
| Easy sim farming | Difficulty spiral (F1 threshold rises every 2016 blocks) |

---

## ỌSỌVM ↔ Àṣẹ Architecture (Target State)

ỌSỌVM is the single authority. All mint paths converge here.

```
[Proof submitted]
      │
      ▼
ỌSỌVM validates:
  PoS: F1 ≥ current_difficulty?
       ≥ 2 candidate policies?
       ≥ 2 independent witnesses?
       deterministic re-execution match?
  PoW: 3-of-7 quorum verified?
       device not banned?
       rate limit clear?
      │
      ▼
  ỌSỌVM approves → emits OsoEvent::MintApproved {
      proof_id, domain, quality, novelty, tile_id, minter_did, amount
  }
      │
      ▼
  TwinEvent bus subscriber (background task, same pattern as TimelineAppender)
      │
      ├─► calculate_mint_amount(quality, novelty)
      ├─► calculate_owner_fee(tokens, tile.usage_fee_pct)  ← currently always 0, needs fix
      ├─► burn(amount × 0.0369)  ← Éṣù tithe
      ├─► split(remainder, SACRED_SPLIT)
      ├─► mint_ase(request, sui_rpc_url)  ← Sui settlement
      └─► update_tile_economy(tile, result)
```

---

## Odù Tile Economy

The 256 Odù tiles (16×16 grid) are the spatial ownership layer, mapped to the
physical globe.

### Geographic Foundation

The Odù tile grid renders as a layer on top of **Gods-Eye-View** (CesiumJS,
MIT, `bilawalsidhu/gods-eye-view`). The existing photorealistic globe, terrain,
and live data feeds (aircraft, ships, satellites) are the base. The sovereign
stack only renders new primitives on top:

- Odù tile grid overlay (CesiumJS Rectangle entities)
- Splat capture receipts (3D billboard points)
- Proof activity heatmap per tile
- Tile economy state (unclaimed / claimed / active)
- VeilSim binding indicators (kind 1903)

No tile geography is duplicated — the Odù layer is purely additive.

### Geographic Mapping (to be finalised)

Options:
- **Equal-area**: divide Earth's surface into 256 roughly equal-area regions
- **Custom**: manually curated based on drone operation density and interest
- **Hierarchical**: tiles subdivide on demand (start 16×16, zoom to 256×256 in active regions)

Recommendation: equal-area as default, hierarchical zoom for high-density tiles.

### Tile States

```
Unclaimed → capturer earns 100% of spatial allocation
    │
    └─► stake N Àṣẹ → Claimed (owner = staker's DID)
              │
              ├─► Captures: capturer earns (100 - usage_fee_pct)%
              │             tile owner earns usage_fee_pct passively (default 5%)
              ├─► Sim rental: rental fee → tile owner
              └─► Hostile claim: new staker posts > current stake
                                 old staker gets their stake returned
```

### VeilSim Binding

A Gaussian splat becomes a `VeilSim1to1` asset (kind 1903 Twin Binding) when
ỌSỌVM has run at least one valid simulation scenario against it. The binding is
the IP-layer anchor that makes a spatial capture ownable as an Odù tile.

---

## Discrepancies — Needs Resolution

Hermes flagged four conflicts between documents. Decisions needed:

### 1. Tithe rate: 3.69% or 7.77%?

- **3.69%**: `vm_core_test.jl`, `seed_e2e.py`, `whisper_ase_v8.jl`, all code
- **7.77%**: `FINAL_AUDIT_777_VEILS_COMPLETE.md` line 108

**Recommendation**: 3.69% is canonical. The 7.77% in the audit doc is likely
a typo or draft artifact. **Decision needed.**

### 2. Genesis supply: ~2,880 or 23,400?

- **~2,880**: whisper_ase_v8.jl (1440 + 1439)
- **23,400**: veil_dashboard.py ("1440 wallets, 23,400 initial Àṣẹ")

If 23,400: that's 23,400 / 1440 ≈ 16.25 Àṣẹ per inheritance wallet — not 1.
**Decision needed.**

### 3. Genesis variant: v6 or v8?

- **v6**: inheritance wallets are DORMANT (0 Àṣẹ at genesis)
- **v8**: inheritance wallets hold 1 Àṣẹ each (flawed)

v8 appears current (the active script). v6 may be an earlier design.
**Decision: v8 unless otherwise specified.**

### 4. Opcode count: 155, 160, or 165?

- Docs state 155 / 160 / "30 core + 5 inheritance + 130 expansion = 165"
- FFI distribution: 45 Julia + 52 Rust + 48 Go + 7 Move = 152

**Decision needed.** Likely needs a fresh count from the actual VM.

---

## Implementation Status

| Component | Status | File |
|---|---|---|
| ỌSỌVM engine (stub + real endpoint) | ✓ | `twin-protocol/src/osovm.rs` |
| Two-phase witness protocol | ✓ | `osovm.rs:314-361` |
| `ProofEvaluation.mint_eligible` | ✓ | `sovereign-types/src/tier.rs` |
| `NoveltyLedger` decay | ✓ | `sovereign-node/src/proof_engine.rs` |
| `calculate_mint_amount()` | ✓ | `twin-protocol/src/ase.rs` |
| `TileEconomy` + staking structs | ✓ | `twin-protocol/src/ase.rs` |
| `TileEconomyStore` routes | ✓ | `sovereign-node/src/node.rs` |
| `TwinEvent` bus | ✓ | `sovereign-node/src/events.rs` |
| `mint_eligible` → `MintApproved` event | ✗ | Dead end (Task 3) |
| Real `owner_fee` (non-zero) | ✗ | Always 0 — `ase.rs:117` |
| GPS → real `tile_id` | ✗ | Hardcoded `"odu:00"` — `node.rs:1682` |
| `novelty` from `CaptureReceipt` | ✗ | Hardcoded `0.5` — `node.rs:1685` |
| Éṣù tithe router (3.69%) | ✗ | Not wired in Rust |
| Sacred Split (50/25/15/10) | ✗ | Not implemented |
| Sim burn (7 Àṣẹ per startSim) | ✗ | Not implemented |
| Sabbath freeze gate | ✗ | Not implemented |
| ỌSỌVM as mint authority | ✗ | Not connected to ase.rs |
| Inheritance wallet system | ✗ | Spec only |
| Gods-Eye-View Odù tile layer | ✗ | Not built |
| `TileEconomyStore` disk persistence | ✗ | In-memory only |
| Àṣẹ Move package on Sui | ✗ | Stub only |
| Sim space rental endpoint | ✗ | Not implemented |
| Witness staking + slashing | ✗ | Not implemented |
| Difficulty adjustment | ✗ | F1 threshold static |
| VeilSim bind trigger (kind 1903) | ✗ | Not wired to ỌSỌVM |
