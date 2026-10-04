# Amounts, units, and uniqueness

- Basis: `Plan.md` 16.2, 16.5; `Implementation.md` 4.3, 4.5, 4.6, 5.2, 14.1
- Status: design contract; implementation starts in Phase 1.

## 1. Principles

1. Never store monetary amounts as floating point; use integer USDC base units.
2. Quantities/prices are normalized decimal strings, never converted to `f64`.
3. Unknown amounts do not increase withdrawable balances; pending deposits and unrealized PnL are not confirmed balances.
4. Round once at account crediting and record the rounded result.
5. Enforce uniqueness in DB constraints, not only application prechecks.

## 2. USDC representation

| Item | Value |
|---|---|
| Unit | 1e-6 USDC |
| Type | `nat64`, equivalent to 0–18,446,744,073,709.551615 USDC |
| Display | Six decimals; grouping only in presentation |
| Arithmetic | Integer addition/subtraction; explicit quotient/remainder for multiplication/division |
| Prohibited | f64/f32, SQLite REAL storage, exponent input |

- Accepted input: `^[0-9]{1,13}(\.[0-9]{1,6})?$`. Reject signs, exponents, leading `+`, and empty fractional parts.
- Normalize output by removing trailing zeros; use `0` if only `.` remains. No exponent notation.
- Per-request, per-user, and total custody caps are **undecided**. Fix them before real-fund admission as `environments.md` configuration.

## 3. Quantities and prices

| Item | Rule |
|---|---|
| Quantity | Normalized decimal string respecting asset szDecimals; reject excess precision without rounding |
| Price | Normalized decimal string; establish real HL precision/significant-digit rules through Phase 1 testnet measurements |
| Precision source | Real meta_cache retaining network/DEX, retrieval time, digest, and indices; digest/count alone cannot validate orders |
| Assets | Initial BTC/ETH perps allowlist; scan the full universe for indices; reject delisted/unknown assets |
| Leverage | Default 3×, UI maximum 5×; development restriction, not an investment recommendation |
| Slippage | Market uses IOC limits; default 50 bps (0.5%) |
| Type | Strings normalized at action construction; no f64 |

- Intermediate calculations also use integers/decimal strings and one rounding point.
- Reject conversion errors at admission with `BadRequest.PrecisionExceeded`, `QuantityOutOfRange`, or `PriceOutOfRange`.
- Signed msgpack amounts/quantities/prices are **strings**. Encode integers minimally; values outside int32 use the minimal signed-compatible representation (nonnegative `0xcf` uint64, negative `0xd3` int64), matching SDK `_l1.js` adjust() and @std/msgpack. Verified with SDK `cancel_large_oid` on 2026-09-19 (`docs/phase-1/README.md` section 6).
- Resolve network-specific indices from meta.universe: testnet BTC=3/ETH=4; mainnet BTC=0/ETH=1.

## 4. Integer ledger rules

Based on `Implementation.md` 14.1:

- Double-entry journals: equal debit/credit totals with matching assets/units.
- Reject overflow; check arithmetic in the same DB-write transaction.
- Reject duplicate request/event postings through unique constraints.
- Derive balances from journals; update any cache within the same transaction.
- Separate reserve-unallocated, withdrawal-reserved, in-transit, per-user trading equity, and margin holds. Never double-count shared and per-user account assets.
- Unrealized PnL is not confirmed withdrawable funds. Combined displays must avoid duplicates and state whether PnL is included.
- Account for fees under agreed rules; unsettled funds are not revenue. Do not divert customer assets to cycles/development/Treasury.
- Truncated residuals are not user credit. Record them as residuals without disappearing between accounts.

## 5. Rounding

| Stage | Rule |
|---|---|
| User credit: deposits/confirmed recovery | Round down, bounded by verified external amounts; never overcredit |
| User request validation | Reject amounts above balance/precision, rather than rounding down the request |
| HL reconciliation ingestion | Use returned values without custom conversion/apportionment |
| Display | Round presentation only; do not alter internal balances |

- FX/other-asset conversion is outside initial USDC-only scope. No implicit exchange rates.

## 6. Uniqueness and idempotency

Based on `Implementation.md` 5.2 and 14.1:

| Item | Rule |
|---|---|
| Admission | UNIQUE(user_id, client_request_id) plus normalized-body fingerprint; same ID/body returns same result, changed body yields IdempotencyConflict |
| Action nonce | Persist signer-specific max(now_ms, last_nonce + 1), one per action rather than child order; update last_nonce atomically with action creation |
| cloid | Unique fixed 16 bytes from a secure prefetched randomness pool; reject exhaustion with SigningQueueFull/BadRequest |
| External event | Persist stable ID, network, account, counterparty, asset, amount, time, kind, evidence; deduplicate by stable ID |
| Duplicate ingress | DB admits one; loser returns existing result through DuplicateIgnored |
| Agent generation | UNIQUE(account_id, generation) and UNIQUE(agent_address); never reuse revoked generations |

- cloid is a reconciliation key, not perpetual exactly-once assurance; nonce retention and agent lifetime matter. Never reapprove expired/revoked keys.
- Retry signing only for the same unsent action/digest. Never automatically reorder with a new nonce/cloid from dispatching onward.
- Specify request-ID replay-record retention windows consistently with old-request rejection.

## 7. Randomness

- Use secure async management-canister raw_rand for cloids/tokens/cryptographic IDs, supplying synchronous admission from prefetched pools.
- **Never use SQLite random()/randomblob()**: this VFS makes them deterministic, including identical values within one call.
- Allocate nonces from time/persistent counters rather than randomness.
- Randomize user/account IDs cryptographically; never embed EOAs/principals in public derivation paths or cloids.

## 8. Retention and deletion

| Data | Period |
|---|---|
| Terminal reconciled signed payloads | May delete after 24 hours |
| Detailed order/fill/fund history | May delete after 30 days |
| Unknown actions/reservations | Never delete |
| Balances/unreleased reservations | Never delete |
| Current auth/agent state | Never delete |
| Deduplication records: request mappings, stable event IDs, nonces | Do not delete; state retention windows |

- Check order termination and action reconciliation separately before cleanup. Keep open, partially filled, and unknown records.
- DB deletion does not guarantee deletion from old snapshots/replica storage. Production retention/legal grounds require legal approval.
- Development data is synthetic only.

## 9. Pending decisions

| Item | Timing |
|---|---|
| Real HL price precision/significant digits | Phase 1 testnet |
| Request/user/total custody caps | Before real-fund admission |
| Transfer fees/minimum amounts | Phase 1 section 16.2 measurements |
| Concrete retention windows/deletion jobs | Phase 2-1, Phase 3-7 |
| Production retention/legal grounds | After legal approval, Phase 4 |
