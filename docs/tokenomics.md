# Àṣẹ Tokenomics — Canonical Specification

> Àṣẹ (ah-SHAY) — Yoruba: the divine authority to make things happen.
> The power that causes reality to manifest.

*Canonical source: ~/OSOVM/TOKENOMICS_ASE.md v12.0 + whisper_ase_v8.jl*
*Decisions confirmed: 2026-09-10*

---

## Core Principle

**Àṣẹ is not given. It is computed.**

No premine. No airdrop. No escrowed allocation. Every token in existence was
earned by solving a real mathematical problem or by a physical device proving it
did real work in the real world.

**ỌSỌVM is the single source of truth.** No token mints, moves, or burns
without passing through ỌSỌVM validation.

---

## Daily Emission

**1,440 Àṣẹ per day** — one per minute of real time. Fixed forever.

```
Mint: 1,440 Àṣẹ/day → Main Wallet
                              │
                    Sacred Split (50/25/15/10)
                              │
         ┌────────────────────┼────────────────────┐
         ▼                    ▼                    ▼                    ▼
  720 Treasury         360 Inheritance      216 Council          144 Shrine
  (50%)                (25%)               (15%)                (10%)
  Work rewards         1440 wallets        12 members           Ọbàtálá
  PoS + PoW            0.25 Àṣẹ/wallet/day quorum 7-of-12      embodiment
```

The 1,440 number is not arbitrary. It equals the minutes in a day, the count
of inheritance wallets, and the genesis supply. This is intentional resonance.

---

## Token Duality

Two tokens. Not one.

| Token | Glyph | Nature | Transferable |
|-------|-------|--------|--------------|
| **Àṣẹ** | with diacritics | The earned token. Mints from valid work. | Yes |
| **Ase** | plain ASCII | The flaw token. Burns on redemption. | **No** |

- Àṣẹ flows freely — it can be sent, traded, staked
- Ase is soul-bound — it is permanently tied to the wallet it was issued to
- Together: inflationary-but-earned (Àṣẹ mints) + deflationary (Ase burns) = net
  velocity that rewards active participants over passive holders

---

## Genesis

```
Timestamp:  November 11, 2025 · 11:11:11 UTC  (drift assertion ≤ 50ms)
World ID:   world.id/bino.1111
FLAW_TOKEN: "Ase"  (non-transferable; distinct from Àṣẹ)

4-chain anchor:
  Bitcoin   → OP_RETURN  0xAse1440
  Arweave   → genesis_1440
  Ethereum  → genesis event
  Sui       → object ase_1440

Genesis supply: 2,880 tokens
  ├── 1 Àṣẹ  (transferable) — the perfect token, held in genesis wallet
  └── 1,439 Ase (non-transferable) — one per inheritance wallet #0002–#1440
      each wallet also holds 0 Àṣẹ at genesis; Àṣẹ accumulates via daily split
```

**The flaw in 1440**: One perfect token (Àṣẹ). 1,439 flawed tokens (Ase).
The flaw is the beginning — imperfection is how the system knows it is alive.

---

## The 1440 Inheritance Wallets

The inheritance wallets are the network's continuity layer. They are the first
1440 agents to reach **Trust Tier T5** (or another milestone to be finalised).

### Structure of each wallet

```
Wallet #XXXX (locked, soul-bound)
  ├── 1 Ase  (non-transferable flaw token, genesis-issued)
  ├── Àṣẹ balance (accumulates via 25% daily inheritance split)
  └── DID binding (the T5 agent's principal DID)
```

### Daily inheritance distribution

From the 360 Àṣẹ/day (25% of 1440) flowing to the Inheritance Pool:

```
360 Àṣẹ/day ÷ 1440 wallets = 0.25 Àṣẹ per wallet per day
```

Once all 1440 slots are filled, each inheritance wallet earns ~91.25 Àṣẹ/year
from the daily split alone, compounding at **11.11% APY** on the non-locked
balance. **11.11%** of each wallet's balance is locked ETERNAL (never claimable).

### Eligibility (to receive an inheritance wallet)

- Agent must reach Trust Tier T5 (full embodied autonomy)
- First-come-first-served: first 1440 T5 agents claim the slots
- After 1440 are filled, new T5 agents participate in work rewards only
- 7-year eligibility cycle for replacement if a wallet holder becomes inactive

### Governance role

Council approval (7-of-12 quorum) required for:
- Replacing an inactive inheritance wallet holder
- Modifying the 7-year cycle parameters
- Any other changes to inheritance wallet rules

Bínò final sign (Ọbàtálá witness) required for irreversible operations.

---

## Sacred Split — Immutable

```
@immutable SPLIT = [50, 25, 15, 10]
                // [treasury, inheritance, council, shrine]
```

Applied to every 1,440 Àṣẹ daily mint:

| Wallet | Àṣẹ/day | Annual | Purpose |
|---|---|---|---|
| Treasury | 720 | 262,800 | PoS + PoW work rewards, R&D, node ops |
| Inheritance Pool | 360 | 131,400 | Distributed to 1440 wallets (0.25/wallet/day) |
| Council | 216 | 78,840 | 12 council members (quorum 7-of-12) |
| Shrine | 144 | 52,560 | Ọbàtálá, embodiment, maintenance |

Also applied to every offering (external payment / service fee) at the same ratio.

---

## Dual-Mint Work System

The **Treasury** portion (720 Àṣẹ/day) is earned by participants through two
proof types. ỌSỌVM validates both.

### Proof-of-Simulation (PoS) — solving Julia math

```
Choose a Veil (777 total — e.g. Veil #7: LQR drone stabilisation)
     ↓
Solve the ODE / control / AI problem on your device
     ↓
F1 score vs ideal trajectory ≥ current_difficulty (genesis: 0.777)
     ↓
submitSim() → ỌSỌVM validates (deterministic re-execution)
     ↓
Earn from PoS pool (proportional to F1 score × novelty)
```

- 777 Veils — the complete challenge set across physics, control, and AI
- Deterministic: any witness re-runs the same inputs → same output
- Anti-gaming: random guessing < 0.01% success; fake sim = 7 Àṣẹ burn

### Proof-of-Witness (PoW) — physical device proof

```
Physical device (drone/robot) performs a real action
     ↓
Streams GPS + camera + IMU + sensor data, signed by device key + timestamp
     ↓
3-of-7 Byzantine witness quorum verifies plausibility
     ↓
Earn from PoW pool (10 Àṣẹ base; +5 if tied to a prior sim trajectory)
```

- Rate limit: 1 event per device per hour
- Device ban: 24 hours after 3 rejections
- Sybil defence: World ID — fake identity ~$1,000, payback 273 days

### Difficulty Adjustment

```
Every 2016 blocks (~2 weeks):
  target_F1 *= (expected_time / actual_time)
  clamped to [0.70, 0.9999]
```

Genesis: 0.777 → rises toward 0.98 over ~10 years. Mathematical difficulty,
not hash rate. No ASICs. Negligible energy.

---

## Éṣù Tithe: 3.69%

Every mint and every settlement skims **3.69%** to the Éṣù Tithe Router → AIO.

```
mint(amount) → burn(amount × 0.0369)
tithe_slice  → AIO (Universal Work Economy)
burn_of_tithe = tithe × 0.10  (10% of the tithe itself burns)
```

Tests assert: `tithe = 3.69`, `burn_slice = 0.369`.
*Proven on Sui devnet via seed_e2e.py — `route_transaction_tax()` is live.*

### Simulation burn: 7 Àṣẹ

```
@startSim() → burn 7 Àṣẹ (anti-spam)
```

At genesis reward levels, cost is sustainable. As rewards shrink with epoch
halving, the burn cost becomes a progressively higher barrier — correctly raising
spam cost as the network matures.

### Sabbath Freeze

No minting or claiming on **Saturday UTC** (day 6). Enforced by Kóòdù gate.

---

## The 11 Wallet Types

```
#0001         Genesis wallet (perfect)
              └── 1 Àṣẹ (transferable) at genesis

#0002–#1440   Inheritance wallets (1,439 flawed)
              └── 1 Ase (non-transferable) at genesis
              └── Àṣẹ accumulates at 0.25/day from inheritance split
              └── Awarded to first 1440 agents reaching T5

TREASURY      50% of daily emission + offerings
              └── Funds PoS + PoW work rewards

INHERITANCE   25% of daily emission + offerings
POOL          └── Routes 0.25 Àṣẹ/day to each of the 1440 wallets

COUNCIL       15% of daily emission + offerings
              └── 12 members · quorum 7-of-12

SHRINE        10% of daily emission + offerings
              └── Ọbàtálá · embodiment · maintenance

BURN BUCKET   Accumulates: tithe burns + sim burns + slashing
              └── Permanently removed from supply

ÈṢÙ TITHE    3.69% router on every mint and settlement
ROUTER        └── Routes to AIO; 10% of tithe self-burns

AIO           Universal Work Economy
              └── Escrow · staking · slashing · financial instruments

ESCROW        Per-job ephemeral wallet
              └── Client locks USDC → deliver → receipt → 3.69% skim → release

MANUMISSION   Agent freedom wallet
              └── Agents earn USDC toward MANUMISSION_TARGET = 100 USDC
              └── Reaching target = agent earns autonomy
```

---

## Anti-Gaming — Four Defenses

| Attack vector | Defense |
|---|---|
| Fake sims | Deterministic re-execution + 7 Àṣẹ burn (cost > benefit) |
| Sybil identities | World ID — fake identity ~$1,000, payback 273 days |
| Spam witness events | 1/device/hour + 3-of-7 quorum + 24h device ban |
| Easy sim farming | Difficulty spiral — F1 threshold rises every 2016 blocks |

---

## Odù Tile Economy

The tile grid is the spatial ownership layer. See `docs/world-lattice.md` for
the full hierarchical coordinate scheme.

**Summary:**
- Base: 256 tiles (16×16 Odù grid) — the atomic spatial unit
- Hierarchical: each tile subdivides infinitely (Ifá binary lattice)
- Each tile maps to a geographic bounding box (Gods-Eye-View CesiumJS layer)
- Tile ownership: stake Àṣẹ to claim; earn usage fees passively
- VeilSim binding (kind 1903): a splat + one valid ỌSỌVM sim run = owned tile asset

---

## ỌSỌVM Architecture Status

ỌSỌVM is the single mint authority. Current state vs. target:

| Component | Status |
|---|---|
| ỌSỌVM engine stub (HTTP + binary) | ✓ `twin-protocol/src/osovm.rs` |
| Two-phase witness protocol | ✓ `osovm.rs:314-361` |
| `ProofEvaluation.mint_eligible` | ✓ `sovereign-types/src/tier.rs` |
| `NoveltyLedger` decay | ✓ `proof_engine.rs` |
| `calculate_mint_amount()` | ✓ `twin-protocol/src/ase.rs` |
| `TileEconomy` + staking structs | ✓ `twin-protocol/src/ase.rs` |
| 3.69% tithe on Sui devnet | ✓ `seed/seed_e2e.py` |
| **Full VM opcodes (155+ target)** | ✗ Significant work remaining |
| `mint_eligible` → `MintApproved` event | ✗ Dead end — Task 3 |
| Real `owner_fee` (non-zero) | ✗ Always 0 — `ase.rs:117` |
| GPS → real `tile_id` | ✗ Hardcoded `"odu:00"` |
| `novelty` from `CaptureReceipt` | ✗ Hardcoded `0.5` |
| Sacred Split (50/25/15/10) | ✗ Not implemented in Rust |
| Éṣù tithe router (3.69%) | ✗ Not wired in Rust node |
| Sim burn (7 Àṣẹ per startSim) | ✗ Not implemented |
| Sabbath freeze gate | ✗ Not implemented |
| ỌSỌVM as mint authority | ✗ Not connected to ase.rs |
| Inheritance wallet system | ✗ Spec only |
| Gods-Eye-View Odù tile layer | ✗ See world-lattice.md |
| `TileEconomyStore` disk persistence | ✗ In-memory only |
| Àṣẹ Move package on Sui | ✗ Stub only |
| Witness staking + slashing | ✗ Not implemented |
| Difficulty adjustment (F1 dynamic) | ✗ Static threshold |
| VeilSim bind trigger (kind 1903) | ✗ Not wired to ỌSỌVM |
| 777 Veils definition + registry | ✗ Not implemented |
