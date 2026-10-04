# Phase 0: Implementation contracts and UI specifications

- Created: 2026-09-19.
- Status: contracts defined. Real funds and testnet connectivity have not started at this stage. Local canister implementation progressed in Phase 1; see `docs/phase-1/README.md` and `docs/implementation-status.md`.
- Governing documents: `Plan.md` v0.9 (especially chapters 3 and 16), `Implementation.md` v0.5 (chapters 3, 4, 5, 9, and 14), and `Implementation-Roadmap.md` v1.1 (chapters 4, 10, and 12).

> Historical contract record. Implementation and verification statements below describe this phase; later reports record subsequent work.

## 1. Purpose

Phase 0 is complete when another implementer can identify responsibility for success, failure, and retries (roadmap chapter 4). This directory holds the contracts and UI specifications fixed before Phase 1 implementation.

API names here are **design names**, not deployed Candid or implemented code. Mark unknown canister IDs, testnet key IDs, and HL limits as unresolved; do not substitute guesses.

## 2. Deliverables

| Document | Contents |
|---|---|
| `authority-matrix.md` | Canister responsibilities, allowed callers, retained keys, permitted signed actions, and prohibitions |
| `api-contract.md` | APIs by canister, error types and retry classes, HPKE envelopes, and limits |
| `state-machines.md` | Action, fund-request, and order transitions; reconciliation rules; canonical UI labels |
| `money-and-units.md` | Integer money units, rounding, maxima, nonces, duplicate events, and retention |
| `environments.md` | Separation of local/testnet/mainnet, keys, endpoints, and mock issuers |
| `threat-test-matrix.md` | Threats, tests, expected results, evidence, and implementation phases |
| `privacy-evaluation.md` | A/B0/B1 inputs, attacker-visible information, and acceptance criteria |
| `ui-spec.md` | Screen layout, abnormal states, balance categories, explanations, and chart policy |

Additional deliverables (roadmap chapter 12, item 2): the Rust workspace scaffold and pinned dependencies in root `Cargo.toml`, `rust-toolchain.toml`, `icp.yaml`, `crates/`, and `scripts/check-no-await.sh`.

## 3. Mapping to roadmap chapter 4

### Contracts to define before implementation

| Roadmap item | Coverage |
|---|---|
| Define each canister's responsibilities, callers, and permitted signed actions | `authority-matrix.md` sections 2–4 |
| Define authentication, order, funds, upgrade-reservation APIs, and errors | `api-contract.md` sections 2–5 |
| Define withdrawal reservations, in-transit funds, unknown outcomes, and cancellation transitions | `state-machines.md` sections 2–6 |
| Define integer units, rounding, maxima, and duplicate events | `money-and-units.md` sections 2–5 |
| Separate IDs, keys, endpoints, and mock issuers by environment | `environments.md` sections 2–5 |
| Map threats to tests, including duplicate transfers, authorization bypass, stale callbacks, and malicious upgrades | `threat-test-matrix.md` section 3 |
| Define privacy-comparison inputs, attacker information, and acceptance thresholds | `privacy-evaluation.md` sections 2–5 |

### UI design

| Roadmap item | Coverage |
|---|---|
| Define desktop trading, funds, and history layouts | `ui-spec.md` sections 2–4 |
| Define disconnected, insufficient funds, sending, unknown, stale, and stopped states | `ui-spec.md` section 5 |
| Treat custody balance, trading-account equity, and withdrawable amount separately | `ui-spec.md` section 6 |
| Explain confidentiality, canister custody, EOA loss, and recovery restrictions during stoppage | `ui-spec.md` section 7 |
| Assess chart requirements and licensing conditions; narrow the candidates | `ui-spec.md` section 8 |

## 4. Completion and unmet conditions

- Every item above must be explained, with success, failure, retry, and reconciliation responsibilities identifiable for each API.
- These documents define contracts; they are not implementation or live-verification evidence. If Phase 1 measurements contradict a contract, record the measurement and update the contract.
- Funds, signing, authentication, and guard contracts cannot be completed with successful-path descriptions alone. Phase 1 Go/No-Go remains unmet while required failure tests in `threat-test-matrix.md` have not run.

## 5. Exclusions

- Full hl-sign implementation (action construction, msgpack, EIP-712, and recovery of v), real DB tables and migrations, HL testnet round trips, PocketIC fault-test infrastructure, fixed Candid `.did`, frontend changes, and final chart selection.
- Selection of Rust PocketIC (`pocket-ic`) belongs to Phase 1. At this stage, docs.rs builds only `x86_64-unknown-linux-gnu`, and Apple Silicon operation is unverified.
- Confirm actual testnet/mainnet values (canister IDs, signing key IDs, HL limits and fees) through Phase 1 measurements.

## 6. Differences from the governing documents

No change to `Plan.md`, `Implementation.md`, or `Implementation-Roadmap.md` is required at this stage. Add conflicting implementation decisions to this table; obtain separate approval for revisions to the governing design.

| Date | Target | Difference | Response |
|---|---|---|---|
| 2026-09-19 | Implementation 14.3 | API names are design names; Candid is not fixed | Explain in `api-contract.md`; fix actual Candid in Phase 1 |
| 2026-09-19 | Implementation 3.1 | `Cargo.toml` and `icp.yaml` did not exist | Add scaffolds in Phase 0 |
| 2026-09-19 | Roadmap chapter 4 | Deliverables are documents, but workspace preparation from chapter 12, item 2 also occurs here | Record in section 2 |
| 2026-09-19 | Implementation 14.3 | Agent-generation creation and history APIs were undefined, though required by chapter 7 and the UI | Add the list in `api-contract.md` 5.1, without expanding authority |
| 2026-09-19 | Implementation 3.1 and 14.3 | `policy_registry` APIs were undefined | Define 4 design methods in `api-contract.md` section 5 |
| 2026-09-19 | Roadmap 10.5 | Retain Lightweight Charts; Advanced Charts remains unevaluated | Record requirements and reasons in `ui-spec.md` section 8 |

## 7. Open items

Track these in parallel (roadmap chapter 4); they are outside Phase 0 completion criteria.

| ID | Item | Status | Effect |
|---|---|---|---|
| O-1 | Operator, target countries, and terms compliance | Not investigated | No recruitment or admission of real customers |
| O-2 | SNS token allocation and sale conditions | Unresolved | Production prerequisite |
| O-3 | Advanced Charts availability and integration cost | Unevaluated | Reconsider only if initial requirements need advanced drawing (`ui-spec.md` section 8) |
| O-4 | Testnet/mainnet canister IDs and signing key IDs | Unresolved | `environments.md` defines only where to record them |
| O-5 | Local-network startup (icp launcher) | Measured in Phase 0 | Results in `../implementation-status.md` |

## 8. Explicit limits

- At this stage, canister code, Candid, and testnet canister IDs do not yet exist. Contracts cannot replace implementation.
- The UI demo (`frontend/`) uses synthetic data. These UI specifications are implementation instructions, not evidence that the demo satisfies them.
- Documentation cannot establish confidentiality, correlation resistance, or fund safety. Phase 1 measurements and independent audit are required.
