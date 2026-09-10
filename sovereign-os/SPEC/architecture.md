# Sovereign OS — Canonical Architecture

> **This document is the single source of truth for Sovereign OS layering.**
> Every crate, repo, and hardware target must be mappable to one of the five layers below.
> When a decision conflicts with this document, update this document first — never silently deviate.

---

## The Five Layers

```
Layer 5 — Intelligence
  Local models, remote models, Hermes, OpenClaw, Gemini, etc.
  The OS is NOT the model. Omo-Koda is NOT the model.
  Omo-Koda governs, remembers, authorizes, executes, and proves what the model does.

Layer 4 — Ecosystem
  Vantage · Mycelium · ÒRÀCÙLÙM · Zàngbétò · Twelve Thrones
  OSOVM · ScarabSwarm · Witness · DIP · VCP · TSP
  These are external services and optional adapters — NOT core dependencies.
  Omo-Koda MUST boot with zero internet access.

Layer 3 — Omo-Koda Runtime  (Agent Substrate)
  The sovereign agent operating environment.
  Identity · Birth · Lifecycle · Memory · Cognition
  Permission · Capability · Execution · Tool-Runtime
  ActReceipt · Hooks · Delegation · Swarm · Economy

Layer 2 — Sovereign Runtime  (Machine↔Agent Boundary)
  The universal execution contract enforced between agents and machines.
  Principal → Capability → Authorization → Action → Evidence → ActionReceipt
                                                                      ↓
                                              DIP · VCP · TSP · Mesh · Chain
  Also owns machine-facing interfaces:
  Device · Power · SecureElement · Embodiment · Safety · Update · Attestation

Layer 1 — Substrate
  Linux · RTOS · MCU firmware · (future: Sovereign Microkernel)
  Drivers · Hardware Abstraction · Boot · Real-Time Scheduling
  Omo-Koda does NOT own this layer.

Layer 0 — Hardware
  MCU · ARM · x86 · GPU/NPU · Sensors · Camera · MIDI · Audio · Radio · GPIO
```

---

## The Key Boundary: Omo-Koda ↔ Sovereign Runtime

```
SOVEREIGN OS
      │
      ├── Layer 3: Omo-Koda Runtime
      │         Agent brain. Receives intents, owns memory, enforces permissions,
      │         manages lifecycle, generates ActReceipts.
      │
      └── Layer 2: Sovereign Runtime
                Machine-facing contract.
                Authenticates Principal, validates Capability, authorizes Action,
                collects Evidence, emits ActionReceipt, routes to DIP/VCP/TSP.
                Machine hardware interfaces live here, NOT in Omo-Koda.
```

**What Omo-Koda owns:**
- Agent identity (BIPON39, Ed25519 DNA, NIP-06)
- Birth / lifecycle / memory / cognition
- Permission enforcement + capability delegation
- Tool registry + WASM sandbox
- Hooks + skills + swarm coordination
- ActReceipt (agent-level: PoCW, EpistemicSeverity, Plane, BLAKE3 chain)
- Economy + reputation

**What Sovereign Runtime owns:**
- Principal type (hardware attestation binding, nostr_pubkey, sui_address, ip_root_id)
- CapabilityKernel (universal authorization engine)
- ActionReceipt (machine-level: action, resource, evidence, outcome)
- EvidenceBundle (Merkle-rooted proof of what was observed)
- ExecutionEngine (begin → complete/fail lifecycle)
- Device traits (HardwareKeyProvider, EmbodimentManager, SafetySupervisor, PowerPolicy)
- Attestation (hardware-backed signature over principal_id)

**What neither owns (external adapters):**
- Vantage · Nostr · Sui · Walrus · MCP · A2A · Mycelium · Twelve Thrones · LLMs

---

## The Universal Execution Reflex

Every consequential operation in every repo MUST flow through this chain:

```
REQUEST
   ↓
authenticate Principal         (sovereign-runtime: principal.rs)
   ↓
resolve Capability             (sovereign-runtime: capability.rs)
   ↓
authorize                      (sovereign-runtime: CapabilityKernel)
   ↓
execute                        (sovereign-runtime: ExecutionEngine)
   ↓
collect Evidence               (sovereign-runtime: evidence.rs)
   ↓
create ActionReceipt           (sovereign-runtime: receipt.rs)
   ↓
persist + broadcast            (sovereign-node: receipt_store, dip_gateway)
```

If a consequential action in any repo does NOT emit an ActionReceipt, that is a P0 bug.

---

## Receipt Convergence

Two receipt lineages must converge into one canonical type:

| Field              | Source                          |
|--------------------|---------------------------------|
| receipt_id         | Omo-Koda ActReceipt (BLAKE3)    |
| action             | sovereign-runtime ActionReceipt |
| resource           | sovereign-runtime ActionReceipt |
| principal_id       | sovereign-runtime ActionReceipt |
| capability_id      | sovereign-runtime ActionReceipt |
| outcome            | sovereign-runtime ActionReceipt |
| evidence_root      | sovereign-runtime ActionReceipt |
| proof_of_work      | Omo-Koda ActReceipt (PoCW/BB)   |
| epistemic_severity | Omo-Koda ActReceipt             |
| plane              | Omo-Koda / omokoda-hermetic     |
| previous_hash      | Omo-Koda ActReceipt (BLAKE3)    |
| signature          | sovereign-types CanonicalReceipt|

The unified type lives in **sovereign-runtime**. Omo-Koda's `ActReceipt` becomes a domain-specific
view on top of the sovereign ActionReceipt.

---

## Hardware Tier Mapping

| Tier     | Substrate             | Omo-Koda variant        | Use case                    |
|----------|-----------------------|-------------------------|-----------------------------|
| Micro    | MCU (Zephyr/FreeRTOS) | Omo-Koda Micro          | MIDI controller, IoT sensor |
| Portable | Linux ARM64           | Omo-Koda Standard       | Phone, Termux, SBC          |
| Spatial  | Linux ARM64 + GPU/NPU | Omo-Koda Spatial        | Gaussian splatting device   |
| Robotics | Linux + RTOS + PX4    | Omo-Koda Robotics       | Quadruped, drone, arm       |

For all tiers, Omo-Koda sits on top of Sovereign Runtime which sits on the substrate.
The substrate changes per tier; Omo-Koda's agent model does not.

---

## The Principal Continuity Principle

The same Principal (the same identity, the same DID, the same BIPON39 DNA) must be able
to embody different physical machines over time:

```
MIDI device (Micro)
     ↓ same Principal
Phone (Portable)
     ↓ same Principal
Spatial camera device (Spatial)
     ↓ same Principal
Robot (Robotics)
```

The body changes. The agent doesn't.

This is enforced by: `Principal.chain.principal_id` being consistent across all embodiments,
and `HardwareAttestation` binding each embodiment to its current physical hardware without
replacing the identity.

---

## Offline Sovereignty Invariant

Omo-Koda MUST boot and operate with zero internet connectivity.

Core local capabilities (always available):
- Agent identity and memory
- Capability enforcement
- ActionReceipt generation
- Local tool execution
- WASM sandbox
- DIP/VCP local mesh (BLE/mDNS/Meshtastic)

Optional (degrade gracefully to no-op when unavailable):
- Vantage registration
- Nostr broadcast
- Sui anchoring
- LLM providers
- Mycelium
- Twelve Thrones
- ScarabSwarm

---

## Safety Hierarchy (Robot/Spatial tier)

```
Agent intent (Omo-Koda)
      ↓
VCP capability gate
      ↓
Safety Supervisor (sovereign-runtime: safety.rs)
      ↓
RTOS / PX4 / ArduPilot
      ↓
Motor Controller / Actuator
```

Omo-Koda says "move toward target."
The Safety Supervisor decides whether that command can physically execute.
The RTOS executes if and only if both agree.

---

## The Sovereign Machine Runtime Boot Sequence

1. Secure Boot (hardware)
2. Linux kernel / MCU firmware
3. Hardware Identity (secure element: ATECC608B / TPM2 / SE050)
4. sovereign-runtime init (Principal hydration from secure element)
5. CapabilityKernel bootstrap
6. Embodiment Manager init (devices, sensors, power)
7. DIP / VCP / TSP init (local mesh first, remote on connect)
8. Omo-Koda birth (if first boot) / resume (if existing identity)
9. Agent runtime ready

---

## Canonical Repo-to-Layer Map

| Repo / Crate          | Layer | Role                                         |
|-----------------------|-------|----------------------------------------------|
| sovereign-types       | L2    | Shared canonical types (IdentityChain, etc.) |
| sovereign-runtime     | L2    | Execution contract + machine interfaces      |
| dip                   | L2    | Routing protocol                             |
| vcp                   | L2    | Session/capability protocol                  |
| twin-protocol         | L2    | Spatial twin protocol (TSP)                  |
| sovereign-node        | L2/L4 | Node daemon wiring all protocols             |
| sovereign-pipeline    | L2    | Capture pipeline                             |
| sovereign-a2a         | L4    | A2A adapter                                  |
| sovereign-cli         | L4    | CLI tooling                                  |
| mycelium-finetune     | L5    | Fine-tuning pipeline (intelligence layer)    |
| Omo-Koda2             | L3    | Agent runtime (THE cognitive core)           |
| ip-layer              | L4    | IP Root publication (Nostr kind:31900)       |
| organism-core         | L4    | Julia simulation runtime                     |
| Blocksim              | L4    | PoSim blockchain simulation                  |
| ScarabSwarm           | L4    | Swarm coordination                           |
| sovereign-os          | L2    | OS spec + boot + embodiment                  |
