# Phase 2: Single user testnet MVP (planning and progress)

Implementation plan in `Implementation-Roadmap.md` §6. Prioritize **transmitting funds on testnet** over the finer details of Phase 1 implementation.

## GATE 0: Prerequisites (if not met, do not proceed to testnet)

| # | a premise | State | Required operation |
|---|---|---|---|
| G0-1 | Identity and cycles for IC testnet, 4 canister deployment approval | **Not yet** | Create identity with `icp identity` → obtain cycles with `faucet` → `icp deploy` (`funds_vault`/`trading_core`/`control_guard`/`policy_registry`) |
| G0-2 | HL testnet account + test USDC, MetaMask | **Not yet** | Receive USDC on HL testnet faucet, add testnet to MetaMask |
| G0-3 | Confirmation of testnet tECDSA key ID (`test_key_1` first candidate, unconfirmed) | **Not yet** | After deployment, measure `ecdsa_public_key` and record it to `docs/phase-0/environments.md` |
| G0-4 | HL testnet restrictions (minimum amount, fees, final events) | **Not yet** | Real-time measurement of deposits and withdrawals in one round |

## a milestone

| M | Content | a gate |
|---|---|---|
| M1 | GATE 0 | 4 canisters started on testnet and key id confirmed |
| M2 | 2A Generalization of Environment Settings / 2B Application of Individual API for HPKE envelopes | PocketIC＋E-1/E-2 |
| M3 | 2C positions, PnL, SL/TP reconciliation／2D cancellation, Cancel All, position closing／2E freshness gate | PocketIC, alternative flow locally |
| M4 | 3A Wallet authentication / 3B IC connection + envelope client / fund flow | Deposit on testnet → allocation → recovery → withdrawal |
| M5 | 3C trading screen / 3D abnormal state | **Local implementation completed**. Testnet acceptance is after GATE 0. |
| M6 | 3E Minimum Alternative Client/Playwright/Measurement Report/End Review | **Local implementation is complete**. The test results report will be added after testnet. |

## Things that are not handled in Phase 2
Deferred: Phase 3 and later work (multi-user isolation, load, backup restoration, cycles alerts, eligibility, audit, and retention cleanup), real funds and mainnet (E-2 tests rejection), and Phase 1 refinements such as UI polish. The fixed window in `reconcile_all`, which excludes older accounts when there are at least 3 deposit destinations, is a real bug and must be fixed by M3.

## Remaining implementation (1–3 implemented and locally verified on 2026-09-21)

### 1. SL/TP (Trigger Order) — **Completed**
- Added `trigger_is_market` to `orders` with a CHECK requiring all 3 trigger fields (`trigger_is_market`, `trigger_kind`, and `trigger_price`) together. `trigger_kind` also represents `tpsl`. Connected this through `NewOrder`, `SignableOrder`, `OrderSummary`, and `OrderView`.
- Acceptance verification (`submit_inner`): `reduce_only` is mandatory, the trigger price must be the actual price and the accuracy (same conditions as the price), the existence of positions and `side` must be opposite to the positions. Up and down movements relative to the market price are not checked at acceptance because the core does not have a mark price (the exchange makes the final judgment).
- signatureaction: Output `OrderType::Trigger` + `Grouping::PositionTpsl` in the common path of `order_action` (for msgpack) and `order_action_json` (for sending JSON). Also include the trigger in the reception fingerprint.
- Test: `crates/pocket-ic-tests/tests/core_triggers.rs` (receipt verification, signature action consistency, message body, `orderStatus` reflection).

### 2. Full payment/partial payment (reduce-only) - **Completed**
- The receipt and registration of `submit_order` were extracted to `submit_inner(user_id, args)`.
- Add `close_position(session, client_request_id, market, ratio_bps, limit_price)` and `close_all(session, client_request_id)` (`limit_price` is the slippage limit. If omitted, the mark price is approximated from the observed positions). The quantity is rounded down to `szDecimals` by multiplying the number of positions by the ratio.
- `reduce_only` has excluded risk reservations and freshness gates (it is more dangerous if you cannot make a settlement or protection when there are positions). Emergency suspension will also stop the settlement as usual.
- Replace all the positions taken into account (no positions that have been settled will remain). The receipt ID for `close_all` is derived from `client_request_id` and the stock name.
- Test: `crates/pocket-ic-tests/tests/core_close.rs` (opposite trade, partial ratio, full settlement, positions0 with send and fill).

### 3. Apply HPKE envelope personal API (T-605) - **Completed**
- Transfer the envelope implementation to the shared crate `crates/hpke-envelope`, and reuse `funds-vault` and `trading-core` in the same crate.
- Add `hpke_keys` (v7) and `hpke_requests` (v8) to core and implement `rotate_hpke_key` (only for controller) and `get_hpke_public_key`.
- `get_account_snapshot`, `list_orders`, `list_fills`, and `cancel_order` are **mandatory envelopes**. `key_id` (current public key), `network`, `canister`, `method`, `caller`, and `request_id` are used to bind the expiration date, and `aad` is recalculated to verify consistency (tampering results in decryption failure). `request_id` is recorded as a single-use entry upon successful decryption and is rejected for resending. The response is sealed to `client_public_key`.
- Not applicable: `submit_order`, `cancel_all`, `close_position`, `close_all`, `request_agent_generation`, `get_agent_status` (because the scope of application of Contract §6 is defined by 4 methods).
- Test: `crates/pocket-ic-tests/tests/core_hpke.rs` (normal bidirectional, expiration, tampering, caller/canister/network/method binding, request_id reuse refusal, key update).

### 4. Live order pipeline and reconciliation (M3 balance) — **Completed**
- We moved the signature, sending, and reconciliation functions that were limited to `test-venue` to `crates/trading-core/src/venue.rs` (outcall and conversion) and `pipeline.rs` (sending and reconciliation), and made them work in productionwasm without features. `submit_order` only confirms acceptance (`pending`) as usual.
- Sending (`/exchange`) is a non-replicated POST. Acceptance is `open` + oid, rejection is `rejected` + risk reservation release, unknown outcome is `unknown` (do not **resend** and do not release reservation). Cancellation is also sent through the same path.
- reconciliation (`/info`) is replicated outcall + deterministic transformation function `transform_info` (only keep the elements needed by the purpose). `userFillsByTime` is taken into power by `tid`, `clearinghouseState` is replaced by the total number of observations, and `orderStatus` is reflected only in the non-terminal orders that can be identified by `oid`.
- The startup is done with the global timer (`ic-cdk-timers` `set_timer_interval` only for production builds, 5-second intervals), and is rearmed with `init`/`post_upgrade`. `sweep` (controller manual) calls the same `sweep_once`. Heartbeat is not used (it is called every round, and it also incurs cost even when idle). In the test build, automatic sweep is not set up, and it is driven definitively with `test_sweep_now` (to avoid conflicting with the outcall where the test waits in PocketIC).
- The reconciliation target is an active account that has the trading exchange address saved in `accounts`, and it is traversed by a cursor in order of `account_id` (do not traverse by fixed window). The address is obtained and saved from the vault only once during the processing of the signature-verified request of the owner (`cache_trading_address`).
- Limit: Send 4 items, cancellation 4 items, reconciliation 2 accounts, order status 4 items per account. The result is returned as `SweepOutcome{dispatched, cancels, reconciled}`.
- Release of risk reservation: Release the reservation at the time of the order becoming terminal (fill, cancellation, rejection) (`state-machines.md` section 5). If not released, the reservation will remain permanently and will exhaust the new order limit for equity. The margin of positions is represented by the observation of the trading floor (`clearinghouseState`), so the reservation represents the risk of unfilled orders.
- Test: `crates/pocket-ic-tests/tests/core_pipeline.rs` (forwarding/reconciliation of send→reconciliation, rejection/unknown classification and non-resend, reservation release after fill, 3 account rotation, controller-limited manual sweep). Existing send tests are driven by `venue_router_default` (only specify send response, reconciliation is default response).
- not performed: Sending to real HL is after GATE 0. The automatic timer is only set up for the live build, so the interval, rearm, and periodic cost are measured during testnet deployment (even if the timer is lost, it can be restarted with a permanent state and manual `sweep`). The cycles budget limit requested by the 5th paragraph of `state-machines.md` (such as pausing the sweep if the remaining cycles are below the threshold) is only a count limit and is unimplemented. The margin of positions does not include the `marginSummary.totalMarginUsed` from the trading exchange, and `margin_used` represents the total reservation of unfilled orders.

### 5. Generalization of environmental settings and E-2 (M2 2A/remainder) — **Completed**
- Moved the network, HL endpoint, and tECDSA key ID from the build constants to the **configuration at startup** (section 4.1 of `docs/phase-0/environments.md`). `funds_vault` has `set_network`, `set_venue_endpoints`, and `set_ecdsa_key_id`, while `trading_core` has `set_market_context` (adds network validation), `set_venue_endpoints`, and `set_ecdsa_key_id`, both dedicated to the controller. `get_environment` is a public diagnostic query.
- Verification is centralized in pure crate `hl-types::environment`: **reject mainnet** (do not handle real funds in Phase 2), reject if the endpoint host does not match the network (also reject lookalike domains), reject the real venue host in local settings, format verification of key ID. Host testing is fixed.
- The default settings are `local`, `loopbackendpoint`, and `test_key_1`. `outcall` (`/exchange`,`/info`) and `signaturekey` are resolved from the settings at the time of invocation (`venue.rs`, `crypto.rs`).
- Tests: `core_environment.rs` and `vault_environment.rs` (mainnet rejection, inconsistent rejection, `get_environment` and **the configured endpoint becomes the actual outcall URL**), host tests for `hl-types`.
- E-1 (mock eligibility token) is **not performed**: eligibility issuance is done in Phase 3, and the token does not exist. E-5 (mock endpoint real-world inclusion) is structurally satisfied by setting it only in the configuration without embedding the mock endpoint into the code.

### Local connection (assuming M4 3A/3B and built on 2026-09-22)

To connect the screen and canister without waiting for testnet (GATE 0), to the local network
Deployed and scripted the initial setup. Candid is extracted fromwasm and fixed in the repository.

```sh
# 1. Extract Candid (.did) (wasm without real features. Also inspect the inclusion of test-only entry points)
bash scripts/extract-candid.sh

# 2. Local network and identity (use ICP_HOME for project-specific)
export ICP_HOME="$PWD/.icp-home"
icp identity new private-perp-local --storage plaintext   # Only the first time
icp identity default private-perp-local                   # Only the first time
icp network start -d
icp deploy --yes

# 3. Initial settings (policy's allowlist, core/vault's network/endpoint/key id/HPKEkey)
bash scripts/bootstrap-local.sh
```

- By directing `ICP_HOME` to the project, **do not change the identity and default of other projects**. `icp-home/` and `icp/` are subject to `.gitignore` (do not commit the key and Canister ID).
- The Canister ID is fetched using `icp canister status <name> --json` (it can change for each deployment, so do not hardcode it in scripts or UI). `PUBLIC_CANISTER_ID:<name>` is injected into the canister.
- `candid/*.did` is the output of `scripts/extract-candid.sh`. Rust's contract (`api-types`) and Candid's mismatch are detected by re-executing this script.
- The screen (`frontend/`) is connected to the local canister. As M5/M6, it verifies the public market status WS, positions, SL/TP, cancellation, partial/full settlement, abnormal conditions, and even the minimum client using local mock. The local endpoint can be verified with `get_environment`.

### Local M5/M6 (2026-09-22)

- Public market status is directly connected to mock HL's WebSocket, and `BroadcastChannel` and Web Locks consolidate multiple tabs' connections into one. Subscription does not include EOA or trading account.
- We separated the `totalMarginUsed` and unrealized PnL of clearinghousereconciliation from the core unfilled order risk reservation. The freshness is calculated from the account observation time rather than the final fill.
- `/trade` handles SL/TP, Cancel All, 25/50/100% settlement, all settlements, pending, unknown, and stale. `/fallback` only provides authentication, cancellation, and withdrawal.
- These are local synthetic fund validation and do not mean the completion of GATE 0 or testnet fund transfers.

## Operational precautions (parallel work countermeasures)
- PocketIC must be executed via a script: `POCKET_IC_TEST_DIR=$PWD/target/test-venue-mine bash scripts/pocket-ic-test.sh --test <name>`.
- The raw `cargo test` will fail falsely because it reads the productionwasm (without feature).
- The envelope test uses the controller to call `rotate_hpke_key` to generate the key (ungenerated personal APIs are rejected with fail-closed).

The fill history is obtained using a time range and a persistent cursor, with a maximum of 2,000 entries per request and a response limit of 1 MiB. It is re-obtained including the boundary timestamp of the cursor, excluding duplicates based on the tid. When restoring the oid of an order that was unknown, the history cursor is returned to the time of order creation within the same transaction and saved as pending acquisition. It will not lose retroactive acquisition even in subsequent communication failures or upgrades, and the same processing will be applied when the restoration journal is re-applied. If the limit is reached at the same time, the cursor will not be advanced and will be retained as incomplete reconciliation. Queries about the order status are processed in the order of the final reconciliation timestamp, and fill acquisition failures are handled in the order that do not interfere with the order status reconciliation. The fund history cursor is a combination of timestamps and request IDs.
