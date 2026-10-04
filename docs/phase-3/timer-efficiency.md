# Timer efficiency investigation (2026-09-29)

This investigation covered the then-undeployed unified canister. Its original branch used non-replicated HTTP v2 throughout. The current integration retains replicated reads; the worker lifecycle changes below do not depend on the earlier transport choice.

## Implemented change

When a Vault/Core timer is stopped, a post-transaction callback checks for outstanding executable work before starting it. Cycle sampling and session or HPKE writes alone do not create empty timers. Database read failures conservatively start a worker to avoid abandoning work. Existing running timers and the final recheck before shutdown remain.

The callback performs reads only, after releasing the transaction connection. Writing inside that callback would recurse and is prohibited. The wakeup refinement itself changes neither the public API nor the schema; manual permissions have their own migrations.

## Investigated opportunities

Failure and unknown-outcome automatic retries have since been replaced by [manual permission](manual-retry-plan.md). The deadline/backoff proposals below were not adopted for stopped work. UI polling reduction, duplicate-read avoidance, pruning frequency, and shared cycle observation remain proposals.

| Priority | Evidence and behavior | Proposal | Constraint |
| --- | --- | --- | --- |
| High | `local-session.tsx` and `gateway.ts` fetch 13 categories every two seconds while visible; cycle sampling and encrypted list reads use update calls | Separate active order/fund data from lower-frequency cycles/configuration; refresh immediately after an operation | Do not mix old results across logout/login; retain hidden-page suppression |
| High | `pipeline::reconcile_account` may fetch `clearinghouseState` immediately after manual refresh | Check account observation time and active execution before duplicate reads | Keep final post-send observation, recovery checks, and ten-second freshness |
| High | `actions::recovery_checks` stores next-check time, but outstanding work wakes a five-second worker | Distinguish executable and future work and schedule the earliest deadline | New work advances scheduling; handle lease expiry, traps, and upgrade |
| Medium | Unknown-order status was formerly checked every sweep | Durable observation backoff | Never resend or complete an uncertain order; keep monitoring separate. Superseded by manual permission for stopped work |
| Medium | `sweep_once` prunes terminal history every pass | Lower-frequency, bounded pruning based on accumulated data | Preserve payload cleanup and old-fill deletion without permanent backlog |
| Medium | Vault/Core record the same canister cycle balance in separate databases | Share and throttle observations | Keep role-specific budgets and pre-send balance checks |

## Shared timer design

Combining two five-second timers into one sequential await loop can delay orders and cancellations behind fund HTTP calls. A shared design should centralize scheduling parameters and deadlines while keeping Vault/Core execution independent.

Defining `next_due` for executable work can let scheduling sleep until the earliest deadline. Reducing timer count alone does not reduce HTTP traffic. No single dispatcher that waits for all external calls has been introduced.

## Required continuations

Uncertain-send observation, final account/fill checks after order termination, position monitoring, ledger recording of transfer results, and fence release remain necessary. Healthy work must finish even after the UI closes; uncertain sends must never be retransmitted.
