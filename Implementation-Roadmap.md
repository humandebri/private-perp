# private-perp: Phase-by-phase implementation and UI planning

> Historical design record. Dates, decisions, estimates, and verification status refer to the original record. See [implementation status](docs/implementation-status.md) and [single-canister architecture](docs/phase-3/single-canister.md) for later changes.


- Date of creation: 2026-09-18
- Version: v1.1
- Status: Implementation plan. Implementation, real-world verification, and audit are not completed.
- Reference documents: Plan.md v0.9, Implementation.md v0.5

Progress this time: 6 ADRs, Start + React/Workers foundation, 4-screen composite UI, and added demo testing for order and fund operations. Since there is no ICP code, Candid, or connection point, authentication, HPKE, and testnet connections are unimplemented. For details, refer to docs/implementation-status.md and do not count the completion of the entire Phase as a success.

## 1. What you aim for

MetaMask as the gateway, confidential canister manages funds and provides a service for trading on user-specific Hyperliquid (HL) accounts. Utilizes HL standard fill, margin, and liquidation to reduce the public links between connected wallets and trading accounts.

The goal of the UI is "a high-quality trading screen with a limited set of functions that HL users can use without any discomfort". The UI should offer more than a basic trading form without trying to reproduce every HL feature. We emphasize appearance, ease of input, and accuracy of state display from the beginning.

Screen smoothness and order execution speed are separate. We have measured delays in canister signing and reconciliation and do not promise the same execution speed as HL's official. We do not consider anonymity achievable only through shared custody of funds or TEE.

### Usage of documents

| a document | Role |
|---|---|
| Plan.md | Requirements, funds, key permissions, prohibited items, and live conditions |
| Implementation.md | Technical configuration, DB, signature, state machine, API boundary |
| this document | Implementation order, results by Phase, completion conditions, UI phase introduction |

This document does not change the existing authority model. If older phase descriptions still contain unresolved wording, follow the finalized decisions in Plan chapter 16 and Implementation v0.5. Record any new architecture decision in the governing design document.

## 2. Initial scope

### Included features

- EOA authentication such as MetaMask. When withdrawing, it is necessary to add the signature of the person.
- USDC on HyperCore. Deposits from the user's HL account and withdrawals to the same user's HL account.
- Shared reserve account and independent HL trading account for each user.
- master key management by funds_vault, trading agent management by trading_core.
- BTC/ETH perps, Market / Limit / Cancel / Cancel All / Close / SL / TP.
- HL standard positions unit SL/TP, reduce-only. Do not create independent settlement.
- Only the public status is directly connected from the browser to HL. The personal data is authenticated and encrypted and obtained through Canister.
- Control by SNS and change reservation and 7-day execution delay by control_guard.

### Excluded from the initial scope

- spot, HIP-3, all securities consolidated treatment, portfolio margin, unified account.
- Automatic deposit from other chains, unique bridges, withdrawal to third parties.
- Automatic strategies, TWAP, copy trading, ranking, referral function.
- Complex bracket orders, drag orders on charts, free multiple chart layouts.
- Native app, operator reset in case of EOA loss.
- A method of consolidating all players' positions into a single HL account.

The values of initial leverage, slippage, Agent expiration, holding period, etc. are in accordance with Chapter 16 of Plan. Do not change them implicitly during implementation.

## 3. Phases and dependencies

| Phase | Main purpose | Working features and deliverables | Conditions to proceed next |
|---|---|---|---|
| 0 | Fix installation contract and screen configuration | API, state transition, test specifications, screen specifications | Clear responsibilities during implementation and behavior in case of failure |
| 1 | Check the validity of technology and confidentiality | Fund transfer spike measurement and UI mock | Safety compliance. Identify the not met item and determine the connection range. |
| 2 | Complete the trading by yourself | Single user testnet app | Complete from deposit to withdrawal on the screen |
| 3 | Can be separated and restored even for multiple people | Multiple user testnet and load test | Verify fund separation, recovery of obstacles, and performance |
| 4 | Match the actual conditions and operation | Confidential basis evidence, rehearsal for withdrawal, transfer procedure | No unfulfilled operational conditions, or explicitly transferred to Phase 5 audit |
| 5 | It will be released for limited time after audit | Audit-approved release candidates and operational records | Full-game gate pass and separate execution approval |

Foundation development and UI mock can be done in parallel. However, even if the UI is completed, if the safety and privacy are not met, we will not proceed to the production stage.

## 4. Phase 0: Implementation contract and screen specifications

2026-09-19: Contract and screen specifications, and a Rust workspace template prepared (**implementation not yet completed**). The results will be placed in `docs/phase-0/`. The following checks indicate that the contract has been fixed, but do not mean that the Canister implementation and verification are complete.

### Contracts to define before implementation

- [x] Represent the responsibilities, permission callers, and actions that can be signed for each Canister. (`docs/phase-0/authority-matrix.md`).
- [x] define the API and error types for authentication, orders, funds, and change reservation. (`docs/phase-0/api-contract.md`)
- [x] Fix the state transition of withdrawal reservation, in transit funds, unknown, and order cancellation. (`docs/phase-0/state-machines.md`)
- [x] Defines the integer units of money, rounding, maximum value, and handling of duplicate events. (`docs/phase-0/money-and-units.md`)
- [x] Separate the ID, key, endpoint, and mock issuer for local testnet and mainnet. (`docs/phase-0/environments.md`. The real values are finalized in Phase 1.)
- [x] Address threats and tests. Include duplicate transfer, authorization bypass, old callbacks, and malicious upgrades. (`docs/phase-0/threat-test-matrix.md`. Tests are not executed yet)
- [x] Input privacy comparison, information to be passed to the attacker, and set the pass criteria. (`docs/phase-0/privacy-evaluation.md`).

### UI design

- [x] Decide the layout of the desktop trading screen, fund screen, and history screen. (`docs/phase-0/ui-spec.md` sections 2-4)
- [x] Define not connected, balance insufficient, in transit, unknown outcome, data delay, and screen paused not only in normal conditions (see paragraph 5).
- [x] Treat "custody balance", "trading account equity", and "withdrawable amount" as different values. (See paragraph 6)
- [x] Explain the confidentiality, create the wording for Canistercustody, EOA loss, and the recovery restrictions during suspension. (See paragraph 7)
- [x] Check the necessary functions of the chart and the usage conditions, and narrow down the candidates for recruitment. (Same as paragraph 8. Continue using Lightweight Charts, and the remaining unevaluated items are Advanced Charts.)

### Results and completion conditions

The output includes an API contract, state transitions, permission tables, test list, and screen specifications. It must also include a completion condition that allows another implementer to determine whether the responsibility for success, failure, or retry is theirs.

The investigation of the operating entity, target country, compliance with the terms and conditions, and SNS allocation will be conducted concurrently. Although synthetic data development can be carried out even if these are undecided, it does not permit the recruitment and acceptance of actual customers.

## 5. Phase 1: Fund transfer, signature, privacy verification

2026-09-19: Implementation order 1 (Rust action construction, signing payloadhash, signature verification, and comparison test with the official SDK) completed (`docs/phase-1/README.md`). The PocketIC foundation has also been confirmed to be established. **Other items are not yet completed**, and no mandatory verification is being executed for the items listed below.

### Implementation order

1. Pure action construction in Rust, signing payloadhash, signature verification, and comparison tests with the official SDK.
2. EOA challenge, short-lived session, HPKE request/response, authentication-verified public key acquisition.
3. Integer double-entry ledger, fund reservation, permanent outbox, nonce, fencing.
4. Normal bidirectional operation and injection of faults using Mock HL, and the permanentization and upgrade test of PocketIC.
5. Master/Agent approval on HL testnet, USDC deposits, allocation, recovery, withdrawal, minimum orders and cancellation.
6. Local verification of control_guard reservations, SNS authorization, deferrals, and bypass denials.
7. Public trace privacy evaluation, delay in signature and reconciliation and cost measurement.

### Required verification

- [ ] Only add the deposit once, and do not include the undecided transfer in the final balance.
- [ ] verify the signature of the person withdrawing, the recipient, the amount, the deadline, and the nonce.
- [ ] Even if there is a response loss after transfer success, we will not double charge with automatic resend.
- [ ] You cannot request master signing or arbitrary withdrawal from trading_core.
- [ ] There is no query from the browser to HL regarding the per-user trading account.
- [ ] signaturep50/p95, reception→HL reception p50/p95, record the delay in reconciliation, outcall/signature/storage cost.
- [ ] Record whether Confidential Subnet is available and the unverified trust assumption.

### Privacy evaluation

Compare A (direct deposit), B0 (immediate equal allocation after common custody), and B1 (candidate options such as allocation timing separation) of Plan 16.6. Use a synthetic history equivalent to 1/5/20/100 users and 30 days, and actual testnet observations.

For more than 20 evaluation groups, the development benchmark is a success rate of less than 20% in top-1 matching, more than 80% reduction in A ratio, and less than 5% of the proportion that can be directly identified. In addition to the average, we report conditions including characteristic amounts, repetition, profit and loss, and withdrawal separately. This benchmark is not a mathematical guarantee of anonymity.

### Parallel UI work

Create a trading screen for synthetic data and check the information volume and operation sequence of the chart, order book, order form, and position list. Always display that it is a mock, and do not connect to real accounts or accept funds. Prioritize the basic components that can be used even if the funding path changes.

### Results and progress judgment

The results include reproducible spikes, signature fixtures, protected traces of fund transfer logs, measurement reports, correlation assessments, and manipulable UI mocks.

- Safety not met: Stop the fund connection and fix the state machine and authorization.
- Privacy not met: redesign the funding allocation method. General UI and synthetic data development can continue, but it will not meet the requirements for a privacy product.
- Confidential foundation not secured: Continue only with synthetic data and testnet verification in normal environments.
- Acceptance→HL acceptance p95 exceeds 5 seconds: UX for general use is not met. Do not weaken confidentiality, and perform verification and improvement of the order entity on testnet.

Without the final funding pathway being decided, we will not invest heavily in UI and actual implementation dedicated to that method.

## 6. Phase 2: Single user testnet MVP

### Target for implementation

- [ ] MetaMask connection, signature login, expired/reauthentication/logout.
- [ ] Generation of dedicated HL accounts, Agent approval and expiration date display.
- [ ] USDC deposit information, confirmation status, allocation, recovery, withdrawal to the person in charge.
- [ ] BTC/ETH market status, candlestick chart, board, public fill.
- [ ] Market/Limit orders, enter quantity, price, leverage, and slippage.
- [ ] Order list, positions, PNL, margin, settlement price, SL/TP.
- [ ] Cancel, Cancel All, partial/full payment. Dangerous bulk operations are confirmed with a pause.
- [ ] Fund history and order history, data update time, communication and service status.
- [ ] The minimum authentication, cancellation, and withdrawal client available when the UI is stopped.

### Representative acceptance scenarios

1. Log in and review the risk explanation.
2. Deposit test USDC from the personal HL account and allocate the confirmed balance to the trading account.
3. Place a market order and check the cancellation.
4. Hold a position and set HL standard SL/TP.
5. Make partial payment, full payment and recover the funds.
6. Sign the withdrawal details in EOA and confirm the receipt of funds into the HL account of the person.

### UI and failure testing

- Reload the browser, close tabs, log out of session, reconnect to the internet.
- Display partial fill, cancellation in progress fill, HL rejection, and send unknown outcome correctly.
- Double orders or duplicate withdrawals will not occur even if you repeatedly press the send button or request reception resend.
- In account states older than 10 seconds, stop new risk increases and display the reason and observation time.
- Verify representative flow and failure flow in Playwright. Fund safety is reconciled not only with screen tests but also with DB and external states.

The completion condition is that the above two-way operation passes without manual DB correction, and that the funds and order status can be restored after restarting. The success of a single user does not serve as proof of achieving anonymity.

## 7. Phase 3: Multiple users, load, recovery

### Target for implementation

- [ ] Separation of funds, accounts, agents, and sessions per user.
- [ ] Coordination of simultaneous orders, simultaneous withdrawals, and orders in recovery.
- [ ] Shared REST budget, user-specific limits, back-off, priority processing for cancellation and reconciliation.
- [ ] agent generation update, expired, refusal of old callback.
- [ ] Stop transmission and reconciliation during DB migration, upgrade, and restoration of old backups.
- [ ] Notification of cycles, suspension of new applications, securing budget for withdrawal.
- [ ] Eligibility verification, audit logs, and retention/deletion jobs separated from production use.

### UI finishing

- Share public market data connection across multiple tabs.
- Even with high-frequency market updates, make sure that the input, scroll, and cancellation buttons do not freeze.
- Uniformize the decimal places, precision, color, focus, and error messages for numbers.
- Allows you to check balance, cancel, pay, and withdraw from your notebook PC with a narrow screen and on your mobile device.
- Do not display all panels at the same time on mobile, instead switch display.

### Completion conditions

It is mandatory that you cannot refer to and operate other users' balance and orders, do not repeat execution even under load, and do not resolve unknown without reason.

The concurrent usage is set based on Phase 1 measurements. Even if a synthetic 20-person or 100-person test is conducted, do not call it the guaranteed number of users; instead, record the limit along with the API budget, delay, and cost. Continue reconciliation even when closing the browser.

Re-conduct correlation tests for multiple users. Separate the evaluation of funding separation, confidentiality, and performance, and do not substitute one success for another.

## 8. Phase 4: Actual base, operation, exit

### Mandatory work

- [ ] Check the scope of protection for Confidential Subnet attestation, outcall, state sync, upgrade, and recovery.
- [ ] Do not assume that TEE abnormalities cannot be observed, but set the stop conditions based on the signals that can be confirmed.
- [ ] We will test the avoidance of bypasses including 7-day delay and policy through guard's SNS connection in a live environment.
- [ ] We simulate cancellation, recovery, and withdrawal, taking into account the respective stops of UI, Canister, HL, and SNS.
- [ ] Organize the recovery restrictions, cycles replenishment, and dependency ID fixation for auditing when guard cannot be changed.
- [ ] Determine the operator, target area, terms and conditions, required identity verification, storage obligation, and fees.
- [ ] Create the procedure for SNS allocation, centralized voting rights, launch, and power transfer.
- [ ] Check reproducible build, WASM hash, controller configuration, and dependency library conditions.

### Things to add to UI

Display change proposals and executable times, fund recovery procedures, failure states, and support pathways. Do not express "You can definitely exit if you have 7 days" or "You can't move funds even in a DAO."

The completed results include proof of base verification, operational runbook, performance rehearsal records, release candidates, and audit materials. The irreversible controller removal and SNS launch for the live event cannot be executed solely by creating this document.

## 9. Phase 5: independent audit and limited mainnet

- [ ] Fund ledger, master/Agent permissions, signature, authentication, encryption, upgrade, guard, recovery are audited independently.
- [ ] Independently review the correlation resistance of the final funding route and determine whether it can be provided even under actual low utilization conditions.
- [ ] Correct major problems and complete the regression test and re-review.
- [ ] Determine the maximum total deposit limit, user limit, trading limit, suspension conditions, and the person in charge for the actual operation.
- [ ] Check the actual fees, terms and conditions, privacy explanation, and the scope of protection to be disclosed.
- [ ] After obtaining separate approval, we will transfer permissions, set up the live setting, and conduct limited reception.

The fact that it is an invitation-only system and that the amount is small does not replace auditing, confidentiality, or legal compliance. If a small number of participants cannot meet the conditions for confidentiality, they will not start new deposits and will remain on the testnet. Synthetic users will not be counted as anonymous participants.

Exit of existing users is protected. Withdrawal is not indefinitely withheld because of the breakdown of the anonymity conditions.

## 10. UI policy: Quality and functions are limited to those close to HL

### 10.1 Basic desktop configuration

```text
┌ Stock/Price/Connection Status/Funds/Account ────────────┐
│                                            │
│        Chart         │ Board/Public fill │ Order input │
│                         │             │          │
├────────────────────────────────────────────┤
│ positions / unfilled orders / fill history / fund history             │
├────────────────────────────────────────────┤
│ Data update time, service status, and explanation of confidentiality │
└────────────────────────────────────────────┘
```

This is an information configuration policy and not a directive to replicate HL's brand or assets. It aims to make it easy to move between charts and order entry, and to move directly from positions to SL/TP, cancellation, and settlement.

### 10.2 UI priority

| Necessary from the beginning | Add later | Not applicable for the time being |
|---|---|---|
| Candle feet, time feet, zoom, crosshair | In-demand indicators and drawings | Comprehensive reproduction of analysis tools |
| Board, public fill, Market/Limit input | Order and SL/TP operation on the chart | Advanced automatic strategy UI |
| positions, PNL, margin, clearing price, SL/TP | Save layout, add shortcuts | Free multiple chart terminals |
| Fund allocation, deposits, withdrawals, history | Add deposit path | Unique bridge |
| pending/unknown, update time, reconnection | Notification and detailed analysis | Ranking and introduction measures |

Use the criterion of "cutting is about function numbers, not the quality of basic operation."

### 10.3 How to display order status

- Immediately after entering, the local "Reception Confirmation in Progress" is displayed. Only after confirming the Canister reception will it be marked as "Received".
- Distinguish between "Sending ready", "Sent/Confirmed", "HL received", "Partial fill", "fill", "cancellation confirmation", "cancellation completed", "rejected", "unknown outcome".
- Do not display timeouts as failures or cancelled. Do not encourage re-submissions when unknown outcome.
- Display the age of the data and provide the reason and recovery conditions when restricting operations.
- If you cancel protected orders with Cancel All, you must confirm it on the confirmation screen. Do not treat order cancellation and position settlement as the same operation.

### 10.4 How to show funds and privacy

Separate the unallocated balance in the shared reserve account, the equity in the HL trading account, the amount in transit, and the confirmed withdrawable amount. If you want to display the total value, do not double count, and include or indicate the unrealized PnL.

Confidentiality is not explained by the "Private" badge alone. It explains that account information on HL is publicly available, that the amount and time of deposits and withdrawals can be inferred from them, and that trust remains in Canister and its change permissions. Anonymous scores without measurement results are not displayed.

### 10.5 Chart selection policy

Maintain the Lightweight Charts of the existing plan as the initial candidate. If advanced drawing or indicators are required for initial use, check the usage conditions and integration cost of Advanced Charts in Phase 0 and re-choose them before implementation.

The assertion in the existing Implementation.md that "Advanced Charts cannot be used in commercial closed environments" does not serve as a basis for hiring decisions. Public services for enterprises and source code confidentiality are separate conditions, and official provision terms and contracts must be confirmed. This does not guarantee the adoption of Advanced Charts.

Reference: [TradingView official library comparison and terms of provision](https://www.tradingview.com/free-charting-libraries/). Do not assume that all HL chart functions are included just by the name of the library, and check each necessary function separately.

### 10.6 Acceptance criteria for UI quality

- Fix the representative terminal and browser in Phase 0 and measure in that environment.
- Do not wait for network completion and update local display for transmission operation within 100ms as a target. This is not the HL acceptance time.
- Even while the market is updating, you can also enter orders, scroll, and cancel.
- Main operations can be performed with the keyboard, and it does not convey profit and loss or errors only by color.
- Long price and quantity, empty data, partial fill, even on a narrow screen, the operation is not hidden.
- In reverse order of reconnection and updating, the old balance and orders are not overwritten as the latest.
- Combine functional testing by Playwright, visual verification of the main screen, and manual operation in the real-world.

## 11. Operation of task and completion report

Each task has the following items: Do not "complete" the entire Phase in one go, but leave evidence.

```text
ID：P2-ORDER-01
Purpose: Display from receipt of limit order to HL receipt
Dependencies: authentication, order API, reconciliation, identity data retrieval
Target implementation: API / Canister / UI
Not applicable: Orders from the chart
Acceptance conditions: Normal/rejected/unknown/re-reading are all correct states.
Verification: Single / PocketIC / testnet / Playwright
Evidence: Test results, revision target, required protected records
Remaining items: impact of unverified conditions and progress
```

Changes related to funds, signature, authentication, and guard cannot be completed by just successful path. The success of UI mock is not counted as the success of the real funds flow. Each Phase is divided into safety, confidentiality, performance, UI, and operational judgment.

## 12. Next steps and estimates

The first work units should be in the following order.

1. Make the API, funding status, and screen specifications of Phase 0 into short reviewable units.
2. Set up a Rust workspace and fixed dependencies, and a local test environment.
3. Implement the idempotency of signaturefixture, integer ledger, and fund request.
4. Allows normal bidirectional communication, loss of response, and double execution refusal in Mock HL.
5. Create a trading screen for synthetic data simultaneously and check the operability.
6. Perform testnet bidirectional and correlation evaluation to determine the adoption pathway, performance, and cost.

Do not reuse the labor hours of the old Agent-only proposal. At the end of Phase 1, re-estimate by dividing the work into base, UI, external dependencies, and audit waiting. Do not shorten unresolved funding or confidentiality conditions based on the completion of the UI.

The main deployment will be with Cloudflare Workers + Static Assets. Next.js/Preact will not be used. Oxlint + type-compatible, Oxfmt, tsc, Vitest, and Playwright will be used, and the funding, signature, and order status will be retained in ICP. The Plan and Implementation will also be aligned with this ADR. No live operations will be carried out.
