# Threat-to-test matrix

> Historical Phase 0 test plan. Execution status below records the dates stated here; later phase reports contain subsequent results.

- Basis: `Implementation.md` 9.2–9.4, 14.4, `Plan.md` 3.2, Chapter 16, Roadmap Chapters 4 and 6
- Status: Design contract. **Partially executed**. The executed tests should be labeled with the execution date and proof, and a list of executed and unexecuted tests should be placed in Section 11 (executed and unexecuted local versions of authentication, funds, outbox, and guard executed from 2026-09-19 to 21). Unexecuted tests should remain unexecuted.

## 1. How to read

- "Layers" are the execution methods for the test: `unit` (pure function/host), `PocketIC` (Canister integration), `testnet` (HL connection), `Playwright` (UI), `manual` (environment/operation), `review` (static code/configuration verification).
- "Evidence" is a record left at the time of passing the test. Protect evidence related to funds, signature, and authentication, and do not leave plain text intent, signature, or identity-to-account mapping.
- The Implementation Phase shows which Phase the test can be executed in. Phase 0 only fixes the contract.

## 2. Response to mandatory verification (Chapters 5 and 6 of the roadmap)

| Roadmap items | Corresponding test |
|---|---|
| Only record the deposit once, and do not include the undecided transfer in the final balance. | T-201, T-202, T-204 |
| Verify the person's signature, recipient, amount, deadline, and nonce for withdrawal | T-102, T-104, T-203 |
| Even if the response loss occurs after a transfer is successful, double payment will not be automatically resended. | T-205, T-206 |
| Cannot request master-key signing or optional withdrawal from trading_core | T-301, T-302 |
| Do not query per-user trading accounts directly from the browser to HL. | T-601, T-602 |
| Record delays in signature/reconciliation, outcall/signature/storage costs | T-801 (Measurement. Results are in `Implementation.md` 2.4) |
| Record whether Confidential Subnet is available and the unverified trust assumption | T-802 |

## 3. authentication/session

| ID | Threat and failure conditions | Measures (contract) | Layer | Expected results | Evidence | Phase |
|---|---|---|---|---|---|---|
| T-101 | Solve challenges with fake EOA | EIP-712 `typed_data` binds origin/network/canister/purpose/nonce/expiration | unit, PocketIC | Reject (`BadRequest.InvalidSignature`) | Test log | 1 |
| T-102 | Open a session with a different Principal | Bind the challenge to the principal and signing EOA | PocketIC | refusal | Test log | 1 |
| T-103 | Reusing expired challenge | 5-minute expiration, one-time nonce | unit, PocketIC | Reject (`ChallengeExpired`) | Test log | 1 |
| T-104 | Reuse challenge (double `open_session`) | Nonce uniqueness is guaranteed by the uniqueness constraint of the database. | PocketIC | Reject the second time (`ChallengeReused`) | Test log | 1 |
| T-105 | Requests from different origins and different networks | `aad` and challenge binding | unit, PocketIC | Reject (`OriginMismatch` / `NetworkMismatch`) | Test log | 1 |
| T-106 | Establish a session with the `purpose = withdrawal` challenge | Function separation | PocketIC | refusal | Test log | 1 |
| T-107 | The old session is still active after logging out | Update revocation generation with `revoke_session` | PocketIC | Expiration (`SessionRevoked`). Refused fund request and new risk acceptance. | Test log | 2 |
| T-108 | Call `trading_core` before the expiration notification | Do not accept unconfirmed sessions | PocketIC | Reject (`SessionIssuedByUnregisteredVault` etc.) | Test log | 2 |
| T-109 | Session key and personal cache browser persistence | Do not persist these data | Playwright, review | Not saved in the storage area | Screen recording | 2 |

Executed (2026-09-19~21, 7 items in `crates/pocket-ic-tests/tests/vault_auth.rs`): T-101 (rejects signatures from a different key), T-102 (binds the challenge to the caller when issued and rejects redemption with a different principal. `a_challenge_bound_to_a_principal_cannot_be_redeemed_by_another` and `a_challenge_principal_must_match_the_caller`), T-103 (rejects expired), T-104 (rejects reuse of a challenge), T-105 (rejects signatures from a different origin/network). T-106~T-109 are not executed yet.

Executed (2026-09-19, `crates/pocket-ic-tests/tests/vault_outbox.rs`): T-205 (does not automatically resend due to response loss after transfer success). T-206 (does not release or resend unknown information over time) is also executed (confirmed to retain after 10 minutes).

Executed (2026-09-19, `crates/pocket-ic-tests/tests/guard_upgrade.rs`): T-501 (non-SNS reservation rejection), T-502 (early execution rejection), T-503/T-504 (content mismatch rejection), T-506 (cancellation + new reservation with new allowance). Execution of matching reservations (`install_code`) is also verified with minimal Wasm.

Executed (Other, evidence table `docs/phase-1/evidence/P1-010.md`): T-201, T-202 through T-204, T-207, T-211, T-212, some parts of T-307, T-407 and T-705 (local parts of `vault_deposits.rs`, `vault_reconcile.rs`, `vault_funds.rs`, `vault_upgrade.rs`). T-212 is up to double rejection of the same `client_request_id`.

## 4. Funds, ledger, withdrawal

| ID | Threat and failure conditions | Countermeasure | Layer | Expected results | Evidence | Phase |
|---|---|---|---|---|---|---|
| T-201 | Duplicate recording of deposit events | Uniqueness constraints on external stable IDs | PocketIC | Rejected the second item. Balance unchanged. | Test log | 1 |
| T-202 | Imbalance (debit ≠ credit) | examination in journal units | unit | Reject without writing | Test log | 1 |
| T-203 | Change of recipient address/withdrawal intent to a different network | Intent-signature binding (amount, recipient, network, nonce, expiration date) | unit, PocketIC | Denied (`DestinationNotAllowed` / `NetworkMismatch`) | Test log | 1 |
| T-204 | Uncertain deposit and unrealized PnL are included in the withdrawable amount. | Account separation and credit rules | unit, PocketIC | Not included in `withdrawable` | Test log | 1 |
| T-205 | After the transfer is successful, the response is lost, and double payment is automatically resended. | `dispatching` only sends after persistence, `unknown` does not automatically resend | PocketIC | No duplicate transfer. Keep `unknown` | Test log | 1 |
| T-206 | Release `unknown` after the expiration date and resend | Do not release it over time | PocketIC | Reservation retention. No resend. | Test log | 1 |
| T-207 | Withdrawal in parallel to pay beyond the balance | Atomic reservation guarantee, account-level lock | PocketIC | The total does not exceed the balance | Test log | 1 |
| T-208 | Count the orders in recovery that can use the in transit margin. | Adjust with the funds transfer lock/generation | PocketIC | Refuse the increase of new risks | Test log | 3 |
| T-209 | Transfer losses to other people's balance | Account by user | PocketIC | Other users' balance remains unchanged | Test log | 3 |
| T-210 | Automatic re-signature after rolling back the master nonce and outbox | When restoring, from sending stop to reconciliation | PocketIC | Do not automatically re-signature | Test log | 3 |
| T-211 | Only add it to the total when the deposit is successfully displayed. | Verification of stable event ID is mandatory | PocketIC, testnet | Do not credit it | Test log | 1 |
| T-212 | Double ingress (concurrent transmission of the same `client_request_id`) | `UNIQUE(user_id, client_request_id)` | PocketIC | Only one case is accepted. The loser is `DuplicateIgnored`. | Test log | 2 |

## 5. Permission/signature separation

| ID | Threat and failure conditions | Countermeasure | Layer | Expected results | Evidence | Phase |
|---|---|---|---|---|---|---|
| T-301 | Request master-key signing from `trading_core` | Do not set any digest-signing API to `funds_vault` | Review, PocketIC | The corresponding API does not exist | Review records | 1 |
| T-302 | Request arbitrary withdrawal from `trading_core` | Verification of caller, purpose, amount, recipient, owner authorization, and balance | PocketIC | refusal | Test log | 1 |
| T-303 | Mistake in taking master key and agent key | Separate the entity that holds the key and limit the usage | Review, PocketIC | No reverse path | Review records | 1 |
| T-304 | Change the state using the callback of revocation generation | CAS for `worker_epoch` and generation | PocketIC | No updates. Discard the result | Test log | 2 |
| T-305 | Request for approval of arbitrary Agent address | Account, generation, and public key extraction reconciliation | PocketIC | refusal | Test log | 2 |
| T-306 | Arbitrary transfer/withdrawal destination change due to operational authority | Do not set up the corresponding API | review | There is no API | Review records | 1 |
| T-307 | Trust the `caller` value from user input | Authorization only with the caller of IC messages | unit, PocketIC | Cannot impersonate | Test log | 1 |

## 6. idempotency and order state

| ID | Threat and failure conditions | Countermeasure | Layer | Expected results | Evidence | Phase |
|---|---|---|---|---|---|---|
| T-401 | Same `request_id` and different content | Text fingerprint comparison | unit, PocketIC | Reject (`IdempotencyConflict`) | Test log | 2 |
| T-402 | Resend with the same `request_id` and the same text | Return the processed result | PocketIC | No double orders, same response | Test log | 2 |
| T-403 | Automatic re-order after `dispatching` | Ban automatic resend | PocketIC | New cloid and new nonce will not be issued. | Test log | 2 |
| T-404 | Treat order refusal within HTTP success as a success | Classify the status within the response by order | PocketIC | The inner error becomes `rejected` and the reservation is released. | `core_pipeline.rs` | 2 |
| T-405 | Display cancellation after partial fill as `cancelled` | Separation of `cancel_requested` and cumulative fill amount | unit, PocketIC | Maintain `partially_filled` | Test log | 2 |
| T-406 | `orderStatus` not present is determined to be not executed | Maintain `unknown` considering the retention period and visualization delay | PocketIC | Don't leave it as `rejected` | Test log | 2 |
| T-407 | upgrade in `dispatching` | Restart with reconciliation worker | PocketIC | Can be reconciled from `unknown` | Test log | 2 |
| T-408 | kill-switch and cancellation in the signature | Invalidation of the epoch of unsubmitted action | PocketIC | `aborted`. Sent is cancel action | Test log | 2 |
| T-409 | Pressing the send button repeatedly | idempotency key + local pending display | Playwright | No double orders | Screen recording | 2 |
| T-410 | Display that encourages re-ordering when the unknown outcome is detected. | Display terms and conditions | Playwright, review | Do not offer controls encouraging resubmission | Screen recording | 2 |

## 7. guard/change rights

| ID | Threat and failure conditions | Countermeasure | Layer | Expected results | Evidence | Phase |
|---|---|---|---|---|---|---|
| T-501 | Reservation from non-SNS principal | caller verification | PocketIC | refusal | Test log | 1 |
| T-502 | Execution within less than 7 days | `executable_at` inspection | PocketIC | Reject (`UpgradeTooEarly`) | Test log | 1 |
| T-503 | Changing the reservation WASM hash | `wasm_hash`reconciliation | PocketIC | Reject (`UpgradeContentMismatch`) | Test log | 1 |
| T-504 | Replacing the argument hash | `arg_hash`reconciliation | PocketIC | refusal | Test log | 1 |
| T-505 | Route bypass by adding controller, reinstalling, stopping, and deleting | Do not set up the relevant API. Empty guard's own controllers. | PocketIC, Manual | Cannot be bypassed | Test log | 1, 4 |
| T-506 | The delay is practically shortened due to changes in reservation details. | Changes are cancellation + new reservation (new 7 days) | PocketIC | A new allowance will be started | Test log | 1 |
| T-507 | Intentionally misleading calls after the SNS itself is upgraded | Do not omit the 7-day grace period on the guard side | PocketIC | Refuse execution without any delay | Test log | 4 |
| T-508 | Immediately execute the revocation of emergency suspension and the relaxation of restrictions. | The path on SNS recorded for the cancellation | PocketIC, Manual | Immediate relaxation is not possible | Test log | 4 |
| T-509 | Confidential information infiltration into public proposals and public logs | Only the target ID, hash, time, and status are published. | Review, manual | Customer information is not available | Review records | 4 |

## 8. Data protection and privacy

| ID | Threat and failure conditions | Countermeasure | Layer | Expected results | Evidence | Phase |
|---|---|---|---|---|---|---|
| T-601 | Check and subscribe to per-user trading account from the browser to HL | Direct connection only to public market conditions | Playwright (network recording), review | No sending of trading account address | Network record | 2 |
| T-602 | Subscription to user-oriented WS channels | Do not implement initially | review | No corresponding channel | Review records | 2 |
| T-603 | Output of log for plain text orders, signatures, and identity-to-account mappings | Log rules | unit, review | No plaintext is exposed | Inspection record | 2 |
| T-604 | Get balance and orders from other people's queries | Request owner authorization even in queries | PocketIC | refusal | Test log | 1 |
| T-605 | Reception and reconciliation while updating HPKE key | Verification of key ID and expiration date and handling of old keys | PocketIC | New registrations are only accepted with new keys. Reconciliation data will not be discarded only after the expiration date. | Test log | 1 |
| T-606 | Confirm with the public market status WS by guessing fill | The actual state is only the canister reconciliation value. | Playwright, review | Cannot be confirmed by inference | Screen recording | 2 |
| T-607 | Access control failure for signing payload and transmission payload | Controlled as confidential data | review | Cannot be read outside control | Review records | 2 |
| T-608 | JS modification by the distribution authority (not exempt from guard allowance) | Dependency fixed/distribution permission separation/CSP/release review | Manual, review | Explicitly stating that it is not performed | Operational records | 4 |

## 9. Environment, failure, recovery

| ID | Threat and failure conditions | Countermeasure | Layer | Expected results | Evidence | Phase |
|---|---|---|---|---|---|---|
| T-701 | The mock token passes the real-world settings | `environments.md` E-1 | Manual | refusal | Test records | 1 |
| T-702 | Signature queue overflow | Bounded backoff, do not make failures part of order failures | PocketIC | Return to retry | Test log | 1 |
| T-703 | New orders during HL suspension | cancel-only mode, new risk increase stop | PocketIC, testnet | Stop new orders; allow cancellation | Test log | 2 |
| T-704 | Insufficient cycles | Gradual withdrawal of acceptance (30-day target/7-day notice/3-day withdrawal) | PocketIC, Manual | Stop new deposits and new risk acceptance | Test records | 3 |
| T-705 | Recovery from an old DB backup | Stop sending → reconciliation → judgment of cancellation | PocketIC | Do not automatically restart | Test log | 3 |
| T-706 | Canister upgrade (each state) | `Db::init`→`migrate` with `init/post_upgrade`, epoch fencing | PocketIC | The state recovers and does not run in duplicate | Test log | 2 |
| T-707 | Stable memory growth failure - `ZeroExtentLimitExceeded` | Treat as a capacity incident | PocketIC | Error that can be recovered without panic | Test log | 3 |
| T-708 | Stale data (more than 10 seconds) | Stop new risk increase and show reason | unit, Playwright | Rejected. Show time | Test log | 2 |
| T-709 | An older response overwrites newer state after reconnection or updates arrive out of order. | Comparison of `revision` and `observed_at` | Playwright, unit | Do not overwrite | Screen recording | 3 |
| T-710 | Cut-off and snapshot of public market status WS | Replace with `isSnapshot` when reconnecting, send ping | Playwright, unit | Do not apply incorrectly as a difference | Screen recording | 2 |

## 10. Measurement (whether it passes or fails is Phase 1 Go/No-Go)

| ID | Item | output | Phase |
|---|---|---|---|
| T-801 | `sign_with_ecdsa` p50/p95, reception→HL reception p50/p95, queue overflow rate, outcall required, REST weight attribution, signature/outcall/storage cost | Measurement report | 1 |
| T-802 | Confidential Subnet availability, outcall, upgrade, recovery operation, unverified trust assumptions | Record (explicitly stating the assumption of reliability) | 1 |
| T-803 | A/B0/B1 correlation evaluation (`privacy-evaluation.md`) | Evaluation report | 1 |

## 11. List of executed and unexecuted

Executed (local, PocketIC. Evidence is in `docs/phase-1/evidence/`):

- T-101, T-102, T-103, T-104, T-105 (`vault_auth.rs` - 7 items)
- T-201 (Reject duplicate counting), T-211 (Verify stable event ID) (`vault_deposits.rs` - 2 entries, `vault_reconcile.rs` - 1 entry)
- T-202~T-204, T-207, T-307 (`vault_funds.rs` 6 items)
- T-205, T-206 (`vault_outbox.rs` and 9 entries. Includes some of T-212)
- Local parts of T-407 and T-705 (1 file: `vault_upgrade.rs`)
- T-501, T-502, T-503, T-504, T-506 (`guard_upgrade.rs` - 5 items)

Not performed: other than the above (T-106~T-109, T-208~T-210, T-301~T-306, T-401~T-406, T-408~T-410, T-505, T-507~T-509, T-601~T-608, T-701, T-702, T-703~T-710, T-801~T-803). Testnet, Playwright, Manual, and Review layers are not performed.

- Do not treat the success of UI demo, unit test, and build as test success in this table.
- Changes related to funds, signature, authentication, and guard cannot be completed by successful path alone.
