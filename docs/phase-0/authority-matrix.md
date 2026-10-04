# Authority matrix: canister responsibilities, permitted callers, and signing actions

- Basis: `Plan.md` 3.1–3.3, 8.1, 16.1, 16.3; `Implementation.md` 1.2, 3.2, 14.1, 14.3
- Status: design contract; implementation and live verification are incomplete.

## 1. Reading this matrix

- Actors: user (authenticated EOA/browser), funds_vault, trading_core, policy_registry, control_guard, SNS governance, and operator. Operators are separate actors without fund authority.
- Signing actions are an exhaustive allowlist for each canister's keys. Do not implement requests outside it.
- This covers ordinary code authority. If SNS can upgrade arbitrarily, approved upgrades can still leak funds or information (`Plan.md` end of 3.1).

## 2. Canister responsibilities

| Canister | Responsibilities | Excluded authority/data |
|---|---|---|
| `funds_vault` | EOA authentication/sessions; shared-reserve and per-user trading master keys; double-entry ledger; fund requests/reservations/outbox; agent approval/revocation; deposit/withdrawal/allocation/recovery reconciliation | Order execution, plaintext user orders, arbitrary-digest signing API |
| `trading_core` | Order/cancellation/closing authorization and state machines; per-account/per-generation agent keys; signed action construction/dispatch; HL reconciliation; snapshots | Master keys, withdrawal signing, fund-ledger writes |
| `policy_registry` | Read country policy, terms versions, verification keys, emergency stop, allowlist | Fund/order execution; reads fail closed |
| `control_guard` | SNS scheduling, seven-day delay, matching-content upgrades | Customer-fund signing, customer-data storage, arbitrary management calls |
| `frontend` (Workers) | Public SSR, trading/fund/history delivery, direct public HL market data | Plaintext orders, personal balances, wallet signatures, fund DB, order API |

## 3. Ordinary-code authority

| Operation | User | trading_core / Agent | funds_vault | Operator / SNS |
|---|---|---|---|---|
| Orders/cancellation/closing | Authorize/request | Validate/execute | Outside scope | No discretionary trading |
| Deposit credit/allocation to HL | Authorize deposit/allocation | No withdrawal authority | Confirm deposits/manage balances/allocation | No arbitrary movement |
| Withdrawal | Authorize destination/amount | Prohibited | Validate balances/holds and execute | No DAO vote for ordinary withdrawals |
| Third-party transfer | Only withdraw own balance | Prohibited | Requires valid authorization | No diversion to DAO budget |
| Agent approval/revocation | Request stop/revocation | Stop new signatures | Execute/reconcile with master signature | Users cannot revoke directly on HL |
| Stop new orders/lower risk limits | Stop own trading | Enforce limits | Limit new allocation | Limited operator stop-only authority |
| Change destination/ownership | Owner authorization required | Prohibited | Evidence-backed transitions only | No arbitrary rewrite API |
| Change Wasm/controllers | Verify public information/exit | Prohibited | Prohibited | SNS + guard; seven-day upgrade delay; controller changes prohibited |

## 4. Permitted callers, keys, signing actions, and queries

| Canister | Permitted callers | Keys | Signing actions | Queries |
|---|---|---|---|---|
| `funds_vault` | Authenticated users (updates), registered trading_core (limited methods), control_guard (upgrade executor) | Shared-reserve/per-user trading master keys (tECDSA) | HL usdSend and fund transfers, agent approval/revocation, nonce-bearing fund actions | Owner's fund state only |
| `trading_core` | Authenticated users with vault sessions; funds_vault for allocation-state queries | Per-account/per-generation agents (tECDSA) | Orders/cancellation/modification/SL/TP trading actions | Owner's orders/positions/snapshot only |
| `policy_registry` | Fund/order canisters; control_guard for updates | None | None | Eligibility, terms, allowlist, stop state with limited public scope |
| `control_guard` | SNS governance for scheduling/cancellation; anyone may trigger reserved-content execution | None | None | Target ID, Wasm/argument hashes, execution time, state |
| `frontend` | Public | None | None; browser signs with user EOA and sends personal data through HPKE | Public SSR only |

Notes:

- Do not add trading_core→funds_vault withdrawal or arbitrary-digest signing. Calls are restricted to allocation-state checks.
- control_guard holds no signing keys or customer data.
- Failed policy reads stop new admission/risk increases (fail closed).

## 5. Design invariants

Make `Plan.md` 3.2 testable:

1. Separate fund/trading authority. Core has only trading agents; vault validates purpose, amount, destination, owner authorization, and balance, not merely caller.
2. Separate customer/operator funds. No diversion to cycles, development, or Treasury; no revenue recognition of unsettled funds.
3. Reconcile claims and backing. No duplicate credits/withdrawals; distinguish available, allocated, margin-held, withdrawal-reserved, and in-transit assets. Unknown amounts do not increase withdrawable funds.
4. Bind withdrawals to owner-authorized amount, destination, asset, network, nonce, and expiry. No DAO vote for ordinary withdrawals.
5. Reconcile uncertain external effects. Persist reservation/operation ID before send; lost replies do not imply no transfer.
6. Audit keys and upgrade authority. tECDSA keys bind to canister ID, derivation path, and key ID; malicious replacement code in the same canister can also sign.
7. Separate trading stop from withdrawal stop; do not promise unverified self-recovery.
8. Agent compromise can misuse trading margin; fund-layer or upgrade-authority compromise affects all custody assets. TEE does not prevent malicious authorized code; SNS does not prevent implementation bugs.

## 6. Caller verification and session propagation

Based on `Implementation.md` 14.3:

- Enforce owner authorization on queries, responses, logs, and history.
- Core accepts only vault-issued sessions with expiry/revocation generation; do not trust user-provided caller fields.
- No fund requests or new-risk admission if session revocation propagation is unconfirmed.
- Inter-canister calls validate caller ID, account, purpose, generation, and request ID; fence reentrant callbacks and stale replies (`state-machines.md` section 6).
- Never expose HPKE private keys through public queries; generate/rotate purpose-specific keys (`api-contract.md` section 5).

## 7. Authority denied to operators

Retain `Plan.md` 3.3 prohibitions:

- Arbitrary customer withdrawals, unauthorized destination changes, or account transfers.
- Full private-key extraction or fund-key signing of arbitrary messages.
- Customer-fund diversion to DAO operating budgets.
- Orders above user-specified limits.
- Arbitrary obstruction of valid stop/exit requests.
- Reading plaintext orders.

## 8. Pending Phase 1 decisions

- Public policy query scope and eligibility granularity.
- Vault/core session transport, authenticated response format, and revocation interval.
- Guard SNS generic-function invocation format and public execution-trigger scope.
- Developer-controller scope in development; production removal requires separate Phase 4 approval.
