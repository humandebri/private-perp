# Phase 3 mixed load: PocketIC and mock HL

On 2026-09-25, reran 20 and 100 users with fixed seed `20260924`. Each user completed signed eligibility registration, a synthetic deposit, allocation, agent approval, an ETH order, cancellation, and recovery. Orders/cancellations used core's real dispatch pipeline; allocation/recovery used vault's real outbox. Only HL REST responses were mocked. User funds, accounts, and request IDs were separate. Checks covered all POST counts, post-recovery balances/fences, and the absence of builder-fee fields. The run includes normal V2 custody-account creation events and vault/core V2 send-result events. The tables reflect the rerun with per-business-stage cycles measurements.

| Users | Allocation/order/cancel/recovery POSTs | Total REST weight | Budget wait, simulated IC time | Order admission p95, host time | Per-user processing p95, host time | ETH observation age | Failures |
|---:|---:|---:|---:|---:|---:|---:|---:|
| 20 | 20 each | 6,840 | 366s | 120ms | 1,943ms | 61s | 0 |
| 100 | 100 each | 61,894 | 3,416s | 133ms | 1,670ms | 61s | 0 |

| Users | Vault cycles | Core cycles | Policy cycles | Vault journal cycles | Core journal cycles |
|---:|---:|---:|---:|---:|---:|
| 20 | 625,132,242,995 | 630,260,292,004 | 3,794,483,371 | 4,473,515,931 | 3,546,722,837 |
| 100 | 3,129,985,960,306 | 3,163,271,407,877 | 20,390,986,618 | 23,026,531,288 | 18,110,716,209 |

Read balances for all 5 canisters before and after each stage and log inclusive deltas as `phase_cycles`. The table below shows the two main fund/trading canisters in billions of cycles. Log arrays use the table's stage order for rows and vault, core, policy, vault journal, and core journal for columns.

| Stage | 20 users: vault | 20 users: core | 100 users: vault | 100 users: core |
|---|---:|---:|---:|---:|
| Login/deposit preparation | 8.2 | 0.0 | 41.9 | 0.0 |
| Allocation | 204.7 | 0.2 | 1,024.4 | 1.1 |
| Agent/account observation | 203.9 | 1.9 | 1,020.1 | 9.4 |
| Order | 1.8 | 415.9 | 9.4 | 2,085.5 |
| Cancellation | 0.3 | 206.9 | 1.3 | 1,039.9 |
| Recovery | 206.3 | 5.4 | 1,032.9 | 27.3 |

The shared REST budget is 1200 weight/minute with an exit reserve of 300. Tests advance the IC clock by 61 seconds whenever weight exceeds 900. The 100-user wait of 3,416 seconds is **not a prediction of real user wait time**. Host p95 excludes simulated clock advances. Total weight is high because sweeps also reconcile other accounts and reserve conservative maximum weight for `userFills`. Stage deltas include concurrent timers; signing, HTTPS outcalls, and storage costs are not separated. Real HL limits, real-time concurrent admission, and failure rates under faults remain unverified. Safety-test success is not a performance pass.

## Remeasurement after fewer agent public-key calls and non-replicated `/info`

Reran the same seed and test-venue tests on 2026-09-25. The `ic-cdk-management-canister` 0.2.0 HTTP builder already used fee model v2, and `/exchange` was already non-replicated. This change removed per-agent-signature `ecdsa_public_key` calls and made core/vault `/info` non-replicated. Values below compare **aggregate balance deltas across 5 canisters**, not the fee of one management-canister call.

| Users | Earlier cycles | Updated cycles | Reduction | Reduction rate | Core-only reduction |
|---:|---:|---:|---:|---:|---:|
| 20 | 1,267,207,257,138 | 1,265,138,673,035 | 2,068,584,103 | 0.163% | 2,019,095,652 |
| 100 | 6,354,785,602,298 | 6,343,243,273,806 | 11,542,328,492 | 0.182% | 11,892,209,229 |

Updated REST weight was 6,840 for 20 users (unchanged) and 60,166 for 100 users (previously 61,894). Both had 0 failures. The 100-user observation age rose from 61 to 244 seconds: within the 10-minute stop threshold, but less fresh. Timer and account-sweep counts vary, so the full cycles delta cannot be attributed solely to fewer public-key calls or non-replicated reads. Real HL cycles costs, read trust assumptions, and freshness remain unverified.

## Remeasurement after fixture correction on 2026-09-28

Retain earlier tables as historical measurements. The updated fixture records trading-account receipt as a separate event after allocation dispatch. Previously, it recorded only a direct deposit before sending and requested recovery while allocation remained unsettled, producing `ReservationConflict` under current balance-observation safety checks. Assert a trading balance of 10,000 USDC after receipt and 9,999 USDC after recovering 1 USDC.

| Users | Allocation/order/cancel/recovery POSTs | Total REST weight | Budget wait, simulated IC time | Order admission p95, host time | Per-user processing p95, host time | ETH observation age | Failures |
|---:|---:|---:|---:|---:|---:|---:|---:|
| 20 | 20 each | 6,916 | 366s | 91ms | 1,284ms | 61s | 0 |
| 100 | 100 each | 64,092 | 3,599s | 111ms | 1,474ms | 244s | 0 |

These are five-canister PocketIC/mock-HL measurements, not unified-canister concurrent-load or real-HL performance results. Different fixtures prevent using the old/new tables to calculate an improvement rate.
