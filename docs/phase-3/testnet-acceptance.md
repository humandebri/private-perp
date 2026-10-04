# Testnet acceptance status

Checked on 2026-09-30. Status: **not accepted**. Deployed the corrected unified canister locally and checked authentication, eligibility registration, account preparation, and movement of a real 10 USDC Spot deposit into Perps. Revalidation from real HL deposit through payout after the fixes, and real orders/cancellation, remain incomplete.

## Public environment

UI: https://private-perp-ui-testnet.hude.workers.dev . IC host: `https://icp-api.io`. Unified canister: `xis3j-paaaa-aaaai-axumq-cai`. Deployed on 2026-09-30; checked test-wallet signed login, HPKE-encrypted APIs, eligibility registration, account preparation, and balance queries. Chromium checks on the public UI also confirmed signed login, session-bound eligibility registration, deposit destination display, and logout. Public shared reserve: `0xc7ed130680612632b22ff0b23f9350977a33cd70` (zero USDC during verification). No transfer/order POST was sent. The cycle-related new-operation pause was false.

Keys and ledgers are separate from the local accounts below; the local 10 USDC was not moved.

## Local verification environment

- IC gateway: `http://127.0.0.1:18100`.
- Unified canister: `private_perp`, `4caro-hl777-77775-aaaba-cai`.
- HL: `https://api.hyperliquid-testnet.xyz/info` and `/exchange`.
- ECDSA: local `test_key_1`.
- Owner test wallet: `0x08ef566005b8f2b5ed94273add6cbbb414fe3bab`; real Perps balance 8 USDC.
- Shared reserve: `0x793dcb0dc098ff33c5aa8a115ee142a7b6b0ee1d`; Perps 10 USDC, Spot zero USDC.
- Trading account: `0x7e1c4ddb1fede6a8a2edb7efbbe8a483c43a1568`; real balance zero USDC.

The CLI owner test wallet's reserve, trading, in-transit, withdrawal-reservation, and withdrawable ledger balances are all zero. A different wallet sent the shared account's 10 USDC; it is not attributed to the CLI owner test wallet. Unknown outcomes, recovery fences, and fund requests are all zero. All accounts have zero positions and open orders. Environment preparation tests send neither transfers nor orders.

Keep private keys, seeds, sessions, and signed requests out of Git. Operation records are in untracked `.icp-home/hl-testnet/`. Addresses correspond to current canister state; retrieve them again from the canister after recreating accounts.

## Spot deposit ingestion

Ingest USDC `send` deposits to the shared account by matching sender, destination, received amount, and transaction hash. `send.amount` is the received amount; do not subtract the sender-paid `fee` again. For Spot deposits, check available balance and move funds into Perps with `usdClassTransfer` within the same shared account. Transfers from managed accounts are not new user deposits.

Conversion POSTs use nonreplicated HTTP outcall v2, with persisted send intent and single-use permission. Do not repeat POSTs after unknown responses. Credit the deposit only after replicated history queries confirm a conversion matching amount, direction, time window, and hash. Missing/saturated history or multiple candidates leaves the conversion unresolved. Do not reuse a consumed conversion hash for another same-amount deposit.

Deposits from unregistered senders are recorded in suspense. Signing in normally with that wallet automatically transfers only its own deposit history into its balance; another wallet's login cannot claim it. No dedicated manual-signature screen is needed.

Real HL showed one conversion-history entry for the 10 USDC deposit from `0x88f88c9667ecb746c11b8a0182f11f622ffbb844`. Shared Spot became zero and Perps 10 USDC; the CLI owner test wallet's 8 USDC and trading account's zero USDC were unchanged. UI login and balance retrieval with the sender wallet remain unverified because that wallet's signature is required.

## Fee accounting and verification

Persist `internalTransfer.usdc` as the sender's gross payment and `fee` as the deduction from the receipt. Credit deposits net of fees. Allocation clears the full gross in-transit amount and credits the trading account with the net receipt. Recovery `DepositCredit` also stores both amounts. Missing, invalid, or fee-greater-than-or-equal-to-gross values stop crediting.

PocketIC checked that a 9 USDC transfer credits 8 USDC and that a 5 USDC allocation leaves reserve 3 USDC, trading 4 USDC, and in-transit zero. Also checked duplicate-ingestion prevention, invalid-fee rejection, and balance reproduction after snapshot recovery. Recovery from old snapshots without verified send records retains the dispatch pause.

Passed all 157 PocketIC tests, unified production-Wasm verification, 82 Rust host tests, 46 frontend tests, four UI E2E tests with isolated local IC/mock HL, typechecking, lint, and 44 Lean proofs. [Walletless validation](walletless-validation.md) records execution conditions and remaining Linux issues. These do not guarantee real HL response completeness or public IC multinode behavior.

## Acceptance test conditions

`frontend/tests/integration/hl-testnet-smoke.test.ts` performs account preparation only with `TESTNET_SMOKE=1`. `TESTNET_REQUIRE_EMPTY_LEDGER=1` additionally checks zero ledger balances, fund requests, and holds. `TESTNET_PREFLIGHT_ONLY=1` reads the ledger and fund requests after authentication, checks that nothing is unsettled, and exits.

Explicit `TESTNET_ACCEPTANCE=1` enables acceptance with real transfers. `TESTNET_FUNDS_ONLY=1` records orders/cancellation as unverified and checks only fund movement.

The deposit helper permits only one 9 USDC testnet transfer to the prepared reserve and saves the signed request before dispatch. If a record already exists, reconcile instead of resending. The current owner wallet has 8 USDC, insufficient to run the transfer test as written. After a partial failure, inspect saved requests and real balances to select a resumption point instead of unconditionally rerunning the entire test.

## Remaining acceptance work

- Obtain enough test USDC and compare ledger and real balances for corrected deposit, allocation, recovery, and payout flows.
- Allocate enough principal to meet the minimum order value and verify real orders, cancellation, or reduce-only closing. The default 5 USDC allocation nets 4 USDC after fees, insufficient for order acceptance testing.
- Verify long-running public ICP operation, nonreplicated HTTP outcall v2 transfers, and outcome confirmation through replicated history queries.
- Keep `recovery_history_verified` false. Completeness of real HL history and unknown-outcome resolution have not passed acceptance.
