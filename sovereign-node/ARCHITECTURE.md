# sovereign-node Architecture

## What This Crate Is

sovereign-node is the thin protocol glue daemon and physical hardware access layer
for the sovereign stack. It owns exactly the concerns that require a running process
on a physical device: scanning for nearby peers, routing protocol envelopes in and
out, managing the capture pipeline, publishing timestamped receipts, and evaluating
proof-of-simulation scores that originate from local sensor data.

It is NOT a general-purpose application server. Business logic, economic state, and
agent cognition all belong in dedicated crates upstream.

## Legitimate Concerns (belongs here)

1. VCP device discovery and sessions — BLE/mDNS scan loop, peer session lifecycle,
   physical proximity events.

2. DIP envelope routing — receiving, validating, and forwarding signed envelopes via
   the Vantage and Nostr transport adapters.

3. Capture job pipeline — accepting capture requests, coordinating sensor reads,
   packaging raw artifact bundles for downstream consumers.

4. TSP receipt publishing — stamping and emitting Zangbeto-style receipts that
   anchor physical events to the provenance chain.

5. Proof-of-Simulation evaluation — scoring simulation fidelity against sensor
   ground-truth when that data is only available on this device.

## Misplaced Concerns (future migration targets)

The following modules or subsystems have accumulated inside sovereign-node but belong
elsewhere. The existing code compiles and all 89 tests pass. This is a migration
guide for future sessions, not an immediate refactor requirement.

| Concern | Migrate to |
|---|---|
| Governance store (proposals, votes, council state) | Vantage |
| Wallet store (balances, signing keys, ASE ledger) | Vantage |
| Emission allocator (1 ASE/min distribution logic) | Vantage |
| Proof engine scoring unrelated to local sensors | OSOVM |
| Body sessions (agent embodiment lifecycle) | Omo-Koda2 |
| Perception loop (active sensing state machine) | Omo-Koda2 |
| Swarm coordination (multi-node task dispatch) | Scarabswarm |
| Nostr publishing (outbound note construction) | Omo-Koda2 ip_layer |

## Scope Boundary Rule

If it does not touch a physical device, a protocol wire format, or the capture
pipeline — it does not belong here.

When reviewing a pull request or planning a new feature, apply this test first.
Code that passes the test can live in this crate. Code that fails should be routed
to the appropriate upstream crate listed above.
