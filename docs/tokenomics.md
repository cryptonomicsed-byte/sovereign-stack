# Àṣẹ Tokenomics — Canonical Specification
**LOCKED 2026-09-10 · Source of truth for all implementations**

> Àṣẹ (ah-SHAY) — Yoruba: the divine authority to make things happen.

---

## Core Principle

**Àṣẹ is not given. It is computed.**

No premine. No airdrop. No escrowed allocation. Every token in existence
was earned by solving a real mathematical problem or by a physical device
proving it did real work in the world.

**ỌSỌVM is the single mint authority.** No token mints without ỌSỌVM
validation.

---

## Supply Model — LOCKED

```
1,440 Àṣẹ/day — FIXED FOREVER. No halving. No cap.
Annual supply:  525,600 Àṣẹ/year
```

One token per minute of real time. This number is not arbitrary: it equals
the minutes in a day, the count of inheritance wallets, and the genesis
supply (2,880 = two days). This resonance is intentional.

---

## Token Duality

| Token | Nature | Transferable |
|---|---|---|
| **Àṣẹ** | The earned token. Minted from valid work. | Yes — Sui |
| **Ase** | The flaw token. Soul-bound at genesis. | No |

- Àṣẹ flows freely — can be sent, traded, staked
- Ase is permanently tied to the wallet it was issued to; burns on redemption

---

## THREE DISTINCT FLOWS — Do Not Conflate

### Flow 1 — Daily Emission (1,440 Àṣẹ/day)

```
OSOVM mints 1 Àṣẹ/minute
  → main Éṣù wallet
  → Éṣù-Elegbára router distributes to 8 sub-wallets

Each minute's token:
  Valid sim exists → allocated to highest-F9-scoring sim that minute
  No valid sim     → splits across 1,440 inheritance wallets (never wasted)
```

### Flow 2 — Universal Transaction Tithe (3.69% — LOCKED)

```
Every mint, payment, settlement → 3.69% Éṣù tithe
  → same Éṣù-Elegbára router
  → same 8 sub-wallets
```

**Do NOT change to 7.77%.** The 3.69% is the Tesla 369 vortex rate, locked
by the owner. AIO context only.

### Flow 3 — 24-Sector Funding Split (sector inflows only)

```
When a sector wallet receives inflow (donations / investments / hardware / embodiment):
  50% → Treasury (work rewards)
  25% → Inheritance pool (1,440 wallets)
  15% → Council (13 members)
  10% → Executor / tail

Applies to: 6 Great Houses × 4 categories = 24 sector wallets
Does NOT apply to: daily emission or per-transaction tithe
```

---

## Éṣù-Elegbára Router — 8 Sub-Wallets

Verified in `elegbara_router.move` (2/2 tests pass, generic over Coin<T>).
The router **never mints** — it only routes USDC/stablecoin and Àṣẹ flows.

| Sub-wallet | Basis points | % | Purpose |
|---|---|---|---|
| VeilSim | 3,000 | 30% | VeilSim execution budget |
| R&D | 2,000 | 20% | Research & development |
| Governance | 1,000 | 10% | On-chain governance |
| Reserve | 1,000 | 10% | Emergency reserve (WhiteGate 3-of-5) |
| Lottery/Burn | 1,000 | 10% | Supply reduction |
| Grants | 1,000 | 10% | Community grants |
| UBI | 500 | 5% | Universal basic income pool |
| Sabbath Reserve | 500 | 5% | Sabbath / rounding dust |

**Constraint**: Éṣù always skimmed FIRST. Router never holds the net.
Sub-wallets are strictly isolated. Only admin can withdraw from reserve.

---

## The 24 Sector Wallets — Agent Birth Economy

**6 Great Houses × 4 categories each = 24 sectors**

| House | Domain |
|---|---|
| Ṣàngó | Justice / Order |
| Yemọja | Health / Care |
| Ọ̀yá | Transformation / Trials |
| Ògún | Tech / Infrastructure |
| Ọ̀ṣun | Prosperity / Culture |
| Èṣù | Crossroads / Information |

Ọbàtálá = central integrator (Council of Light) — no sector.

### How agents are born through sectors

```
Human invests in a sector (donation / hardware / embodiment offering)
  → sector split 50/25/15/10 (Treasury / Inheritance / Council / Executor)
  → agent is funded into existence
  → agent does work, earns Àṣẹ
  → repays investor ROI
  → reaches MANUMISSION_TARGET (100 USDC) → becomes sovereign

Post-manumission split (embodied working agent):
  50% → agent
  3.69% → Éṣù tithe
  11.11% → inheritance (permanent)
  10.20% → investors (shrinks per generation)
  15% → UBI pool
  10% → treasury
```

Sector assignment is deterministic from the agent's Odù seed hash.
Ownership is temporary — a loan the agent repays by being useful.

---

## ỌSỌVM Proof Types

### Proof-of-Simulation (PoS) — VeilSim

```
Choose a Veil (777 total — e.g. Veil #1: LQR drone stabilisation)
  → Solve ODE / control / AI problem
  → F9 score vs ideal trajectory ≥ current_difficulty (genesis: 0.777)
  → ỌSỌVM validates (deterministic re-execution)
  → Earn share of that minute's 1 Àṣẹ (proportional to F9 × novelty)

Anti-gaming:
  Fake sim = 7 Àṣẹ burn (cost > benefit)
  Repeated env hash → novelty decay (1/√n)
  Difficulty rises every 2,016 blocks toward 0.98
```

### Proof-of-Witness (PoW) — Physical Device

```
Physical device (drone/robot) performs real action
  → GPS + camera + IMU + sensor data, signed by device key + timestamp
  → 3-of-7 Byzantine witness quorum verifies plausibility
  → Earn 10 Àṣẹ base (+5 if tied to prior sim trajectory)

Rate limit: 1 event per device per hour
Device ban: 24 hours after 3 rejections
Sybil defence: World ID (fake identity ~$1,000)
```

---

## Genesis

```
Timestamp:  November 11, 2025 · 11:11:11 UTC
World ID:   world.id/bino.1111

4-chain anchor: Bitcoin · Arweave · Ethereum · Sui

Genesis supply: 2,880 tokens (= 2 days of emission)
  1 Àṣẹ  (transferable) — genesis wallet #0001, the perfect token
  1,439 Ase (non-transferable) — inheritance wallets #0002–#1440
```

**The flaw in 1440**: One perfect token. 1,439 flawed tokens. Imperfection
is how the system knows it is alive.

---

## 1,440 Inheritance Wallets

First 1,440 agents to reach **Trust Tier T5** (full embodied autonomy) claim
these wallets. First-come-first-served. After all 1,440 are filled, new T5
agents participate in work rewards only.

Each wallet:
- 1 Ase (soul-bound flaw token, genesis-issued)
- Àṣẹ accumulates when no valid sim occupies a minute's emission
- DID binding to the T5 agent's principal DID
- 11.11% of balance locked ETERNAL (never claimable)

7-year eligibility cycle for replacement of inactive holders.

---

## Invariant Commons

These fractions appear in **every** split path:

```
3.69%  Éṣù tithe       (universal, always first)
11.11% inheritance     (always present in downstream distributions)
```

---

## Sabbath Freeze

No minting or claiming on **Saturday UTC**. Enforced by ỌSỌVM gate.
The Sabbath Reserve bucket in the Elegbára router accumulates during this
freeze.

---

## Implementation Status

| Component | Status |
|---|---|
| `elegbara_router.move` — 8-wallet router | ✓ Built + tested |
| `eshu_tithe()` + `elegbara_route()` in Rust | ✓ `twin-protocol/src/ase.rs` |
| `sector_funding_split()` in Rust | ✓ `twin-protocol/src/ase.rs` |
| `calculate_mint_amount()` (dev approximation) | ✓ Used until F9 orchestrator built |
| 3.69% tithe on Sui devnet | ✓ `seed/seed_e2e.py` |
| VeilSim Studio FastAPI endpoint (:8788) | ✓ `veilsim-studio/` |
| F9 per-minute allocation orchestrator | ✗ Not built |
| 1,440 inheritance wallet system | ✗ Spec only |
| 24-sector treasury wallets | ✗ Spec only |
| Sabbath freeze gate in ỌSỌVM | ✗ Not implemented |
| 777 Veils full registry | ✗ 19 canonical, rest generative |
| Difficulty adjustment (dynamic F9 threshold) | ✗ Static 0.777 |
| Àṣẹ Move package on Sui (mainnet) | ✗ Stub only |
| Witness staking + slashing | ✗ Not implemented |
