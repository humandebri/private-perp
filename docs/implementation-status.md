# Implementation status: UI/server base and ADR

For the latest local quality gate, refer to [2026-09-23 verification record](phase-2/local-quality-gate.md). Below, we retain the history of each stage, and we do not treat the number of past test cases or descriptions of synthetic demos as current status.

Phase 3 is referenced in [Local Implementation Progress](phase-3/local-progress.md). It is the stage where the shared REST budget base has been added, but the connection to the transmission path and the overall plan are not complete.

Update: 2026-09-22. Eligible are synthetic demos for local development, Phase 0 contracts, and Phase 1 S1–S3 (within the range that could be verified locally). **testnet and mainnet are unverified**, and all Phases are not complete.

## Phase 1 (S1/PocketIC base)

On 2026-09-19, we implemented and verified the integration test base for the signature component (S1), which is completed locally within Phase 1. At that time, the funding ledger, mock HL communication, and guard were not yet implemented, but **we have since implemented and verified them up to the local scope of S2 and S3**. The latest state is to align the ledger in `docs/phase-1/README.md` with the section at the end of this file.

### Completed ones

- `hl-types`: normalized decimal (`decimal`), canonical msgpack (`msgpack`), action construction (`action`, order/cancel/cancelByCloid/updateLeverage).
- `hl-sign`: Keccak-256, EIP-712 (phantom agent, `Exchange`/`1`/`1337`/`0x0`), action hash (`msgpack ‖ nonce(8B BE) ‖ vault ‖ expires`), deterministic signature and recovery, `v` candidate recovery, user-signed EIP-712 (`HyperliquidSignTransaction`, `ApproveAgent`/`UsdSend`).
- `api-types`: Candid type (shared in the Canister and PocketIC tests) from `docs/phase-0/api-contract.md`.
- `tools/hl-fixture-gen/` (official SDK `@nktkas/hyperliquid@0.33.3` fixed) and 11 files in `crates/hl-sign/tests/fixtures/`.
- PocketIC base: `scripts/fetch-pocket-ic.sh`, `scripts/pocket-ic-test.sh`, `crates/pocket-ic-tests`.

### Verification result (local execution)

- `cargo test`: 66 hosts successful (PocketIC integration test is a separate job) (`api-types` 5, `db` 9, `hl-sign` 35(`lib` 33, `fixtures` 2), `hl-types` 17). `hl-sign` includes a comparison test with the official SDK fixture (11 items). One item of `api-types` is a regression test added with `4c0e5e5`, and at `46b061d` there are 65 items.
- `bash scripts/pocket-ic-test.sh`: **21 files, 60 tests all passed** (`crates/pocket-ic-tests/tests/`). The test binaries are 23, including the single test library and Doc-tests added to the 21 integrated test files.
- Match with the official SDK: action hash (`createL1ActionHash`), signature (r/s/v, definitive), and all recovery addresses match in 11 cases. Since the SDK does not disclose intermediate values (msgpack and digest), they cannot be included in fixtures, and they are verified by matching the signature.
- `cargo clippy --workspace --all-targets -- -D warnings` (all hosts including `pocket-ic-tests` and Canister crate), `cargo fmt --all --check`, `bash scripts/check-no-await.sh`, `bash scripts/check-signing-boundary.sh`: Success.
- `bash scripts/fetch-pocket-ic.sh`: Verifies the server version (`pocket-ic-server 16.x`) and sha256. We have confirmed that if the old version or digest is inconsistent, the program will terminate with a non-zero exit code.
- `cargo build --release --target wasm32-unknown-unknown` (4 Canister): Success.
- `bash scripts/pocket-ic-test.sh`: Deploy 4 Canisters with PocketIC 16.0.0 (arm64-darwin) and `version` query returns a response (including that `init` DB initialization does not fail).

### What remains in Phase 1 (as stated at that time. The local scope has been resolved)

- The `db` schema, Migration, double-entry ledger, reservation, nonce, epoch CAS, `funds_vault` authentication, outbox, HPKE, API, `trading_core` order pipeline and mock HLreconciliation, and the 7-day delay for `control_guard` were all **implemented and locally verified**.
- Local ECDSA spike is completed (the key id for `sign_with_ecdsa` is `test_key_1`). Some of the PocketIC injection of faults and T-xxx tests have been executed (`docs/phase-0/threat-test-matrix.md`).
- **Remaining tasks**: Real-world HL (testnet) order acceptance and reconciliation measurement, size constraints on `control_guard` matching execution (real-sizewasm cannot be sent via a single ingress), testnet forwarding and measurement (requires separate environment and approval), operational procedures for permanent errors. The rollback of timer failures will be resolved on 2026-09-22 and recorded in the Canister logs. T-605 (applying the envelope's personal API) was implemented and locally verified on 2026-09-21 (`docs/phase-2/README.md`).

## Phase 0 (Contract, screen specifications, Rust prototype)

On 2026-09-19, I fixed the implementation contract and screen specifications for Chapter 4 of the roadmap in `docs/phase-0/`, and prepared the Rust workspace prototype and local test environment for Chapter 12-2.

### What I prepared

- 8 contract documents (`docs/phase-0/`): permission table, API contracts and error types, state transitions, amount and uniqueness, environment separation, threat and test support, privacy evaluation input, screen specifications.
- Rust workspace: `Cargo.toml` (dependencies are fully fixed with `=`, `Cargo.lock` is committed), `rust-toolchain.toml`, `icp.yaml`, `crates/` (`hl-types`, `hl-sign`, `db`, `policy`, `funds-vault`, `control-guard`, `trading-core`), `scripts/check-no-await.sh`.
- (At Phase 0) Canister only had `version` and `db::init` (`ic-sqlite-vfs 2.0.0`, Migration is empty), and funds, signature, orders, and reconciliation were unimplemented. **Currently, they are implemented and verified as shown at the end of this file.**
- CI: `.github/workflows/rust.yml` (fmt / clippy (host-wasm) / test / no-await test / wasm build). **Remote CI is not executed**.
- (At Phase 0) 12 tests run on the host (`hl-types` 4, `hl-sign` 3, `db` 5). Currently 66.

### Verification result (local execution)

- Toolchain: `rustc`/`cargo` 1.97.0 (fixed in `rust-toolchain.toml`).
- `cargo fmt --all --check`: Success.
- `cargo clippy --all-targets -- -D warnings` (default host members): Success.
- (At Phase 0) `cargo test`: 12 successful. Currently 66.
- `bash scripts/check-no-await.sh`: ok (no async rule violation in `hl-sign` and `db`).
- `cargo clippy --target wasm32-unknown-unknown -p policy -p funds-vault -p control-guard -p trading-core -- -D warnings`: Success.
- `cargo build --release --target wasm32-unknown-unknown` (4 Canister): Success. Each about 1.25 MB (linked `ic-sqlite-vfs` 2.0.0).
- `icp project show`: Check the recipe expansion of `icp.yaml`.
- `icp build`: 4 Canister success. Embed `candid:service` with `candid-extractor` and `ic-wasm`, and synchronize the extracted `did` and frontend binding to the repository.
- `icp network start -d` → `icp deploy` → `icp canister call <name> version --query`: Verify that the 4 Canisters return `("0.1.0")` and that the `init` DB initialization does not fail. Stop with `icp network stop` and verify the stop.

### Notes on execution in this environment

- The sandbox for this session cannot be written to HOME, so it was run with `CARGO_HOME=<repo>/.cargo-home` and `ICP_HOME=<repo>/.icp-home` specified (both are `.gitignore` files). Not needed in normal development environments.
- Initially `channel = "1.93.0"` was specified, but the MSRV for `ic-sqlite-vfs 2.0.0` is 1.95.0, and because the additional installation of the toolchain failed due to sandbox restrictions, we fixed the already installed 1.97.0 that meets the requirements. The complete fixed version for release will be done in Phase 4.
- The local Canister ID is the value that icp issued for development (under `.icp/`, not committed). The values for testnet and mainnet are undecided.

### What remains in Phase 0

- Real Candid (`.did`), rate limits and upper limits, HL-specific price precision, fees, and final events, and agent generation real-world actions, all are determined by Phase 1 measurements.
- Real table and Migration (Phase 2-1). PocketIC failure test platform (Apple Silicon support for Rust version `pocket-ic` is not confirmed).
- (At Phase 0) No test was executed in the Threat/Test Table (`docs/phase-0/threat-test-matrix.md`). Currently, the tests that can be executed locally have been executed (see the "Executed" column in the same table).

## Completed ones

- ADR 0001~0006. Separated design adoption and experimental status.
- Compatibility with Plan v0.9, Implementation v0.5, and Roadmap v1.1.
- Local delivery of new TanStack Start + React + TypeScript/Vite, Workers + Static Assets.
- pnpm fixed dependencies and lockfile, Workers generation type, Oxlint type support, Oxfmt,tsc, Vitest, Playwright.
- `/`,`/trade`,`/funds`,`/history`. SSR for public pages, account screens are client drawing.
- Lightweight Charts, composite boards, order forms, TanStack Table order list, fund verification and history.
- Normal acceptance, partial fill, rejection, unknown, cancellation competition, simulation of old state.
- Integer computation of composite funds, idempotency of request IDs, memory destruction on logout.
- Workers' GET/HEAD exclusive, real mode 503, example of entrance restrictions by CF country code, security header.
- Added GitHub Actions validation workflow. Remote execution is not performed.

## Verification results

- `pnpm build`: Success. Build for Cloudflare Workers.
- `pnpm typecheck`: Success.
- `pnpm lint`: Success. Type compatible. There is a one-line exclusion for Table v8 due to the absence of React Compiler.
- `pnpm format:check`: Success.
- `pnpm test`: 39 successful results. Includes client/server bidirectional HPKE and AAD mismatch rejection.
- `pnpm test:e2e`: 7 successful. Verify the Workers preview after build with Playwright Chromium.
- Display, operation and console confirmation of the screen by Playwright CLI. Screenshots are saved as the handover result.

We detected and corrected the 404 errors in JS/CSS delivery and layout changes in chart autoSize during the browser test. We do not treat only successful builds as UI completion. Testing for Safari, Firefox, real MetaMask, real ICP, performance load, and real funds was not performed.

## Conditions for unimplemented and stopping the next stage

The Canister code is not a template for `version` and DB initialization alone (it has implemented the business logic for funds, signature, authentication, and orders. Please refer to the end of this file). Candid is fixed to `candid/` and frontend binding has also been generated, but there is no testnet Canister ID. Therefore, we are holding back the ICP connection stage in the live environment.

The following required results:

1. ~~Implementation and verification of funds, signatures, and authentication in Rust/PocketIC~~ → Local scope is completed. testnet verification remains pending.
2. ~~Candid generation~~ and Canister ID, real contract for identity authentication and expiration. network, endpoint, tECDSA key ID are implemented as settings at startup (controller-only setter) and will be confirmed to the real value after testnet deployment.
3. ~~Specification for encryption of HPKE public key acquisition, key update, request and response after authentication~~ → Implementation and verification have been completed up to application to key registry, envelope, and 4 personal APIs (`get_account_snapshot`, `list_orders`, `list_fills`, `cancel_order`). Application to write-only operations such as `submit_order` will be determined in Phase 3.
4. ~~Reconciliation fixture for orders and fund transfers, and recovery contract for unknown~~ → outbox reconciliation and `unknown` resolution have been implemented and verified. `orderStatus` reconciliation automation remains pending.
5. Connection to the HL public market status, connection to account status, positions, PNL, SL/TP, and settlement.

There is a positions preview on the screen, but SL/TP and settlement are displayed as disabled. We do not create real positions based on the fact that they were filled in during the demo. It is not an integer balance model for individual tests; instead, a double-entry ledger and a permanent outbox have been implemented on the Canister side.

## Remaining items before the production deployment

CSP including funding security, confidentiality, correlation resistance, SNS/guard, controller transfer, legal, eligibility issuance, distribution permission separation, nonce support, and reconfirmation of dependency licenses and NOTICE, independent audit is required. The risk of JS changes carried by Cloudflare distribution permission remains.

Cloudflare public release, SNS launch, controller change, wallet connection, real funds operation have not been carried out.

## Canister implementation status for Phase 1 (S2/S3) (2026-09-21, local verification)

On the Canister side, it has become not just a template for `version` and DB initialization. Locally for the fund layer (S2) and control (S3).
The verification range is working, and **29 files and 83 exams were all successful** in PocketIC (Phase 2 on 2026-09-21).
After adding the generalization of environmental settings after 2C/2D/2E+T-605+main pipeline+Phase 1 at 21 files and 60 tests). However, **Phase 1's
Go/No-Go is not passed** and testnet reverse is not performed.

### Verified (evidence: `docs/phase-1/evidence/P1-001`~`P1-010`)

| an area | State | Evidence |
|---|---|---|
| EOAauthentication (challenge, session, expiration, origin binding, principal binding T-102) | Installed and verified | P1-001〜003 |
| Reference, receipt, reservation, double-entry ledger of funds | Installed and verified | P1-004 |
| Deposit recording (`/info` transmission route / suspension recording for unknown recipients / direct debit to the person by the controller) | Installed and verified | P1-004 |
| outbox (claim→real tECDSAsignature→`dispatching` persistence→non-replicated POST→reconciliation) | Implemented and verified with allocation, disbursement, and recovery. | P1-006 |
| Handling of unclear transfers (not resending, not releasing even after a certain time has passed, resolving `unknown`/`dispatching`) | Verified (T-205/T-206) | P1-006 |
| Saving authentication, ledger, and unresolved actions during upgrade | Verified | P1-010 |
| HPKE (keygeneration update, public key distribution, envelope encryption, `aad` binding) | Installed and verified | P1-007 |
| `control_guard` (SNS exclusive, 7-day grace period, content matching, no bypass API, single-execution consistency) | Implemented and verified (execution of real-sizewasm is pending under the following restrictions) | P1-009 |
| `policy_registry` (only fail-closed and stop-direction) | Installed and verified | P1-010 |
| `trading_core` (authorization boundary, order reception, agent key signing, sending, cancellation sending, fill receipt, `orderStatus` reconciliation, snapshot, risk reservation, SL/TP, full settlement, personal API envelope, environment settings) | Installed and verified | P1-008 |
| Environment separation (network, endpoint, tECDSA key ID startup settings, mainnet rejection = E-2) | Implemented and verified (E-1 is not performed because eligibilityunimplemented) | `core_environment.rs` and `vault_environment.rs` |

The local threshold ECDSA is enabled in PocketIC's **test threshold key subnet** (key id `test_key_1`).
The signature round trip on PocketIC is about 17.9ms (not the performance value of the production subnet).

### Unresolved / unverified (next stage)

1. `trading_core`: Receipt (authorization, idempotency, allowlist, metadata tagging) → signature via agent key → `dispatching` persistence → non-replicated transmission → Receipt (`open` + `oid`) / rejection / unknown classification, `get_account_snapshot`, inclusion of fill (power-of-zero with `tid`), `orderStatus` reconciliation, risk reservation, rejection of receipt during emergency halt, input validation, and verification up to sending of cancellation. SL/TP (`positionTpsl` and reduce-only) and full settlement/partial settlement (`close_position` and `close_all`), mandatory envelope for personal APIs and key updates, are all verified. After receipt, **transmission, cancellation,`/info` reconciliation (userFills, clearinghouseState, orderStatus)** and account traversal** were moved to the productionwasm `pipeline`/`venue`, and driven by a global timer (only for production, every 5 seconds) and `sweep` (controller manual) (2026-09-21). **What remains is the receipt behavior on real HL (testnet) and the measurement of timer cycle costs.**
2. 2C: Received by the deposit side (including the transport path), sending of the withdrawal (release with `payout_settled` / rejection + reverse accounting / hold in case of unknown), resolution of `unknown` reconciliation, sending path for recovery (reservation against the equity of the trading account and branching for acceptance, rejection, and unknown cases. Confirmed in the 3 tests of `vault_recovery.rs`). **What remains is to reproduce the 60-second timeout and real HL acceptance on testnet.**
3. ~~Envelope application to the personal data API, response encryption, and handling during key updates (T-605)~~ → Resolved on 2026-09-21 (mandatory envelope for 4 methods, `request_id` resend refusal, `aad` binding, refusal of old envelopes during key updates. Test `core_hpke.rs`). Remaining: Encrypting is only done for `get_account_snapshot`, `list_orders`, `list_fills`, and `cancel_order`; `submit_order`, `cancel_all`, `close_position`, `close_all`, `request_agent_generation`, and `get_agent_status` are plain text (only session authorization).
4. Consistent execution of `control_guard`: The uniqueness of execution paths and concurrent execution has been verified in minimalwasm. However, because the actual size of awasm exceeds the 2 MiB limit as an argument to `execute_upgrade` (actual measurement 2,193,336 bytes), chunking or code registry is required (**not resolved**).
5. Remaining failed tests (T-401-T-410, etc.) and reading-only review after Phase 1 completion.
6. Operational remaining issues: **Operational procedures for permanent errors (e.g., digest mismatch) when they occur** are undefined. The failure of regular sweeps has been resolved by recording it in the Canister log, but the monitoring and notification pathways are unimplemented.
7. testnet: real HL acceptance behavior, signaturep50/p95, acceptance→HL acceptance, the validity of Confidential Subnet. **unverified**.
