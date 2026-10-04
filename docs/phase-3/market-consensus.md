# Market admission and HTTP observations

This integration retains replicated `/info` reads from main and non-replicated `/exchange` POSTs. PR #2's owner-controlled recovery and on-demand workers do not require changing the trust model for custody or order evidence.

Market transforms validate an immutable context containing the configured market, thresholds, and request-start timestamp. Nodes agree on the admission verdict rather than volatile diagnostic price or volume samples.

## Validation

- Metadata checks unique assets, array alignment, delisting, configured asset index, and minimum rolling volume. Extra fractional volume precision is floored; money, prices, and sizes retain strict precision checks.
- Books check the coin, a timestamp within 60 seconds of request start, 1–20 levels on each side, positive prices and sizes, strict ordering, no crossed book, maximum spread, and minimum depth.
- HTTP failure, missing fields, and malformed JSON cannot allow admission. Response limits, the shared REST budget, and state validation around signing remain in force.

BTC/ETH observations commit together. A threshold change during acquisition, or replacement by another observation, rejects the stale commit. Explicit operations fetch observations when due and reuse observations younger than five minutes. The ten-minute market expiry and ten-second account freshness requirements remain.

## On-demand work

Periodic all-account deposit scans and periodic market polling are removed. An authenticated `vault_private_call(confirm_deposit)` reads one shared-reserve history page without returning other users' credited counts. Ownership and deduplication remain governed by venue evidence and ledger checks. Spot conversion continues to use the main branch's existing journal and fee handling.

Database transactions wake workers only when durable executable work remains. Serial five-second workers cover fund requests, send results, fence release, orders, and positions, and shut down when no executable work remains. Allocations check only relevant destination accounts while awaiting arrival. Unknown sends stop and wait for one owner-authorized observation rather than retransmission. See [manual retry](manual-retry-plan.md).

Initialization and upgrade inspect durable state before starting workers. Journal recovery and legacy migration locks remain. Because the serial CDK timer cannot clear itself while running, a following message rechecks outstanding work before clearing it.

The UI offers explicit deposit and trading refresh. New order admission fetches required observations before applying existing risk checks. Healthy monitoring continues when the browser closes. Failed work waits for manual permission. Storage and execution costs still apply.

## Verification and historical comparison

`scripts/test-single-canister.sh` checks production and mock Wasm contracts and market validation. Production fixtures cover authorization, deposit deduplication, idle reserve custody, allocation wakeup and arrival, monitoring lifecycle, and no retransmission after a lost POST response. Request assertions distinguish replicated reads from non-replicated writes.

The September 29 branch used non-replicated reads as well as writes. Its local ten-minute idle comparison recorded about 11.40B cycles for an older public Wasm versus 88,517,626 cycles, zero external calls, and 600 seconds for the changed fixture, including initial shutdown. These are historical fixture measurements, **not** measurements of this integrated revision or live-network bills. That branch also recorded 45 frontend unit tests and three browser tests, with one authenticated local-canister flow unexecuted and UI warnings remaining. A later validation record is available [here](predeploy-validation-2026-09-29.md).

No public deployment or real Hyperliquid fund round trip is established by these local checks. Keep those acceptance steps separate in the [acceptance record](testnet-acceptance.md).
