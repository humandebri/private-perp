# Manual retry and observation

Status: implemented for funds, orders, cancellation, and account monitoring. This document describes the integration of PR #2 with the OSS baseline. No public deployment or movement of real funds is included.

## Behavior

- Failed transfers, orders, and cancellations before dispatch leave automatic processing. Reservations remain held.
- Unknown outcomes after dispatch stop automatic observation too. The owner can request a result check; the application never offers retransmission of an uncertain POST.
- Healthy position and order monitoring continues. A failed monitoring unit stops until its owner resumes it, without stopping other accounts.
- Result persistence and recovery-fence release have separate permissions. Failure stops only the affected operation; successful continuations remain eligible.
- Ordinary UI refresh, deposit confirmation, and new order admission never grant permission to resume stopped work.

## Persistence and concurrency

Execution permissions are durable records independent of business state. Each records the operation ID, processing kind, owner, generation, and allowed flag. A scope-specific guard rejects concurrent execution. Permission is consumed before awaiting external work, so failure, traps, and upgrades cannot reuse it.

Manual requests authenticate the session and check ownership in the database. Only the displayed stopped generation can receive one permission. Requests for another owner, a stale generation, or active execution fail. Sending state, signatures, payloads, nonces, balances, reservations, and evidence are never rewound to make a retry possible.

A retry before dispatch is allowed only where the existing checks prove that no external POST occurred. Leverage preflight POSTs have their own state. An unknown outcome permits history and order-status observation only; existing evidence checks still govern settlement. If one observation cannot resolve the outcome, work stops again.

## Components and API

- **Database:** additional Vault/Core migrations and permission consumption, stopping, and candidate selection.
- **Vault:** distinct permissions for outbox dispatch, result persistence, reconciliation, allocation arrival, and fence release.
- **Core:** separate handling for failures before dispatch, unknown sends, cancellation, and monitoring.
- **Frontend:** an owner-only stopped-work panel with one-attempt retry/check and monitoring-resume controls. Normal refresh does not call those endpoints.

The encrypted private APIs expose `get_manual_work(SessionHandle)` and `resume_manual_work(SessionHandle, kind, work_id, generation)`. Lists are owner-filtered and bounded at 100 entries, excluding completed and actively running work. A lost transfer callback can advance `dispatching` to `unknown` using the existing CAS after manual permission. It never returns to `queued` or clears a signature or nonce.

## Verification requirements

1. Advancing time after external failure must not automatically repeat the POST or observation.
2. Only a failure before dispatch can permit a single retry.
3. An unknown send must never be reposted, including after manual observation.
4. Another user and a revoked or expired session cannot resume work.
5. Double clicks, active execution, and races across awaits cannot duplicate dispatch.
6. Upgrade preserves stopped state and never queues uncertain work again.
7. Other healthy accounts remain monitored.
8. Timers stop when only stopped work remains; ordinary refresh cannot wake that work.

## Unresolvable states and compatibility

Unknown leverage preflight and `recovery_ambiguous=1` reject ordinary owner resumption. The permission transaction checks the owner, generation, stopped state, and blocker together and returns an encrypted reason. The stopped row and generation remain visible. Preflight requires the existing authorized result-resolution API; ambiguous funds require evidence investigation. Neither path releases reservations or erases evidence merely to resume work.

A lost leverage callback can advance `dispatching` to `unknown` under the existing CAS during the owner's request, then return the blocker without granting execution. The snapshot/upgrade regression disables PocketIC install rate limiting only to exercise restoration; it is not a production execution-budget benchmark.

Unlike the earlier PR #2 implementation, this integration retains the main branch's legacy recovery migration lock, APIs, and journal decoding. Upgrade acknowledgment remains executable maintenance even when no user work remains, using a durable pending marker and a Core-only Vault wakeup. This maintenance cannot resume stopped user operations. Removing administrator assignment of unmatched deposits does not remove historical `DepositClaim` journal decoding or evidence-bound owner claims used by Spot deposits.

Manual permission does not guarantee that an outcome can be determined. Reservations and recovery fences remain held while evidence is inconclusive. Browser display refresh continues without authorizing stopped backend work.

## Historical local checks (2026-09-29)

The earlier branch recorded 13 order/cancel, 16 outbox, 15 recovery, and one Vault reconciliation tests; production/mock unified-Wasm and upgrade checks; Wasm Clippy, formatting, transaction-boundary and whitespace checks; and frontend typecheck, lint, 45 unit tests, build, and three browser tests. An authenticated canister browser flow was skipped because its environment was not configured. UI checks passed with disabled-control and unexecuted-event warnings. Subsequent checks recorded production/mock unified suites of five tests each and another 13 order tests after removal of legacy APIs. Those removals are **not** the compatibility policy of this integration. These historical results do not validate the final integrated revision.
