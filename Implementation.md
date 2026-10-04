# private-perp implementation plan

> Historical design record. Dates, decisions, estimates, and verification status refer to the original record. See [implementation status](docs/implementation-status.md) and [single-canister architecture](docs/phase-3/single-canister.md) for later changes.


- Version: v0.5 (UI base implementation, not yet approved for production)
- Last update: 2026-09-18
- Scope: Implementation architecture, repository configuration, persistence design, order pipeline, client design, validation planning, task decomposition
- Exclusion: screen specifications, text, legal judgment

`Plan.md` defines what to build and who can perform each operation. This document defines how to implement it. When they conflict, the authority model and invariants in `Plan.md` section 3.2 take precedence. Implementation convenience must not weaken those invariants.

v0.5 implements the B (confidential fund layer + user-specific HL accounts) based on the `Plan.md` v0.9 implementation baseline. Local and testnet implementation can begin in accordance with the finalized specifications in Chapter 16. The production gates for fund safety, privacy, confidential infrastructure, governance, and legal review is not yet fulfilled, and it is not for accepting real funds or approving deployments. The frontend is a synthetic demo, and the ICP backend is unimplemented.

---

## 0. Additional decisions

In addition to D1 to D8 in `Plan.md`, we decided the following as implementation guidelines.

| # | the point | Decision | Influence |
|---|---|---|---|
| D9 | signature path | Orders are handled by the trading_core Agent, and fund transfers and Agent approvals are handled by the funds_vault master. The browser EOA is used for login, deposit, and withdrawal intentions. | Do not transfer the funding key to trading_core and do not provide the optional digestsigning API. |
| D10 | The configuration of trading_core | Maintain Confidential Subnet (D7 maintenance). Accept delays | The signature is a cross-net call to `pzp6e` each time. Analyze the structure in Chapter 2. |
| D11 | Real-time route | Direct connection between the public market status and the browser↔HL WS. The personal data is encrypted polling authorized by Canister. | Do not directly link the browser IP and trading account to the user-related WS. Measure HL reconciliation costs and delays. |
| D12 | Sustainability | Only `ic-sqlite-vfs`. Do not use `StableBTreeMap` | While the implementation becomes simpler, we entrust all states to young dependence. Establish mitigation measures at 4.9 |

Combining D9 and D10 means that "all orders will be waiting for cross-net signature". This is the strongest restriction in the current plan and will be dealt with in Chapter 2.

With D9, the browser does not sign HL orders or Agent approvals. It only uses wallet signatures for connecting EOA authentication, deposits from the user's HL account, and approval of withdrawal intentions.

The location of the signature, communication path, and fund path are different design decisions. We do not say that "we can only choose canister signing to provide privacy." This time, we finalized D9 in accordance with B's fund and key management, but even if we consolidate the signature into Canister, the public transfer link will not disappear.

---

## 1. confidential fund layer + architecture of user-specific HL accounts

### 1.1 Pathway diagram

```
[User's browser]
  │
  ├─(A) Only public market conditions ──WS directly connected──▶ Hyperliquid
  │       wss://api.hyperliquid.xyz/ws         (D11)
  │
  ├─(B) authentication/withdrawal intention ──EOA signature + encryption──▶ funds_vault
  │        └─ Deposit is done by signature transfer from the HL account of the person depositing to the shared reserve account.
  │
  └─(C) Order/cancellation/personal data ──authentication + encryption──▶ [trading_core @ Confidential Subnet]
                                                     │
                                                     ├─ authorization, limit, deadline (synchronous)
                                                     ├─ ic-sqlite-vfs (synchronous transaction)
                                                     ├─ await sign_with_ecdsa ──xnet──▶ [pzp6e]
                                                     ├─ await non-replicated HTTPS outcall ──▶ Hyperliquid
                                                     └─ await /info reconciliation ──▶ Hyperliquid
```

### 1.2 Responsibilities for each route

| a route | What will flow? | Who will verify it? |
|---|---|---|
| (A) | Only public market conditions | Browser. Do not be the authority on risk assessment. |
| (B) | authentication, fund request, master action, fund reconciliation | funds_vault, Hyperliquid. Agent approval is also executed by funds_vault. |
| (C) | Order intention, signed action, reconciliation result, encrypted identity status | trading_core |

### 1.3 Things that cannot be passed through the canister

- Market data. When it is transmitted, the replicated HTTPS outcall will rotate consensus every second. The outcall cost and consensus load for 7 nodes will be borne, and latency will also deteriorate compared to HL's original.
- Bridge outside of the service. The operation of preparing USDC in the user's HL account outside the service is carried out by the user.

Personal data and fund transfers are conducted through Canister. It balances avoiding market updates and not directly checking account information from the browser.

### 1.4 The role of ICP and design boundaries

In B, ICP handles the customers' funds' master signing, non-public ledger, agent signing, and recovery. There is still a risk of Canister being able to call the signing API. Proof of confidentiality from non-custodial or operator sources cannot be proven solely by the decentralization of the key.

- Connect directly to HL only for market conditions. Personal data is used via D11, and orders and cancellations are processed through D9.
- HL standard SL/TP uses HL trigger orders. It doesn't monitor prices in a canister and rebuild the same mechanism. Only a unique strategy without users requires a different execution engine.
- ICP is not required for front-end distribution.
- B is adopted but public fund transfer remains. Correlation resistance is evaluated in Plan 16.6, and D13 will not be achieved solely through Canister custody.
- The risk restrictions of Canister are only applicable to orders in this service. It is not possible to strictly restrict the entire account by using only the local DB when there is direct trading by the user himself or other agents.

### 1.5 implementation boundary of confidential fund layer

The initial funding route will be the HyperCore USDC cross-back from Plan 16.2. Instead of consolidating everyone's HL positions into a single position, fill, margin, and liquidation will be delegated to the HL standard.

- The common Treasury and HL trading account of confidential fund layer are separate components. They do not lead to omnibus trading from adopting fund layer.
- The Treasury/FAR/IMT for NEAR can be confirmed in the official specifications, but the key management and account allocation for near.com perps are speculative. Do not copy the reference structure as an implemented proof.
- The master key of fund layer is held by funds_vault, and the agent key is held by trading_core. User-independent fund recovery is not guaranteed.
- Place the order DB and the funds DB in separate Canisters. Separate the state machine for receiving, booking, external execution, and reconciliation of fund transfers in Chapter 14.
- Cross-reference links across withdrawal, Agent approval, account-specific WS, API responses, cloid, and audit logs. While complete resistance to correlation between amounts and timestamps is not mandatory, do not confuse it with the risk of guessing direct public links.

At the boundary between Chapter 16 of the Plan and Chapter 14 this document, implement the funds ledger and master action for testnet. Do not reuse the B estimate of the traditional Agent-only period.

---

## 2. Latency design

### 2.1 What is the delay?

The "HFT is not covered" in `Plan.md` was about performance goals, but with D9/D10 it became a problem that **directly affects the UX of normal manual orders**. The structure is as follows.

| a section | Content | Known value |
|---|---|---|
| Browser → Boundary node → subnet | ingress | Hundreds of ms |
| Authorization and risk verification | Synchronous/pure | Can be ignored |
| sign_with_ecdsa | Cross net (confidential → pzp6e) | **Not tested** |
| HTTPS outcall POST | Non-replicated, TLS | Hundreds of ms |
| reconciliation `/info` | replicated POST＋transform | Hundreds of ms |

DFINITY engineers responded that if the canister that calls `sign_with_ecdsa` is placed on the signing subnet, cross-network latency can be completely avoided. The production key `key_1` is deployed only to the fiduciary signing subnet `pzp6e` (34 nodes), and the test key is located at `fuqsr`. **Because this workaround cannot be taken in the D10 configuration**, cross-network latency is paid for each time.

Reference: [Sign with ECDSA takes 12+ seconds](https://forum.dfinity.org/t/sign-with-ecdsa-takes-12-seconds-and-costs-0-03/58325) [Chain-key Signing Performance Improvements](https://forum.dfinity.org/t/chain-key-signing-performance-improvements/64672)

### 2.2 capacity restrictions on signing subnet

- The tECDSA maximum throughput for `pzp6e` is **approximately 3.5 sig/s across the subnet (if 100 pre-signatures are available)**. This is shared across all ICP apps.
- The `max_queue_size` for the inquiry queue `ecdsa:Secp256k1:key_1` is 20. If the pre-signature is sufficient, it can dynamically accept up to 100 simultaneous requests. If it overflows, **signature requests will be rejected**.
- As of now, there is no signature rate limit in canister units (DFINITY, 2026-05).
- The signature price is about 26.15B cycles (about $0.035). The price reduction is under discussion but not performed.

**One signature per order uses subnet-wide shared resources.** Assume queue overflow during bursts and retry preparation failures without treating them as terminal order failures (5.5). Batching multiple orders in one action saves signing slots as well as cost.

### 2.3 Separate acceptance and external execution

`submit_order` verifies the ownership, eligibility, input size, metadata, and risk reservation of the caller and HL accounts. It stores user-level request ID, text fingerprint, cloid, and order intent in the same synchronous transaction and returns the receipt result. Receipt is not accepted by HL or filled. `raw_rand` is asynchronous, so it should not be mixed with synchronous receipt processing. It allocates cloid from a pre-supplied secure random number and rejects receipt when the cloid is exhausted.

Background processing follows the following order.

1. Get queued orders and only consolidate them into action for orders that are compatible with the same account, Agent generation, network, grouping, etc. Persist nonce and immutable signing payload.
2. Get the worker epoch and proceed to signing, requesting the signature.
3. Compare epoch and state via callback, re-verify cancellation requests, Agent generation, expiration, kill-switch, policy freshness, metadata, and risk reservation. Discard results of expired workers.
4. Save the signature and the exact transmission payload and proceed to signed.
5. Re-verify just before sending, save the dispatching and sending intent with the same IC message by CAS, and then issue POST. Do not place another await in between.
6. Store the response as a reference for reconciliation. After dispatching, it will be dedicated to reconciliation and will not be treated as unsubmitted timeouts or callback traps.
7. Reconcile the action results and the lifecycle of each order in HL. Also process cancel as an independent signature action.

The launch of sweep is done with the global timer (rearmed with an upgrade), and it limits the number of items, cycles, and API budget. Queued/signing/signed can update the epoch and recover, but dispatching/unknown can only do reconciliation. The state is permanent and does not assume the correctness of the spawn or timer continuation.

### 2.4 Go/No-Go gate (number) of Phase 1

Measure from the Confidential Subnet (re2t4) and then determine.

| Real test p95 (Reception → HL reception confirmation) | judgment |
|---|---|
| Less than 2 seconds | Accept as a manual order. Activate market orders as a default. |
| 2 to 5 seconds | Use limit orders as the guiding line. Market orders must include a slippage warning. |
| Over 5 seconds | The UX for general use is unsatisfactory. It does not automatically weaken confidentiality, and it redesigns the cause and processing capabilities by limiting testnet to the designated entity. |

Do not proceed to Phase 2 without measuring the "untested" item. This is the first Go/No-Go in the original plan.

### 2.5 Supplementary information on confidentiality

In CrossNet signature, instead of plain text for action, a 32byte digest is provided. However, since the signature request also includes metadata such as key ID and derivation path, we do not claim that it only contains a digest or that it cannot be associated with anything. The order text is handled by the client, the decrypting Canister, and the HL of the execution destination. D10 protects the processing within the Canister and does not hide the order and position on the HL.

The content of HTTPS outcall is disclosed to the destination HL. Verify where the content is handled in replica, adapter, proxy, or TLSterminal, and do not assume that all paths are within TEE until otherwise (`Plan.md` 8.3.3). Also, even if the order is processed through Canister, the IP exposure from path (A)/(B) remains.

---

## 3. Repository configuration

### 3.1 workspace

```
private-perp/
├── Cargo.toml                    # workspace
├── icp.yaml                      # icp-cli settings
├── crates/
│   ├── hl-sign/                  # Pure and non-async. Signature and action construction
│   ├── hl-types/                 # Shared type (action, meta, order)
│   ├── db/                       # ic-sqlite-vfs Wrapper. Sync only
│   ├── policy/                   # policy_registry canister
│   ├── funds-vault/              # master key, authentication, double-entry ledger, funds outbox
│   ├── control-guard/            # Change reservation via SNS and 7-day delay
│   └── trading-core/             # trading_core canister
├── frontend/                     # TanStack Start + React + TypeScript / Workers
├── docs/adr/                     # Reasons for hiring, disadvantages, conditions for reconsideration (6 items)
├── research/                     # Investigation records (not applicable to implementation)
├── Plan.md
└── Implementation.md
```

### 3.2 Creation and responsibility boundaries

| Create | duty | Dependency constraints |
|---|---|---|
| `hl-sign` | action construction, msgpack encoding, EIP-712 hash, v recovery, number normalization | **Do not include `async` at all.** Pure functions only. Test vectors included. |
| `hl-types` | Hyperliquid request/response type, `meta` parsing | purity |
| `db` | Schema, Migration, `Db::update` wrapper, CAS helper | **Do not include `async` at all.** Do not include `call_perform`/`ic_cdk::call` |
| `policy` | Country/Terms version/Verification key/Emergency stop/allowlist | Small. Reading failure is fail-closed |
| `trading-core` | Order authorization, state machine, spawn, sweep, outcall, reconciliation | Only allow trading agent signing |
| `funds-vault` | authentication, ledger, master signing, withdrawal, allocation, reconciliation | Verify owner authorization. Optional hash signing API is prohibited. |
| `control-guard` | Change reservation, 7-day grace period, allowed upgrade execution | Only allow reservations from SNS governance. Do not store customer information. |

Fixing `hl-sign` and `db` to be non-async is necessary to **conform to the type and CI constraints of `ic-sqlite-vfs` (not crossing `await` within a transaction)**. The `ic-sqlite-vfs` core rejects `.await`, `async fn`, `call_perform` `ic_cdk::call` `call_raw` under `src` in `scripts/check-no-await.sh`. Apply the same test to `hl-sign` and `db`.

### 3.3 Synchronous and asynchronous boundaries

- Non-async crate: `hl-sign`, `hl-types` `db`
- Clases with async: `trading-core`, `funds-vault`, `control-guard` `policy`
- `trading-core` always follows the order of "complete 1 sync block and then `await`" when calling the `db` function.
- Reviewing considerations: There is no path from the `Db::update` closure to `await`.

---

## 4. Persistence (`ic-sqlite-vfs`)

### 4.1 Selected version

| an item | cost |
|---|---|
| Create | `ic-sqlite-vfs` |
| a version | `2.0.0` to **fully fixed** (`=2.0.0`) |
| Stable layout | v8 |
| feature | `sqlite-precompiled` (for Wasm build) |
| Repository | https://github.com/humandebri/ic-sqlite-vfs |

`2.0.0` is a destructive stable layout change (v8). The segmented page-map image in v6 cannot be directly opened. **import/export/compact are not publicly available in the Rust facade or canister.** This is a restriction of the standard migration API for raw images, and it does not mean that logical export via SQL is impossible. App-specific consistent backup and recovery paths will be designed and verified separately.

### 4.2 MemoryId allocation

Do not change MemoryId **during the lifespan of the deployed canister**. 255 is reserved by the bundled MemoryManager compatible layout, so the app uses only `0..=254`.

| MemoryId | a use | canister |
|---|---|---|
| 0 | Main DB (users, agents, orders, order_events, nonces, audit) | trading_core |
| 1 | Reservation (future independent image. Record in slot catalog) | trading_core |
| 120 | policy DB | policy_registry |
| 0 | authentication, fund ledger, fund outbox | funds_vault (separate Canister) |
| 0 | Change reservation and execution record | control_guard (separate Canister) |

`MemoryId::new(120)` is merely an "new destination" convention compatible with ic-rusqlite. Do not point existing ic-rusqlite images to this slot.

Call `Db::init(memory)` from **both** `#[ic_cdk::init]` and `#[ic_cdk::post_upgrade]`, before migrations or DB access. Use `MemoryManager::init_strict` in upgrade-sensitive code; never silently initialize a nonempty foreign layout.

### 4.3 Persistence model and constraints

This is the mandatory data model before implementation, and the unverified CREATE TABLE is treated as a completed migration. Quantity and price are stored as normalized decimal strings or range-verified integers, without using floating-point decimal points.

| a table | Required fields and restrictions |
|---|---|
| users/accounts | user_id, random account_id, HL master, ownership verification time, status. Maintain caller response information in a private manner. |
| agents | account_id, generation, agent_address, derivation_path, approval/deactivation/expiration/reconciliation time. UNIQUE(account_id, generation), UNIQUE(agent_address). revocation generation is non-reusable. |
| requests | user_id, client_request_id, canonical_body_hash, reference to the reception result, created_at. UNIQUE (user_id, client_request_id). If the content is different with the same ID, there will be a conflict error. |
| actions | action_id, account_id, agent_address, generation, network, nonce, expires_after, canonical_action, signing payloaddigest, signature, wire_payload, dispatch_state, worker_epoch, lease_until, attempt, policy_version, dispatch_started_at, next_check_at, response summary. UNIQUE(agent_address, nonce) |
| orders | order_id, request reference, cloid (16 bytes UNIQUE), symbol/asset index, direction, kind, price, original quantity, reduce_only, trigger conditions, venue_state, cumulative fill amount, hl_oid, cancel_requested. Do not overwrite the order quantity with the fill quantity. |
| action_orders | action_id, order_id, operation type, annotation in action. Relate the results of orders, cancellations, and corrections on an order unit. |
| nonces | agent_address (PRIMARY KEY), last_nonce. Updated in the same transaction as action generation. |
| risk_reservations | account_id, reference to request/action, reservation content, status. Perform reception and release atomically. Do not release reservations for uncertain orders only based on the deadline. |
| order_events | Order/action reference, time, old/new status, limited reason code. Do not copy plain payload. |
| meta_cache | Real-time data of stocks/precision/decommissioned status that maintain network/DEX, acquisition time, digest, and footnotes. Orders cannot be verified by digest and only by the number of items. |

External keys, CHECK for state, and an index for bounded search for the worker/reconciliation target and user list are set up for all references. The signing payload, signature, and sending payload are accessed as confidential data with access control. The invariant condition that the signature is NULL before the signature and the sending payload and signature are required after dispatching is checked in the DB operation layer. The mandatory fields are checked for each order type, and Market is constructed as an IOC value with a page limit for HL.

After separately confirming the completion of the terminal and action reconciliation for the order, delete the payload after the specified retention period. Do not clean up unfill, partial fill, and unknown entries. The record that prevents re-execution of request IDs clearly indicates the retention window and aligns with the rejection of old acceptance requests.

### 4.4 Synchronous transaction boundary

`Db::update` is a short synchronization transaction that does not cross await or external calls. The commit and completion of SQLite's IC messages are separate. If the same IC message is trapped, the changes committed on SQL within the message will also be rolled back. The state of previously normal-terminated IC messages and external actions that have already been issued are not canceled by the callback trap.

Therefore, record dispatching before POST issuance and proceed to reconciliation even if state preservation in callback fails. We do not assume that a normal Result::Err automatically rolls back the write.

### 4.5 Prohibited items

- **Do not use SQLite's `random()` / `randomblob()` functions.** This VFS is deterministic and produces the same value for the same call. For cloids, tokens, etc., we use secure random numbers derived from the asynchronous `raw_rand` of the management canister. The nonce is assigned from the current time and a persistent counter, not from random numbers.
- Do not use WAL, `-wal`/`-shm`, mmap, or shared-memory methods (not supported).
- `Db::query` is a `query_only`/read-only connection. It does not write state within the query's closure. It does not assume that the query can be persisted.
- Do not move connections, statements, or transactions outside of the closure (`SQLITE_THREADSAFE=0`).
- Does not construct any arbitrary SQL from external input. Values are always bound and identifiers are not dynamically generated.
- Do not allow unlimited `LIKE '%...%'` and full table scans and unlimited `ORDER BY` in public queries. The list must be paginated using a cursor page with `LIMIT` and a definitive timer.

### 4.6 CAS and fencing

worker increments the persistent worker_epoch at the time of acquisition. All writes after every await are protected by a CAS with `WHERE action_id = ? AND worker_epoch = ? AND dispatch_state = ?`, and if no updates are made, the result is discarded. The lease expiration alone cannot prevent contention where the previous signing callback arrives late.

The recovery before the signature can update the generation. The recovery for dispatching/unknown is the resumption of the reconciliation worker, not the reacquisition of the sending rights. Cancellation and kill-switch invalidate the epoch of unsent actions. Since the side effects of sent actions cannot be invalidated, a separate cancel action and venuereconciliation are required.

### 4.7 Migration

- The `Migration` version will strictly increase. `Db::migrate` is called both with `fresh-install` and `post-upgrade` after `Db::init`.
- Each migration is one versioned step. Do not implement it as idempotent initialization using `IF NOT EXISTS`.
- Migration SQL is kept static. It is not assembled at runtime.
- Table reconstruction, backfill, and index creation are measured in PocketIC using **production scale data**. If it doesn't finish in one message, split it into an explicitly restartable application migration. Do not create SQLite transactions across messages.
- The upgrade test is performed along the path from “the Wasm that is actually deployed and the stable layout” to “the proposed Wasm”. It validates schema versions, representative data, consistency, and resource metadata.

### 4.8 Capacity and monitoring

- Monitor the logical DB size and the high water of the selected stable memory **separately**. Stable memory does not shrink.
- In v8 layout, normal commits stabilize `db_base_offset` and keep `page_table_bytes` at 0. If these two values continue to increase, they will be treated as a regression.
- `orphan_bytes_estimate` is an observation and not a proof of recovery possibility.
- Stable memory growth failures are treated as capacity incidents. Do not blindly retry without checking the required number of pages, maximum capacity, cycles, and operation size.
- `checksum` is the "last verified checksum" and is not a commit boundary. It can become `checksum_stale` with normal writes.
- The checksum recalculation is **split** into jobs limited to the controller and executed until the end using `refresh_checksum_chunk`. It does not scan large databases in a single public update.
- The endpoint for integrity checks and checksum maintenance is limited to controller or explicit administrator authorization.

### 4.9 Recovery prerequisites

DB is not just an HL index. The intent of received and unsubmitted, signatureed outbox, nonce, workergeneration, ownership support, risk reservation cannot be fully recovered from HL. Fix the version and lockfile and make the next one the production migration gate.

- Verify a logical backup/restore method that includes authorization, encryption, a point of integrity, and a size limit for confidential data. It is not called a live backup that supports raw stable-memory copies.
- Do not resume transmission just by restoring an old backup. Stop if you reconcile undecidedaction and cannot prove safety due to missing state.
- When the nonce is missing, do not select "sufficient future nonce" and do not restore it. There is a valid window, and there is also a risk of re-accepting an old signature. funds_vault will invalidate the old Agent and approve the new generation before re-reconciling the account. The missing funds outbox of the master signing cannot be resolved by changing the Agent, so transfer history and reservations will be reconciled separately to determine whether to suspend or unSuspend.
- Agent regeneration is based on the opaque account_id and generation saved, not the public user_id. The loss of corresponding data does not guarantee the automatic recovery of the key.
- Layout changes are validated as data migration and retain the old Canister and the original only.

## 5. Order pipeline

### 5.1 distinguish the action and the order status

The state of an action is defined as `queued → signing → signed → dispatching → reconciled / unknown`. Only those that can be guaranteed not to be sent can be set to `aborted`. `reconciled` represents the reconciliation of the results of each child operation and is not equivalent to completion of the fill operation.

The order side distinguishes between `pending / open / partially_filled / filled / cancelled / rejected / unknown` and stores the HL state and cumulative fill amount. It does not interpret the HTTP success of the entire batch as the success of all sub-orders. Since cancellation requests also have a conflict with fill, they are stored separately from cancelled ones.

### 5.2 idempotency

- Identify the received resend by using `UNIQUE(user_id, client_request_id)` and the fingerprint of the normalized text. If the same ID and the same content, the same result; if different content, reject.
- The nonce is permanently secured by the signer as `max(now_ms, last_nonce + 1)` and is used to verify the HL validity window. It is assigned to one action at a time, not to each child order.
- Signature retry is limited to the same action/digest that has not been sent. It will not automatically reorder with a new nonce or cloid after dispatching.
- cloid is a reconciliation key and is not a permanent exactly-once guarantee. It does not re-approve keys that have expired or lost validity because it depends on the record of the nonce and the Agent's lifespan.

### 5.3 Failure, deadline, cancellation

- Signature rejection is bounded backoff within the range of unsubmitted. Re-verify epoch, expiration date, and policy.
- All POST timeouts, response interpretation failure, callback traps, and upgrades in dispatching are reconciled as unknown outcomes.
- Just because `orderStatus` is not found does not prove that the request has not been executed. Considering the retention period and visibility delay, if the issue is unresolved, keep it as unknown and present it to the user.
- `expiresAfter` is the acceptance deadline for action, not the cancellation deadline for orders remaining on the board. It does not cause cancelled/expired status due to local deadline exceeding.
- Unsent cancellations can be completed with epoch invalidation and aborted. Orders that are possible to send should be issued with the HL cancel action, and the result and fill quantity should be reconciled.
- New kill-switch suspension, cancellation of unfilled orders via HL standard scheduleCancel, and position settlement are separate operations. It does not display that positions are automatically closed even with suspension or a dead-man's switch.
- If the unknown outcome cannot be resolved, do not automatically resend, keep the reservation and stop it on the safe side.

### 5.4 Trust in HTTPS outcall and reconciliation

The state change POST is selected as a non-replicated outcall and is tested for operation in the selected CDK/subnet. Even a single transmission cannot achieve atomic external effects. The response size limit is set considering the raw body and header rather than JSON after extraction. The cost is determined by the cost estimate and actual measurement of the adopted API, and the replicated formula is not applied directly to the non-replicated one.

Non-replicated read results are not independent consensus proofs. If you adopt risk limits and authoritative inputs for balance, determine a trust model that includes response tampering and stale data. Stop production operations during the required reconciliation strength is unverified. Do not delete order-specific results or reconciliation identifiers using transform. Do not log the text and signature.

### 5.5 batches and priority

Only orders with compatible settings for the same seat, signatureagent generation, network, vault, grouping, builder, and deadline policy are grouped within the number of orders and payload size limits. Do not mix orders from different users into one signature. Also set a limit on the waiting time for batch processing. Cancel All can also become multiple actions depending on the number of orders limit. Prioritize cancellation over new orders.

## 6. Client design

### 6.1 Only public market status is directly connected to Hyperliquid WS (D11)

The browser connects directly to `wss://api.hyperliquid.xyz/ws` (mainnet) / `wss://api.hyperliquid-testnet.xyz/ws` (testnet). Does not go through the canister.

| a use | Channel |
|---|---|
| Average price of all stocks | `allMids` |
| a slab | `l2Book` (5 stages with `nSigFigs`, `mantissa`, `fast`) |
| fill | `trades` |
| Candle | `candle`（1m〜1M） |
| Best gesture | `bbo` |
| Public stock context | `activeAssetCtx` |

The user's order, fill, account status, and fund history are obtained from the authentication and encrypted Canister API. Do not subscribe to user-related HL channels in the browser. Test that there is no transmission of trading account addresses by performing network inspection in the browser.

The reconciliation on the Canister side uses the trading account master address and cannot be distinguished from the Agent address. The initial personal data is polling, and no permanent WS relay server will be added.

### 6.2 Connection and rate limits

Limit on IP units of Hyperliquid.

| limitation | cost |
|---|---|
| WS connection number | 10 / IP |
| New WS connection | 30 / minutes |
| Number of subscribers | 1,000 |
| **Unique number of users of user-related subscriptions** | **10 / IP** |
| Sent message | 2,000 / minutes (total connections) |
| REST | 1,200 weight / minutes (`l2Book`, `allMids`, `clearinghouseState`, `orderStatus` are weight 2) |

The design consequences on the client's side.

- **Do not establish connections per tab.** Share one connection across multiple tabs (`BroadcastChannel` + leader election). 10 connections per IP can easily run out across multiple tabs.
- The connection will be severed if there are no messages from the server for 60 seconds. Send `{"method":"ping"}` periodically.
- When reconnecting, it detects a snapshot (`isSnapshot: true`) and replaces the state. It does not apply as a difference.
- The real data is updated with the revision and observation time of the Canisterreconciliation result. It distinguishes between the cache display and the latest HL state and does not determine the fill from the public market.
- Subscribe to the channel when you need it and unsubscribe when you leave the screen. 1,000 subscriptions can easily exceed all stocks.
- The history candle has a limit of 5,000. For screens that require long history, keep it yourself or use external sources.

### 6.3 pending display (D9 UX compensation)

Due to D9 and D10, there is a delay in seconds until the order reaches HL. The browser gets the status of confirmed from Canister. It displays the difference between receipt and HL acceptance.

1. User sends → The browser immediately displays the local `pending` line in the order list (using `client_request_id` as the key).
2. Receive `cloid` and `order_id` in the `submit_order` reception response (`queued`) and link them to the `pending` line.
3. Obtain the reconciliation result for HL acceptance by polling the encrypted personal data, and update the pending lines corresponding to the request ID/cloid. Do not display the acceptance completion as fill.
4. If the `submit_order` reception is rejected, delete the pending line and display the reason.
5. If it does not arrive within a certain time (e.g., 15 seconds), move to "Checking sending status" and provide a cancellation request link. However, separate the suspension before sending and cancellation on the HL, and do not display cancellation completed before confirmation.

Instead of "showing as sent", it should "show as received and in process". If you fake this, you will end up showing a filled order that actually hasn't been processed.

### 6.4 OSS stack and license

| Layer | adoption | license | Rationale |
|---|---|---|---|
| Order construction, HL type, WS/REST | `@nktkas/hyperliquid` | MIT | Includes signature, `approveAgent`, 31 subscriptions, and batch orders. TS |
| a chart | `lightweight-charts` | Apache-2.0 | Commercial closed allowed. Sufficient for the performance requirements of `Plan.md`. |
| UI foundation | `shadcn/ui` + Tailwind | MIT | General |
| a list | TanStack Table | MIT | List of positions and orders |
| Reference to the pre-implementation | `vipineth/hypeterminal` | MIT | Refer to the implementation of board, order ticket, and WS reliability. |
| Reference on the Rust side | `infinitefield/hypersdk` | MPL-2.0 | Closed only if MPL files are not modified. |

**Do not hire.**

| a target | Reason |
|---|---|
| TradingView Advanced Charts / Trading Platform | Not selected this time. Public services for enterprises and source confidentiality are subject to separate conditions. If you are selected, please check the public form, attribution, and license. |
| `kline-orderbook-chart` | Commercial proprietary. License fee required. |
| `suenot/profitmaker` | MIT + Commons Clause. Prohibit "Sell the Software" |
| GPL-3.0 / AGPL-3.0 projects (freqtrade, freqUI, OctoBot,nofx) | Copyleft. Especially AGPL clashes with network terms with host-type services. |
| Unlicensed repositories | All rights reserved. **The official `hyperliquid-dex/order_book_server` is also unlicensed** |
| `nomed/hyperliquid`（npm） | npm metadata claims to be MIT, but the repository's LICENSE is 404. Legally unclear. |

Charts will use Lightweight Charts. The old description that "Advanced Charts cannot be used in commercial closed systems" will be withdrawn. Based on the official comparison and terms of service (https://www.tradingview.com/free-charting-libraries/), check the usage and contract when needed.

### 6.5 Rust side

- The official `hyperliquid-rust-sdk` is MIT-licensed but has been stagnant since 2025-10-21. **Signature implementation is written in-house without relying on the official SDK**, and the official SDK is used only as a source for test vectors (`Plan.md` 8.8).
- `hypersdk` (MPL-2.0, active) has EIP-712 signature and batch orders, but agent approval support is not yet confirmed. Please refer to it for reference.

---

## 7. Agent approval flow

1. Authenticate the session in the EOA challenge of Plan 16.1. The funds_vault generates a random account ID and a masterpublic key and binds it to the user's identity ID. It does not allow the user to declare any HL account address.
2. Create a new agent generation with an authorized update and obtain and store the public key with a management call. The query will only return the stored owner's address.
3. funds_vault approvesAgent in trading account master. Reconciles registered trading_core caller, account, generation, and public key, and rejects approval requests for any Agent address.
4. Reconcile the HL approval status independently and make it active. Ignore callbacks that have changed generation or ownership during the request.
5. If you use the builder fee, you must separately confirm the approveBuilderFee and the limit with master signing. Agent approval does not constitute fee approval.

Explicitly specify the address, purpose, permissions, and expiration date in the UI. Reusing the account after the expiration or loss of validity is prohibited, and a new key is derived from the opaque account_id and generation. The derived results are cached for each generation. The 30-day validity and 27-day switching are verified on testnet. Requests to halt or lose validity are executed by Canister, and HL direct unbinding and direct withdrawal are not guaranteed by the user themselves.

## 8. Front-end distribution

The main distribution is to deploy TanStack Start + React to Cloudflare Workers + Static Assets. The public page is SSR, trading, funds and history are drawn by the client. The balance of the person in charge, the order text, the wallet signature, the account support table are not passed to SSR, server function, Workers log. Please refer to docs/implementation-status.md for the implementation status.

Workers does not act as an intermediary for the trading API. Funds, signatures, order status, and owner authorization are retained on ICP. D1/KV/R2/DO, Hono/Express, and custom WS relaying are not added. The current UI is a synthetic demo with no external communication, and it is not connected to the actual HL market data and ICP.

### 8.1 Reference values for old ICP distribution (not the currently selected architecture)

The delivery of static front-end is almost free in terms of cycles.

| an item | cost |
|---|---|
| query call | **Free** (single node, no consensus) |
| Storage | 127,000 cycles/GiB/sec (13 nodes) ≈ **$0.45/GiB/month**. 34 nodes are 332,153 ≒ $1.18/GiB/month. |
| Response byte | There are no billing items |
| ingress reception | 1,200,000 cycles/message + 2,000 cycles/byte (13 nodes) |

The above is an old reference calculation for static distribution only and is not a price guarantee at present. It does not include the cost of reconciliation and encryption polling of personal data, fund ledger, and signatures. The relay of public market conditions is omitted, but the total cost of the B will be re-measured in Phase 1.

### 8.2 Old ICP delivery restrictions memo (Not applicable to Workers)

| limitation | cost |
|---|---|
| ingress payload | 2 MiB |
| query response | 3 MiB |
| update response | 2 MiB |
| stable memory | 500 GiB / canister |
| wasm | 100 MiB |
| query execution thread | 2 / canister |
| update execution thread | 1 / canister |

The asset canister automatically chunks uploads exceeding 2 MiB. Responses exceeding 3 MiB are certified as `206 Partial Content` and are reconfigured by the gateway.

**Practical constraints are that there are two query execution threads per canister.** When the number of simultaneous connected users increases, this will become the bottleneck first. The RPS limit for boundary nodes (according to statements from DFINITY staff, approximately 1k rps/client; exceeding this results in a few minutes of ban) is not a problem with a polling rate of about 1Hz.

### 8.3 About the reliability of boundary nodes (Correction)

The following is a trust boundary memo regarding ICP delivery, not a verification of current Workers delivery. In the current configuration, Cloudflare and the delivery authority's JavaScript changes become the trust point. The 7-day guard period does not apply to UI delivery. Dependency fixed, delivery authority separation, and mandatory release reviews and production CSP/connect-src design are required. Only regional restrictions at the entry point cannot restrict direct Canister calls.

Correcting the previous explanation. "Since query responses are verified by certified data, boundary nodes cannot generate fake responses," is **incorrect on the part of the entity**.

- The gateway cannot generate a fake response and accept it on the verification side (it is chained to ICP route key, and a part of signature requires more than 2/3 of subnet nodes).
- However, **it is a gateway, not a browser, that verifies the browser path**. The official documentation states, "Because the browser cannot verify its own IC certificate, it delegates that verification to a gateway. The choice of which gateway is trusted is the decision of trust."
- The gateway can choose to **not verify**. `raw` hosts (`<canister-id>.raw.icp.net`) will discard the certificate.
- Only the client that can directly talk to **canister** (agent using `read_state`) without using an HTTP gateway can verify it.

Operational implications. If you want to verify certified responses yourself, the frontend does not leave the boundary node to be trusted; it verifies important values (balance, position, risk limit) through the agent before displaying them. Self-hosted gateways are also possible (`dfinity/ic-gateway`, Apache-2.0, actively maintained). Self-hosted gateways mean that you decide “which gateway to trust” yourself, and the gateway does not disappear from the path.

---

## 9. Verification plan

### 9.1 signature test vector (first one)

`hl-sign` will not proceed unless it satisfies the following conditions.

1. The digest and encoding of the same input as the official SDK are consistent. It separates the deterministic local vector using a fixed private key and the actual signature verification of tECDSA. tECDSA requires signature verification and address consistency, and does not require byte consistency of r/s.
2. The field order of action, the integer representation of msgpack, and the normalization of decimal are consistent.
3. The wrapping (equivalent to phantom agent) at agentsignature matches EIP-712 domain/type.
4. Can recover `v` from tECDSAsignature (since `v` is not included in the response of thresholdsignature, try candidates and choose the one that matches the public key).
5. Fix the above as a regression test and run it continuously.

Test vectors are fixed in the repository and record the SDK version of the generator.

### 9.2 PocketIC

- Four systems: fresh-install, update/query, rollback in case of failure, and upgrade.
- Each case of empty, representative, near-limit, incorrect input, trap, migration failure, and post-upgrade.
- Test that trading_core does not have a withdrawalsignature and fundskey, and that the withdrawalaction in funds_vault requires owner authorization, balance, destination, and uniqueness.
- `universe` Conformity test of the asset index that scans all assets. Includes discontinued stocks, unknown tickers, and refusals outside the allowlist.
- Verify the idempotency of the receive resend, automatic resend prohibition after dispatching, and non-reuse of Agent generation.
- Recovery by reconciliation from a state where POST response has been discarded.

### 9.2.1 Verification of privacy and funding rights

- List the pathways that can be directly traced from the connection wallet to the HL account using only public information such as transfer, approval, API, and cloid. In A, the result will record that the transfer link remains directly, ensuring that the achievement of D13 cannot be disputed.
- Confirm that no one other than the owner can obtain order, fill, cloid, and account support. Not only does it not disclose queries, but it also verifies access control to responses, logs, and history.
- Verify information transferred to HL via the WS per account and browser communication with Agent approval, and reconcile with the U25 acceptance range.
- Test fund layer owner authorization, backing up, double-entry, uncertain transfers, upgrades, and suspension/recovery. Do not use the "withdrawal code not present" test for agent-only purposes. A/B0/B1 evaluation for Plan 16.6 is also mandatory.

### 9.3 Values measured in Phase 1

| an item | Why is it necessary? |
|---|---|
| `sign_with_ecdsa` p50/p95 from re2t4 | Go/No-Go gate of 2.4 |
| Reception→HL reception confirmation p50/p95 | a ditto ～s |
| `sign_with_ecdsa` queue overflow rate | The validity of re-design |
| Success and time required for HTTPS outcall on re2t4 | Outcall operation on Confidential Subnet is an unanswered public point of discussion |
| Charging criteria for 7-node subnet | Whether re2t4 is charged based on the 13-node standard is not answered yet. |
| The valid window of nonce and the behavior when there is a collision | `Plan.md` U8 |
| The actual specifications of the Agent's expiration date | `Plan.md` U7 |
| Encryption polling of personal data and HL reconciliation budget | 6.1、Plan 16.5 |
| Upgrade and state recovery in Confidential Subnet | `Plan.md` prerequisites for 8.3.3 |

### 9.4 Fault injection

- Signature queue overflow, signature error, POST timeout, POST error response, `/info` mismatch.
- Inject a kill-switch and cancellation in the signature, a callback trap after POST, and an old callback after lease expiration for upgrades in queued/signing/signed/dispatching/unknown.
- Even if the deadline of unknown is exceeded, do not cancel it, cancel after partial filling, partial rejection in the batch, and verify the rejection of different contents with the same request ID.
- New order suspension during Hyperliquid API outage and successful cancellation in cancel-only mode.
- Stable memory growth failed, `ZeroExtentLimitExceeded`.
- Double ingress (concurrent transmission of the same `client_request_id`).

---

## 10. Task breakdown

### Implementation of the final specifications (Phase 0~1)

- Adopt the funding pathway, master key, authentication, and guard in Chapter 16 of the Plan. Implement the funding exchange and privacy evaluation before the order UI.
- Implement EOAauthentication, HPKE, double-entry ledger, funds outbox, reconciliationadapter, master/Agent permission separation.
- Test the reservation, deferral, SNS authorization, bypass prevention, and recovery from stop/unknown of immutable guard in the test environment.
- The following old work period is invalid. We will re-estimate after the actual measurement of B.

### Phase 1 (fund transfer, signature, privacy spike, the period will be re-quoted)

| # | a task | Completion conditions |
|---|---|---|
| 1-1 | Implementation of `hl-sign` (action construction, msgpack, EIP-712, v restoration) | Official SDK and test vector consistency |
| 1-2 | tECDSAsignature (from re2t4) | v can be restored and orders can pass on testnet |
| 1-3 | Latency measurement | The main items in the table of 9.3 are filled in. |
| 1-4 | Agent approval, expiration date, and cancellation | Testnet to verify and confirm the specifications |
| 1-5 | idempotency by cloid | Check the idempotency, nonce/Agent lifespan restrictions of request ID |
| 1-6 | Outcall, upgrade, and recovery on Confidential Subnet | Work |
| 1-7 | `ic-sqlite-vfs` 2.0.0 interoperability | update/query/upgrade/migration pass through PocketIC |
| 1-8 | Separation of market conditions WS and personal data | The browser does not send the trading account to HL and can obtain the identity status in encrypted form. |
| 1-9 | Common custody account, independent master account, USDC transfers | Reconciliation can be done from deposit to withdrawal without double counting. |
| 1-10 | Guard and fund recovery | You can refuse to defer the waiver and check the restrictions when canceling, withdrawing, or stopping. |
| 1-11 | A/B0/B1 correlation evaluation | You can record the success rate and failure conditions of Plan 16.6. If not met, redesign. |

**Go/No-Go**: signature, fund transfer, authorization, and state machine pass are the conditions for a single user testnet. We explicitly state that the performance standards of 2.4 and the privacy standards of Plan 16.6 are not met, and we will not proceed with the privacy product or the live version.

### Phase 2 (single-user testnet MVP, period will be re-estimated)

| # | a task |
|---|---|
| 2-1 | Skyscanner and Migration confirmation, PocketIC upgrade test |
| 2-2 | `submit_order` (receipt, verification, CAS, spawn) |
| 2-3 | Implementation of `process` and `sweep`, intrusion attacks |
| 2-4 | Market/Limit/Cancel/Cancel All/Close, SL/TP (confirmation of grouping) |
| 2-5 | Client: WS connection sharing, pending display, order list |
| 2-6 | Agent approval flow (extraction, approval, reconciliation, deadline display) |
| 2-7 | withdrawalowner authorization, fund ledger, duplicate withdrawal and test for refusal of misappropriation to different users |
| 2-8 | cancel-only mode, dead-man's switch, emergency full cancellation |

### Phase 3 (multi-user testnet closed beta, the period will be re-estimated)

| # | a task |
|---|---|
| 3-1 | User-specific derivation path, key derivation cache |
| 3-2 | Implementation of the encryption method for order contents (method determined in `Plan.md` U5) |
| 3-3 | Rate limits, position limits, ticker allowlist and liquidity criteria (`Plan.md` 6.3.1) |
| 3-4 | `universe` Conformity test for all items, positions guide for discontinued stocks |
| 3-5 | Verification of eligibility token |
| 3-6 | Implementation of the builder fee and cost estimation |
| 3-7 | Audit log inspection for the absence of plain text |
| 3-8 | Implementation and validation of the REST weight budget for reconciliation |

---

## 11. REST weight Budget (constraints that are easy to overlook)

Canister itself hits Hyperliquid's REST. The limit is **1,200 weight/minute per IP unit**, and `clearinghouseState`, `orderStatus`, `l2Book`, and `allMids` are weight 2.

Approximate.

| a use | weight | Limit on number of items/minutes |
|---|---|---|
| Reconciliation for 1 order (`orderStatus` 2 times) | 4 | 300 orders/minute |
| Status polling for all users (`clearinghouseState`) | 2/User | 600 users/minute |

In other words, **the design of polling all users uniformly at this frequency exceeds the budget**. Polling 1,000 users every 30 seconds results in 2,000 requests/minute × 2 = 4,000 weight/minute, which exceeds the maximum limit by three times.

Therefore, we combine event notifications with a limited periodic reconciliation for active accounts and open/unknown orders.

- Orders are reconciled immediately after dispatch, when unknown is resolved, and when tracking open/partial fill. It limits load with backoffs and shared budgets.
- Position re-synchronization is performed at the start of the session, order reconciliation, and with the limit timer of the active account. The browser's `notify_fill` is not implemented initially. Even when viewing the public market data WS, the user's fill cannot be confirmed.
- All transactions are scanned at a low frequency and in batches, but the operating account is not risk-evaluated as long as it remains in a state one day older. Browser notifications are limited to hints with authorization and rate restrictions, and reconciliation continues even if no notification is received.
- If the condition is old, stop placing orders that increase the risk. Count parallel orders within the service based on the reservation at the time of reception, and distinguish them from the strict upper limit guarantee of the entire account, including other agents and direct trading.

This budget depends on how the IP of the canister is counted. Since replicated outcalls are sent individually by each node, the IP may be split only by the number of nodes in the subnet. **Exact attribution is measured in Phase 1.** Until it is measured, it is conservatively designed to "share one budget".

---

## 12. Implementation judgment and waiting for actual measurement

| # | Decision/Remaining items | Next verification point |
|---|---|---|
| U16 | HPKE recruitment, Plan 16.5. Testing implementation library and key authentication | Selected, Phase 1 verification |
| U17 | Over 5 seconds is considered a general UX failure. Confidentiality is not automatically downgraded; it is improved by the testnet testnet entity. | Decided |
| U18 | positionTpsl and reduce-only in positions unit. Complex brackets are postponed. | Decided |
| U19 | Share the public market status WS on BroadcastChannel+leader election | Decided |
| U20 | IP assignment for REST weight (whether it is per subnet node or aggregated) | Phase 1 |
| U21 | =2.0.0/v8 fixed. Updates require a real data migration test and do not follow automatically. New Canisters do not simply migrate because they also change the keyID. | Policy decision, verification at each update |
| U22 | notify_fill is not adopted. Canister bounded periodic reconciliation + order/session event | Decided |

The communication by fund pathway, key, and account has been confirmed in Chapter 16 of the Plan. The numbers are not filled in until the actual values of external environments such as U20 are confirmed.

---

## 13. reference materials

- [Hyperliquid nonce and Agent expiration and reuse restrictions](https://hyperliquid.gitbook.io/hyperliquid-docs/for-developers/api/nonces-and-api-wallets)
- [Exchange endpoint: expiresAfter, cancellation, builder approval](https://hyperliquid.gitbook.io/hyperliquid-docs/for-developers/api/exchange-endpoint)

`Plan.md` in addition to Chapter 15.

- [IC HTTPS interface (5 endpoints. No WebSocket)](https://docs.internetcomputer.org/references/ic-interface-spec/https-interface/)
- [IC Edge Infrastructure (Browser → Gateway → Boundary Node → replica)](https://docs.internetcomputer.org/concepts/edge-infrastructure/)
- [How Static Sites Work ( "The decision of trust depends on which gateway you choose")](https://docs.internetcomputer.org/guides/frontends/static-site/how-it-works/)
- [Certification (raw Host discards certificate)](https://docs.internetcomputer.org/guides/frontends/certification/)
- [Canister migration (tECDSA key is retained with a full migration that preserves the canister ID)](https://docs.internetcomputer.org/guides/canister-management/canister-migration/)
- [IC Resource limits](https://docs.internetcomputer.org/references/resource-limits/)
- [Hyperliquid WebSocket](https://hyperliquid.gitbook.io/hyperliquid-docs/for-developers/api/websocket) / [Subscriptions](https://hyperliquid.gitbook.io/hyperliquid-docs/for-developers/api/websocket/subscriptions) / [Timeouts and heartbeats](https://hyperliquid.gitbook.io/hyperliquid-docs/for-developers/api/websocket/timeouts-and-heartbeats) / [Rate limits](https://hyperliquid.gitbook.io/hyperliquid-docs/for-developers/api/rate-limits-and-user-limits)
- [ic-sqlite-vfs API Stability Agreement](https://github.com/humandebri/ic-sqlite-vfs/blob/main/docs/API_STABILITY.md) / [Operations](https://github.com/humandebri/ic-sqlite-vfs/blob/main/docs/OPERATIONS.md)
- Investigation records: `research/icp-realtime-static-frontend.md`, `research/hyperliquid-oss-research.md`

### Items left as unverified

- The user-related WS subscription does not require signature (inference from schema. No official explicit statement)
- The 3.5 sig/s for `pzp6e` is the DFINITY announcement value as of 2026-02. The current value may fluctuate.
- Current number of nodes, operational status and billing criteria for Confidential Subnet (re2t4)
- Hyperliquid API Terms of Use and Trademark Policy (not found in the official document index)
- Scope of support for `approveAgent` and batch orders in the official SDK (Rust version is not confirmed)

---

## 14. implementation contract for fund layer, authentication, guard

### 14.1 persistent data of funds_vault

Each Canister has its own separate DB. Do not mix the MemoryId 0 of funds_vault with the order DB. Set the version/layout of VFS and apply the init/post_upgrade and synchronization transaction rules as the same as chapter 4. Do not assume WAL or the normal file system backup API. At present, Cargo.lock is not implemented, and the actual API of dependencies is reconciled with the fixed version source at the first compilation.

| Table group | Mandatory restrictions |
|---|---|
| identities / sessions / challenges | Consistent with EOA and random user_id. Challenge nonce is one-time, and Principal, origin, purpose, expiration, and revocation are verified. |
| custody_accounts | Purpose of reserve/trading, opaque account_id, derivation path, master address, network. Do not approve tradingAgent in reserve. |
| journals / postings | Debit and credit amounts per journal are equal, and assets and units match. Integer overflow is rejected. Duplicate journal entries from external events or requests are rejected with a strict constraint. |
| fund_requests / reservations | Identity, request ID, text hash, amount, destination of confirmation, EOA intentsignature, expiration date. Refusal to accept insufficient balance, double binding, or substitution for others. |
| fund_actions / master_nonces | Canonical action, digest, signature, wire payload, dispatch state, epoch, lease, reconciliation scheduled. Each master assigns a nonce via a synchronized commit. |
| external_events / reconciliation | External stable ID, network, account, recipient, asset, amount, time, type, proof reference. Missing or competing public APIs are retained as unknown. |
| key_registry / audit | HPKE key ID, expiration date, agent generation approval, limited reason code. Do not publish plain text intent or response tables to the public log. |

Make sure that the balance can be derived from the transaction records, and when updating the cash balance, update it with the same transaction as the transaction record. unallocatedasset and trading account equity are treated as separate accounts, and the cash of the common reserve is not duplicated allocation to multiple users.

### 14.2 Fund-state machines and external reconciliation

The fund request is `accepted → reserved → executing → settled` or `rejected/unknown`. Instead of treating multiple external transfers as a single atomic operation, allocation, recovery, and payment are separated into individual fund_actions. Each action uses the same `queued/signing/signed/dispatching/reconciled/unknown/aborted` as the order, and only sends external messages after dispatching is permanent.

- deposit is not counted in the hash displayed in the browser or the success message. In HL, the destination, authenticated transfer source, asset, amount, and stable event ID are verified. The identity is not estimated only from the amount and time of the new deposit.
- After withdrawal reservation, check the withdrawable amount of each user's account and recover it, and pay the amount to the person in charge from the reserve after the recovery is confirmed. Do not replace the loss of response in the middle with another transfer, but reconcile it. Even if there is funds in the common reserve, do not pay the unconfirmed recovery or undecidedPnL in advance.
- Adjust new orders and recovery with account-specific fund transfer lock and generation. core checks the authenticated allocation status from vault and does not count in transit margin as available. It does not unnecessarily interfere with cancellation or reduce-only.
- Select non-replicated POSTs, and external transmissions will be performed outside the DB's await. The reading of `/info` for capital and risk assessment will be treated as a replicated outcall under fee scheme v2. Response modifications by a single node will not be incorporated into balance accounting or the elimination of unknown entries. transform will not delete meaningful amounts, recipients, or IDs. Cases where discrepancies or the inability to obtain a stable ID will not be recorded.
- Even with replicated reading, the false HL and missing history cannot be cryptographically eliminated. We distinguish between the trust of HTTPS/API and the agreement within ICP. We will confirm in Phase 1 that each transfer has a unique definitive basis, and if it does not, we will not activate the implementation of real funds.
- Reversing the master nonce/outbox is especially dangerous. After recovery, start from a send stop and reconcile external history, all reservations, and balance. Do not automatically re-sign funds that cannot verify the expiration date of the old signature and the acceptance conditions of the HL.

### 14.3 API boundaries

The initial interface is limited to the following operations. The name is a design name and is not an implemented Candid.

- vault: `issue_challenge`、`open_session`、`revoke_session`、`get_funding_instructions`、`request_allocation`、`request_withdrawal`、`get_fund_status`、`request_agent_revocation`。
- core: `submit_order`, `cancel_order`, `cancel_all`, `get_account_snapshot`. close is treated as a reduce-only order.
- guard: `schedule_upgrade`, `cancel_upgrade`, `execute_upgrade`, `get_upgrade_status`. A new 7-day grace period will start when the reservation content or execution time is changed.

Even in queries, owner authorization is not omitted. When core uses the vault authentication results, it only accepts sessions with expiration dates and revocation generation distributed by registered vault callers. It does not trust the `caller` in the user's input. Sessions with unconfirmed expiration notifications are not used for fund requests or new risk acceptance. It verifies the caller ID, target account, purpose, generation, and request ID between canisters, and fences re-entry of callbacks and old responses.

HPKEkey is generated and updated according to the purpose, and does not disclose the encrypted private key to the query. The maximum payload before acceptance is 16 KiB, the maximum number of unsubmitted orders per account is 100, and the maximum number of fund transfers is 1. The upper limit is adjusted during the DoS test. New acceptance is not possible with expired keys, but the data required for accepted reconciliation is not discarded only by the deadline.

### 14.4 Change reservations and fault testing

guard does not sign customer funds. The expansion of permissions for the target Canister, authentication key, registered Canister, withdrawal rules, and dangerous policy changes will also be designed so that they cannot be circumvented by bypassing the delay through the management API. Even if an emergency stop is immediate, unblocking or relaxing restrictions will be carried out via the recorded SNS channel. Even if the caller entity itself is malicious due to an upgrade of the SNS, the 7-day delay on the guard side cannot be omitted, as it will be tested.

Failed exam requiring qualification:

- Fake EOA, different Principal, expired, challenge reuse, destination change, intent of different network.
- duplicate deposit events, inconsistent accounting, simultaneous withdrawal, orders during recovery, loss of response after transfer success.
- master/Agent misidentification, optional withdrawal signature requests from core, revocation generation callback.
- Non-SNS reservation to guard, early execution, WASM/index substitution, bypass by controller/reinstall/stop/delete.
- Receiving and reconciling requests for insufficient cycles, HL suspension, Canister upgrade, DB restoration, and HPKEkey updates.
- Plain text logs, queries from others, trading account transmission from the browser, confidential information infiltration into public proposals.

These are the test specifications that will be implemented in the future, and they have not been executed in this document revision.

## Change history

| Edition | Date | change |
|---|---|---|
| v0.1 | 2026-09-18 | First version. Determine D9~D12, consolidate the final architecture, latency design, repository configuration, only `ic-sqlite-vfs` persistence, order pipeline, client design, verification plan, task decomposition |
| v0.2 | 2026-09-18 | Compatible with Plan v0.4. Clearly define the implementation baseline for agent-only, add consideration boundaries, dependency implementation gates, and verification items to separate the confidential fund layer and HL accounts. Maintain D9, and explicitly specify the IP/account exposure and tECDSA/outcall protection scope for D11. |
| v0.3 | 2026-09-18 | Design and revise the separation of actions/orders, pre-transmission persistence, fencing, deadlines/cancellation/Agent generation, ownership verification, recovery, and risk reconciliation. Clarify the boundaries between fund privacy and signature pathways. |
| v0.4 | 2026-09-18 | Consolidated with Plan v0.8 B. Vault master/tradingAgent, EOA authentication, identity data via Canister, funds DB and state machines, immutable guard, and the decision on development and live gate. Agent-only withdrawal prohibited, direct exit, and withdrawal of old work periods. |
| v0.5 | 2026-09-18 | Added fixed dependencies, linting, type checking, and testing to the composite UI foundation for Workers main distribution and Start+React. Recorded 6 ADRs. Changed the reference to the old ICP distribution estimate and retracted TradingView's overly definitive conclusions. There is no actual ICP API, and the connection process is pending. |
