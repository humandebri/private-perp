# State transition: action, fund request, order

> Historical Phase 0 state-machine contract. Later phase reports and the checked-in code describe subsequent implementation changes.

- Basis: `Implementation.md` chapters 2.3, 4.4, 4.6, 5, 14.2, `Plan.md` chapters 3.2, 16.2
- Status: Design contract. Implementation after Phase 1

## 1. The relationship between the three state machines

| State machine | Unit | Purpose |
|---|---|---|
| action state | One signing/sending operation | Separate the responsibilities of signature, sending, and reconciliation. |
| fund request status | User's 1 request (allocation, recovery, disbursement) | Distinguish between reservation, in transit, and confirmation |
| Order lifecycle | 1 order | Maintain the state on HL and the cumulative fill amount |

One fund request or order can have multiple actions (signature and transmission units). One action can have multiple child orders (batches). **The completion of action reconciliation does not mean the completion of fill or withdrawal.**

The correct state name shall be an English identifier. The display text shall be fixed in an table in Section 8.

## 2. action state

```text
queued ──▶ signing ──▶ signed ──▶ dispatching ──▶ reconciled
   │           │           │            │
   └───────────┴───────────┘            └────────▶ unknown
              aborted (only if it can be guaranteed not to be sent)
```

| State | Meaning | DB invariant conditions |
|---|---|---|
| `queued` | Accepted/Not signed | `signature IS NULL`, payload not yet fixed, `worker_epoch` present |
| `signing` | Signature requested | `signature IS NULL`, `lease_until` setting increased, `worker_epoch` increased |
| `signed` | Signature completed | `signature` required, signing payload digest and wire payload saved, not sent |
| `dispatching` | You have obtained the sending rights | `signature` and the exact transmission payload are required. Saved by CAS before POST. |
| `reconciled` | The results of each child's operation are reconciled. | Keep the results and reconciliation reasons for each child order |
| `unknown` | External effects are unknown | Do not release the reservation. Keep it as a target for reconciliation worker. |
| `aborted` | Guarantee that it has not been sent, cancellation | Only actions that did not generate a signature or send |

Allowed transitions:

| From | To | Condition |
|---|---|---|
| `queued` | `signing` | Batch only compatible orders and permanently combine nonce and immutable signing payload into the same transaction. |
| `queued` | `aborted` | Epoch invalidation, kill-switch, acceptance expired, and risk reservation release were completed in the same transaction. |
| `signing` | `signed` | Received the signature response and re-verified epoch, state, cancellation requests, Agent generation, deadlines, kill-switch, policy freshness, and risk reservation. |
| `signing` | `queued` | Bounded backoff with signature rejection. Update epoch and retry retrieval. |
| `signing` | `aborted` | Confirmed that no signature was created. |
| `signed` | `dispatching` | After re-verification right before sending, save the `dispatching` and sending intent by CAS within the same IC message (without placing `await` in between) |
| `signed` | `aborted` | It was guaranteed that it would not be sent with cancellation, kill-switch, and expired. |
| `dispatching` | `reconciled` | Reconciled the HL results and the lifecycle of each child's orders. |
| `dispatching` | `unknown` | Timeout, response interpretation failure, callback trap, upgrade in dispatching, reconciliation failure |

Prohibited transitions and reasons:

- `dispatching` → `aborted`. The external effects that have been dispatched cannot be invalidated.
- `dispatching` and `unknown` → `queued`/`signing` (automatic re-signature). Prohibit automatic re-ordering with new nonce and cloid.
- `unknown` → `rejected`/`cancelled`. It cannot prove that it has not been executed only by the passage of time and the absence of `orderStatus`.
- `reconciled` → Any other state. If a new external effect is needed after reconciliation, create a new action.

epoch, lease, CAS (`Implementation.md` 4.6):

- Increase the permanent `worker_epoch` when getting an action.
- The order and cancellation worker lease is 30 seconds. Only the failed processing before POST can be reobtained after 5 seconds, and the nonce cannot be changed after re-obtaining.
- Writes after `await` are protected by `WHERE action_id = ? AND worker_epoch = ? AND dispatch_state = ?`, and if no updates are made, the result is discarded.
- The lease deadline alone cannot prevent the competition of delayed old signing callback.
- Recovery before signature is possible with generation updates. Recovery for `dispatching`/`unknown` is a restart of reconciliation worker and not a reacquisition of send permissions.

## 3. fund request status

```text
accepted ──▶ reserved ──▶ executing ──▶ settled
    │            │             │
    └────────────┴─────────────┴──▶ rejected (if you can confirm that it did not occur)
                                 └──▶ unknown (external effects are unknown)
```

| State | Meaning | restriction |
|---|---|---|
| `accepted` | Received the request and determined the idempotency key | Balance, recipient, and owner authorization can be unverified. The receipt does not indicate the start of fund transfer. |
| `reserved` | Constrained the balance and permanently established the fund transfer nonce and operation ID. | Refuse double confinement and substitution with others. Do not credit unconfirmed profits. |
| `executing` | The corresponding action is `signing`~`dispatching` | With account-unit fund transfer lock and generation, adjust the order of new orders and recovery. |
| `settled` | External events were reconciled, and the accounting has been finalized. | The debit and credit of the accounting entries are the same amount. Maintain the external event ID for the definitive basis. |
| `rejected` | It was confirmed that it was not executed. | Release is the same transaction. Do not set `rejected` due to response loss. |
| `unknown` | External effects are unknown | It will not be released or resend just by passing time. It will continue to be presented to the user. |

Initial funding path (`Plan.md` 16.2):

```text
Personal HL account →(personal signature)→ shared reserve account →(master signature)→ user-specific HL trading account
                                    ←(master signature)←
shared reserve account →(master signature)→ personal HL account
```

- Allocate, recover, and withdraw are separated into individual fund_actions. Do not treat multiple external transfers as a single atomic operation.
- After withdrawal reservation, check the withdrawable amount of the user's account and recover it, and pay the amount to the person from the reserve after the recovery is confirmed. Even if there is funds in the shared reserve, do not pay the unconfirmed recovery or unconfirmed PnL in advance.
- Deposit is not counted by the hash displayed in the browser or the success message. In HL, the destination, authenticated transfer source, asset, amount, and stable event ID are verified.
- unallocated balance, withdrawal reservation, in transit asset, and user-specific equity are treated separately. Do not double count shared custody assets and user-specific account assets.
- If there are ledger discrepancies or insufficient liquidity, the new allocation and risk increase will be stopped. Withdrawals will not be stopped uniformly until they can be securely backed and verified by owner authorization, but we will not pay for unknown funds.

## 4. Order lifecycle

Valid state: `pending` / `open` / `partially_filled` / `filled` / `cancelled` / `rejected` / `unknown`

| State | Meaning | Notes |
|---|---|---|
| `pending` | Local reception (`queued`~`dispatching`) | The browser displays it immediately with `client_request_id`. It may not be reached in HL. |
| `open` | HL accepted/not filled | Keep `hl_oid` |
| `partially_filled` | Partially filled | Maintain the cumulative fill amount. Do not overwrite the order quantity. |
| `filled` | Fully filled | Cumulative fill amount = Order quantity |
| `cancelled` | Cancellation completed | Maintain the reconciliation basis of cancellation action |
| `rejected` | HL rejected | Keep the reason code |
| `unknown` | unknown outcome | Do not encourage re-ordering |

- Do not interpret HTTP success as order success and classify the order status in the response individually. The current implementation is 1 action = 1 order, and unused `action_orders` will be removed in Migration.
- `cancel_requested` is kept separately from the state. The fill during cancellation requests is reflected as `partially_filled`/`filled` and not as `cancelled` (conflict between cancellation and fill).
- Cancel All will also become multiple actions depending on the number of items. We prioritize cancellation over new orders.
- `expires_after` is the acceptance and sending deadline for action. It is checked when receiving expired and immediately before POST, and if not sent, the reservation is released as `rejected`. It is not used for the cancellation deadline of orders left on the board.
- Do not purge open, partially filled, or `unknown` orders. The payload and signature of resolved terminal orders are eligible for deletion after 24 hours, and fill details after 30 days, up to 100 items per sweep.

## 5. reconciliation rules

| Event | handling |
|---|---|
| POST TIMEOUT | `unknown`. reconciliation worker checks HL history |
| The response has arrived but cannot be interpreted | `unknown`. Save the raw response as a reconciliation reference |
| callback trap | `unknown`. `dispatching` is already persisted before POST dispatch, so we proceed to reconciliation. |
| upgrade in `dispatching` | `unknown`. Resume from reconciliation after upgrade. |
| `orderStatus` cannot be found | It is not a proof of unexecution. Considering the retention period and visibility delay, if it is unresolved, keep it as `unknown`. |
| Deposit that cannot obtain an stable event ID | Do not credit it |
| Non-replicated reading results | The response of a single node alone does not constitute independent proof of fund entry or resolution of `unknown`. We measure the completeness and reliability conditions of the HL event identifier, account, amount, and history, and separately approve the conditions for finalization in the live event. |
| It was found that cancellation was already made in reconciliation. | Transition to `cancelled` and leave the reconciliation basis for cancellation action |
| Partial fill identified in reconciliation | `partially_filled`. Risk reservation consumes only fill amount. |

- The sweep is initiated by the global timer (with a 5-second interval. It is rearmed using `init` or `post_upgrade`). It limits the number of items, cycles, and API budget. Failures are recorded in the Canister log. The state is persistent and does not rely on the correctness of the timer and spawn (it is restarted manually with a `sweep` when stopped).
- If the unknown outcome cannot be resolved, do not automatically resend, keep the reservation and stop it on the safe side.
- During a failure, prioritize the cessation of new risk increase, the continuation of reconciliation, and the possible cancellation, reduce-only, and confirmed withdrawal.

### 5.1 Separation of stop operation

| operation | Effect | Things that have no effect |
|---|---|---|
| kill-switch (new stop) | Stop the increase in new risks | Automatic settlement of positions, cancellation of existing trading exchange orders |
| Cancellation of unfilled orders by HL standard `scheduleCancel` | Cancel the order remaining on the board | Protection SL/TP may be deleted (explicitly shown in UI) |
| Closing positions | Issue a reduce-only order | Do not automatically market-close positions. |

It does not display that positions are automatically resolved even with emergency stop or dead-man's switch. The dead-man's switch is set to OFF by default.

## 6. fencing, re-entry, old callback

- All writes after `await` are protected by the CAS of `worker_epoch`.
- In callback, epoch, state, cancellation request, Agent generation, validity period, kill-switch, policy freshness, metadata, and risk reservation are re-verified. The results of expired workers are discarded.
- In Canister inter-calling, the caller ID, target account, purpose, generation, and request ID are verified.
- Do not use sessions with unconfirmed revocation propagation for fund requests or new risk acceptance.
- Double ingress (concurrent transmission of the same `client_request_id`) only confirms one request by `UNIQUE(user_id, client_request_id)`.

## 7. restoration

- It will not restart the transmission just by restoring the old backup. Start from transmission suspension and reconcile external history, all reservations, and balance.
- Reversing the master nonce/outbox is especially dangerous. Do not automatically re-sign fund actions if the validity of their previous signatures and HL acceptance conditions cannot be established. A missing master-signing outbox cannot be resolved by changing the Agent.
- Do not restore by selecting "sufficient future nonce" when nonce is missing. `funds_vault` will invalidate the old Agent and re-reconcile the account after approving the new generation.
- Agent regeneration is based on the stored opaque `account_id` and `generation` rather than the public `user_id`. The loss of corresponding data does not guarantee the automatic recovery of the key.
- Stop if you cannot prove safety due to a missing condition.

## 8. Corresponding display text (standard → UI)

Based on sections 5 of `ui-spec.md` and 6.3 of `Implementation.md`.

| Canonical state | UI display | Current demo (`frontend/src/domain/demo.ts`) |
|---|---|---|
| (Immediately after local transmission) | Processing confirmation | unimplemented (replaced with `queued`) |
| `pending` (action `queued`) | Received and in preparation for sending | `queued` ( "Received and in preparation for sending") |
| `pending` (action `signed`/`dispatching`) | Sent/Under review | Unimplemented |
| `open` | HL acceptance | `open` (HL acceptance (simulation)) |
| `partially_filled` | Partial fill | `partial` (**Alternative name. Move to the proper name in Phase 2**) |
| `filled` | fill | `filled` |
| `cancelled` (in `cancel_requested`) | Cancellation confirmation in progress | Unimplemented |
| `cancelled` | Cancellation completed | `cancelled` |
| `rejected` | refusal | `rejected` |
| `unknown` | unknown outcome (do not resend) | `unknown` |

The current demo does not distinguish between `pending` and local reception confirmation, and does not have cancellation confirmation. This difference will be resolved in Phase 2 (no code changes in Phase 0).

## 9. Undecided matters

| Item | Fixed period |
|---|---|
| Batching compatibility conditions and waiting time limit | Phase 1 (1-1–1-5) |
| Reconciliation interval, backoff, and shared budget allocation | Phase 1 (Weight budget real-test in Chapter 11) |
| Operational procedures when `unknown` cannot be resolved | Phase 3 (at the same time as the recovery exam) |
| Specific value of the retention period | Phase 2-1 (when the schema is confirmed) |
