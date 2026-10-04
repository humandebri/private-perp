# Phase 1 (S1–S3): Task ledger and evidence

> Historical Phase 1 evidence ledger. Status and test counts describe the dates below; see the root README for current verification.

- Date of creation: 2026-09-19 (Last update: 2026-09-21)
- Status: **Completed up to the local verification range of S1 to S3** (evidence P1-001 to P1-010). **Phase 1 Go/No-Go is not passed**, and testnet round trips is not performed.
- Standards: Chapters 5 and 11 of `Implementation-Roadmap.md`, Chapters 9.1, 9.2, and 9.4 of `Implementation.md`, and the contracts in `docs/phase-0/`

## 1. How to use this ledger

Record it in the format of Chapter 11 of the roadmap (ID/Purpose/Dependency/Implementation Target/Exclusion/Acceptance Conditions/Verification/Evidence/Remaining items). Do not treat the entire Phase as completed; only consider the range of evidence as "completed".

The evidence is placed in `docs/phase-1/evidence/` and corresponds to T-xxx in `docs/phase-0/threat-test-matrix.md` and 1-x in `Implementation.md`.

## 2. Stages and response

| Stage | Content | State |
|---|---|---|
| S1 | hl-sign (action construction, msgpack, EIP-712, v recovery) + official SDK comparison | Completion (P1-001, P1-002) |
| S2 | PocketIC foundation, ic-sqlite-vfs abstraction, double-entry ledger, reservation, outbox, nonce, fencing | 2A~2C completed/2D partially completed (P1-003~P1-007). 2E (PocketIC failure test) partially executed. |
| S3 | Mock HL bidirectional/fault injection, local ECDSA, guard bypass refusal | Local range is completed (P1-005, P1-008-P1-010). `trading_core` order acceptance, agent-key signing, sending, cancellation sending, reconciliation, snapshot, risk reservation, `control_guard` 7-day delay, execution, content mismatch rejection, `policy_registry` fail-closed and stop operation have been verified. testnet round trips is not performed. |

## 2.1 S2 progress (2026-09-21)

| Stage | State | Content |
|---|---|---|
| 2A | completion | Implement the schema for vault/core (version-specific Migration), double-entry ledger (zero-sum accounting, signed postings, balance derivation), and idempotent handling, reservation, challenge/session, and epoch CAS for `crates/db`. Verify that `Db::init`→`migrate` passes through in PocketIC during Canister installation. |
| 2B | completion | Implement and verify EOA challenge, session and expiration (`auth.rs`), fund reference API and allocation and withdrawal acceptance + reservation (`fund.rs`). Includes challenge `principal` binding and single-use withdrawal intent nonces (`P1-004`). |
| 2C | completion | ECDSA spike completion (`P1-005`, real tECDSA operation locally). The outbox implements three types of allocation, withdrawal, and recovery, and validates from claim → signature → `dispatching` persistence → non-replicated POST → reconciliation (`P1-006`). `unknown` resend prohibition, no release even after time has passed, elimination of `unknown`/`dispatching`, transfer route for deposits (replicated outcall of `/info`), and suspension counting for unknown addresses, and elimination of `unknown` are also validated. In addition to generation requests and state display, `approveAgent` signature with master key and the transition to `active` upon acceptance are also implemented and validated. Authentication, ledger, storage of unresolved actions, and non-resend during upgrade are also validated. |
| 2D | Part completed | Verify key registry (generation update and public key distribution) and envelope encryption and decryption (`P1-007`). `aad` binds the caller and expiration date. Application to personal data API and handling during key update (**T-605**) are not yet implemented. |
| 2E | Part completed | The PocketIC failure test (local parts of T-1xx/T-2xx) has been completed. The rest are T-401~T-410, etc. |
| — | completion | outbox/events repo (operation of `fund_actions` and `external_events`). |

The type of withdrawal intent is `PrivatePerpWithdrawal(address eoa,uint64 amount,string asset,string destination,string network,uint64 nonce,uint64 expiresAt,bytes canister)`. This is because it binds to an authenticated EOA without including any internal ID (user_id, account_id) that the client does not know (`hl-sign::private_perp`).

Outbox evidence: `crates/pocket-ic-tests/tests/vault_outbox.rs` (**9 successful**. Allocation to in transit, resend prohibited when response loss, reservation freed when exchange rejected, payout transmission and `payout_settled`, `unknown` unexecuted resolved, single execution of simultaneous sweep, rejection of forged signing digests).

Evidence of the funds API: `crates/pocket-ic-tests/tests/vault_funds.rs` (**6 successful**. Denial of insufficient balance, denial of idempotency-key reuse with a mismatched payload, denial of reservation-based reduction of withdrawable amount, denial of different key/destination mismatch and expired intent, denial of session by different caller, denial of reuse of withdrawal intent nonce).

Authentication evidence: `crates/pocket-ic-tests/tests/vault_auth.rs` (**7 successful**. Session issuance with the correct signature, reuse of a different key/challenge, expired, rejection of a different origin = T-101/T-103/T-104/T-105, challenge principal binding = T-102, rejection of mismatch between declared principal and caller).

deposit evidence: `crates/pocket-ic-tests/tests/vault_deposits.rs` (2 successful), `vault_reconcile.rs` (1 successful), the recovery from the depositor is `vault_recovery.rs` (3 successful).

Note: Your own EIP-712 schema (challenge and withdrawal intent) has been implemented in `hl-sign::private_perp`, and field binding and signature recovery have been fixed in host tests. `db::init` has been changed to receive a Migration list for each Canister (`policy` and `control_guard` are initialized as empty since they are not defined in the dedicated schema).

## 3. Completed tasks

### P1-001 hl-sign: Non-async and pure action construction and signature

- Purpose: Create a signature implementation that satisfies the 5 items of `Implementation.md` 9.1.
- Dependency: None (Phase 0 workspace).
- Target implementation: `crates/hl-types` (decimal, msgpack, action), `crates/hl-sign` (keccak, eip712, hash, signature, user_signed).
- Not applicable: real signature in tECDSA (management Canister), PocketIC, HL connection.
- Acceptance conditions: The action hash, signature, and recovery address match with the same input as the official SDK.
- Verification: `cargo test` (17 crates/hl-types, 33 crates/hl-sign, 2 tests/fixtures.rs). `bash scripts/check-no-await.sh`.
- Evidence: `docs/phase-1/evidence/P1-001.md`.
- Remaining items: none (within the range of S1).

### P1-002 Fixed test vectors for the official SDK

- Purpose: Meet the requirements of `Implementation.md` 9.1 “Lock test vectors in the repository and record the original SDK version.”
- Target implementation: `tools/hl-fixture-gen/` (fixed `@nktkas/hyperliquid@0.33.3` and `@viem@2.56.8`), `crates/hl-sign/tests/fixtures/` (11 items).
- Excluded: msgpack byte arrays and EIP-712 digests (since the SDK is not available, `null`).
- Acceptance conditions: fixture must be deterministically regenerated and match Rust's computation.
- Verification: Run `cd tools/hl-fixture-gen && pnpm install --frozen-lockfile && pnpm generate` twice and get the same output. `cargo test -p hl-sign --test fixtures`.
- Evidence: `docs/phase-1/evidence/P1-002.md`, `tools/hl-fixture-gen/README.md`.
- Remaining items: none.

### P1-003 Confirmation of the establishment of the PocketIC harness (assuming S2)

- Objective: Build a foundation that can execute Canister integration and injection failure tests in PocketIC.
- Target implementations: `scripts/fetch-pocket-ic.sh`, `scripts/pocket-ic-test.sh`, `crates/pocket-ic-tests`, `crates/api-types`.
- Not applicable: Each scenario of fault injection (S3).
- Acceptance conditions: Obtain the PocketIC server binary and deploy 4 Canisters so that the `version` query responds.
- Verification: `bash scripts/pocket-ic-test.sh` (`tests/spike.rs`). Use PocketIC server 16.0.0 on aarch64-apple-darwin.
- Evidence: `docs/phase-1/evidence/P1-003.md`.
- Remaining tasks: Execution in CI (ubuntu, `pocket-ic-x86_64-linux.gz`) has been added to workflow and **not executed**.

## 4. Completed tasks (P1-004~P1-010)

P1-004~P1-010, which were previously classified as "not completed," all have tests that correspond to the evidence. The actual scope of implementation differs from the initial response table in some cases, so it will be recorded according to the evidence and actual files. The number of cases is the actual value of `#[test]`.

| ID | Implementation content | Evidence | Corresponding test (number of cases) |
|---|---|---|---|
| P1-004 | The receiving side of the funds API (reference, receipt, reservation, double-entry ledger) and deposit (`/info` transmission route, suspension recording for unknown addresses and transfer to the person in charge) | `docs/phase-1/evidence/P1-004.md` | `vault_funds.rs` (6), `vault_deposits.rs` (2), `vault_reconcile.rs` (1) |
| P1-005 | Local ECDSA spike (key id `test_key_1` confirmation for `sign_with_ecdsa` and `v` recovery) | `P1-005.md` | `ecdsa_spike.rs` (1) |
| P1-006 | Sending signature outbox funds and reconciliation (allocation, disbursement, recovery, keeping and clearing `unknown`) | `P1-006.md` | `vault_outbox.rs` (9), `vault_recovery.rs` (3) |
| P1-007 | HPKE (key registry, envelope bidirectional, `aad` binding) | `P1-007.md` | `vault_hpke.rs` (1), `hpke_roundtrip.rs` (1), `vault_hpke_envelope.rs` (1) |
| P1-008 | `trading_core` authorization boundary, order reception, agent-key signing, sending, cancellation sending, reconciliation, snapshot, risk reservation | `P1-008.md` | `core_auth.rs` (1), `core_orders.rs` (11), `core_order_validation.rs` (1), `core_risk.rs` (1) |
| P1-009 | Single-instance reservation, 7-day grace period, content matching, and simultaneous execution of `control_guard` | `P1-009.md` | `guard_upgrade.rs` (5) |
| P1-010 | T-xxx test and session validation that can be executed locally, `policy_registry`, upgrade storage, Agent approval | `P1-010.md` | `vault_auth.rs` (7), `vault_session_status.rs` (1), `policy_stop.rs` (2), `core_policy_stop.rs` (1), `vault_upgrade.rs` (1), `vault_agents.rs` (1), `spike.rs` (1) |

### 4.1 Remaining items (next stage)

- Application of envelopes to personal data API and handling during key update (**T-605** - unresolved).
- Matching execution of `control_guard`: The uniqueness of execution paths and concurrent execution has been verified in minimal Wasm, but full-size Wasm exceeds the `execute_upgrade` argument limit (2 MiB), so chunking or code registry is required (measured 2,193,336 bytes - **not resolved**).
- Remaining failed tests (T-401-T-410, etc.) and reading-only review after Phase 1 completion.
- Definition of operating procedures when permanent errors (such as digest mismatch) occur, and resolution of handling failures in periodic sweep timers (**not resolved**).
- testnet: Real HL acceptance behavior, signing p50/p95, acceptance→HL acceptance, the validity of Confidential Subnet (**unverified**).
- PocketIC execution in CI (ubuntu) is added to workflow and **not executed**.

## 5. Real-test environment (for replication)

- Toolchain: `rustc`/`cargo` 1.97.0 (`rust-toolchain.toml`).
- Since this machine cannot write to HOME, it will be configured to specify `CARGO_HOME=<repo>/.cargo-home`, `ICP_HOME=<repo>/.icp-home`, and `POCKET_IC_BIN=<repo>/.pocket-ic/pocket-ic` at runtime (all of which are already `.gitignore`d). `scripts/pocket-ic-test.sh` will ignore the incompatible environment variable `POCKET_IC_BIN` and use the binary in the workspace (overwriting with `POCKET_IC_BIN_OVERRIDE=1`).
- PocketIC server: `pocket-ic-arm64-darwin.gz` (`pocket-ic-server 16.0.0`) for `release-2026-09-18_03-28-base`.
- Official SDK: `@nktkas/hyperliquid@0.33.3` (only when generating fixtures. Rust implementation is not required).

## 6. Specified specifications (confirmed in Phase 1 and reflected in the contract)

- The action hash is connected by `keccak256( msgpack(action) ‖ nonce(8B BE) ‖ vault ‖ expires )`. The vault is always a marker of 1 byte (`0x00`/`0x01`+20 bytes), the expires is 0 bytes if not specified, and 0x00 marker + 8 bytes BE if specified (official SDK `esm/signing/_l1.js`).
- msgpack encodes integers in the minimal representation. However, the official SDK expands `|value|` to BigInt for integers outside the int32 range, and positive values become `0xcf` (uint64), and negative values become `0xd3` (int64). Amounts, quantities, and prices must be passed as strings (msgpack str).
- Fund and account operations (`approveAgent` and `usdSend`) are not phantom agents but **user-signed EIP-712** (domain `HyperliquidSignTransaction` / version `1` / chainId = `action.signatureChainId` / verifyingContract `0x0`).
- `signature_hex` is `r‖s‖v` (65 bytes, `v` is 27/28).
- The asset index is resolved from `meta.universe`. Testnet is BTC=3 and ETH=4, mainnet is BTC=0 and ETH=1 (as per the policy in section 3 of `docs/phase-0/money-and-units.md`, the index is not embedded as a fixed value).

These do not conflict with the contracts in `docs/phase-0/api-contract.md` and `state-machines.md`. They will be added as implementation-specific fixed values in `docs/phase-0/environments.md` and `money-and-units.md`.

## 7. Response to review feedback (2026-09-19)

Response to the points of reading-only reviews regarding the commit difference in S1 (P3×8 items). The behavior of funds and signatures has not been changed.

| Recommendation | Interaction | verification |
|---|---|---|
| Unused CI `corepack enable` | Removed from pocket-ic Job | `bash -n` and workflow visual inspection |
| `pocket-ic-tests` is not subject to linting | Expand host lint to `cargo clippy --workspace --all-targets` | Clean the entire workspace (detect and fix `manual_is_multiple_of` once with this expansion) |
| No version or completeness verification for the obtained binary | Add `POCKET_IC_SERVER_MAJOR` / `POCKET_IC_SHA256` and verify both when using the cache and after retrieval. | Finished with non-zero in the old version of the stub, finished with non-zero due to tampering (digest mismatch), and successfully completed with real binary |
| Double definition of `ActionStateView` | Remove and integrate to `fund::ActionState` | `cargo test -p api-types` |
| Release of the live keysignature helper | Rename `sign_digest_for_tests` / `sign_action_for_tests` and block references from canister crates in `scripts/check-signing-boundary.sh` | Boundary check: 0 successful paths; non-0 ends when violations occur. |
| `preserve_order` function integration propagation | Remove the dev-dependency feature and replace it with the `OrderedJson` on the test side (which preserves document order using a Visitor) | `cargo tree -e features -p hl-sign` has `preserve_order` not included. Fixture comparison remains 11 matches. |
| Test helper hex length unverified | Add an even length check | Success with 11 existing fixtures |
| Dead variant `SigningFailed` | deletion | `clippy --workspace` Clean |

Added regression test: `ordered_json_rejects_floats_and_keeps_key_order` (rejects floating point numbers and preserves key order).

## 8. Response to review feedback (2026-09-21)

Changes addressing the funds flow and read-only documentation review are recorded below.

| Commit | Supported content |
|---|---|
| `8fa3fea` | Add `--locked` to the CI cargo command, add `--all-targets` to the Wasm lint command, and test all members that can be built on the host. Add a step to hash the actions with SHA and verify that no test-only methods (`test_*`) are mixed into the production build Wasm. |
| `97ae049` | Align the allocation signature holders with the master key of the reserve account (which was previously mismatched with the saved account using a new random key `account_id` generated each time), and set `derivation_path` to the actual derivation path. Fix the problem where the balance reference becomes an `Invariant` error due to the double deduction in the withdrawal reservation. Deposit to unknown addresses should be credited to the suspension account, and the controller's `claim_unmatched_deposit` should be used to transfer the funds to the person in charge. Regular reconciliation should only credit the actual amount, and the loop should not be stopped by a single failure, instead iterating using the `(created_at, master_address)` cursor. |
| `46b061d` | Bind the challenge to the caller principal (T-102) and perform the binding confirmation before consumption. Use the nonce of the signed withdrawal intent only once. Reserve recovery against the equity of the trading account and release it upon acceptance or rejection. The deposit into the trading account is confirmed only within the in transit range, and any excess amount is credited directly to the trading account as a deposit. Resolution of unclear actions is also subject to `dispatching`, and proof of inquiry to the trading exchange is mandatory. `classify_sql` only classifies as `Conflict` based on unique constraints. |

Commit records: `8fa3fea` is only for CI. `97ae049` is recorded as PocketIC 21 files and 54 test success in the commit message, and `46b061d` is recorded as 65 hosts and PocketIC 57 test success.

`4c0e5e5` Re-test at that point (this round): **Host 66 successful** (`api-types` 5, `db` 9, `hl-sign` 35〔`lib` 33, `fixtures` 2〕, `hl-types` 17), **PocketIC 21 files and 58 tests**. Add 2 guard tests with `fdf0512` (policy/guard fail-closed) and **60 tests** (real-time testing with `#[test]` and `cargo test -p pocket-ic-tests -- --list`. The test binary added lib single test and Doc-tests to the integrated test file 21 is 23).

Issues resolved in this round: the transmission path of recovery, the scope of `unknown` resolution (including `dispatching`), full stop and loop miss due to negative values in deposit reconciliation, T-102, and the principal binding of challenge.

Remaining unresolved items: size constraints on consistent execution of `control_guard` (full-size Wasm exceeds 2 MiB limit), T-605, testnet unverified, operational procedures for permanent errors, handling failures in periodic sweep timers.

