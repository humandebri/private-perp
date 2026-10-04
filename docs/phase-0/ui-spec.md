# Screen specifications: composition, state display, description text, chart

> Historical Phase 0 design contract. Later implementation and validation results are recorded in the phase reports and root README.

- References: Roadmap Chapter 10, `Plan.md` 1.4, 16.1, 16.4, 16.5, `Implementation.md` 6.1–6.3, `docs/adr/0004`, `0006`
- Status: Design contract. Implementation will be after Phase 2. The current `frontend/` is a demo dedicated to synthetic data and does not mean the completion of the implementation of this specification.

## 1. Scope of application

- The target is the 4 screens of desktop trading, funds, history, public explanation and the terms and explanation text of status display.
- This specification is an implementation instruction and does not treat the completion of the screen as proof of funds and confidentiality.
- "VEIL" is a provisional name for work purposes. It is not the final product name or trademark.
- Do not display product names or reproduce HL's brand and assets.

## 2. Basic desktop configuration

```text
┌ Stock/Price/Connection Status/Funds/Account ──────────────┐
│                                              │
│      Chart        │ Order book/Public trades │ Order input   │
│                      │              │            │
├──────────────────────────────────────────────┤
│ positions / unfilled orders / fill history / fund history             │
├──────────────────────────────────────────────┤
│ Data update time, service status, and explanation of confidentiality │
└──────────────────────────────────────────────┘
```

| Area | Responsibility |
|---|---|
| Header | Market selection, latest price, connection status of public market status, summary of custody balance/trading account equity, account (connection EOA, session expiration, logout) |
| a chart | Candlesticks, time intervals, zoom, crosshair. The price line for positions and orders will be shown later. |
| Order book/Public trades | Order book (`l2Book`), public trades (`trades`). Overlaying the user’s own orders on the order book is after Phase 3. |
| Order input | Direction, type (Market/Limit), quantity, price, leverage, slippage, reduce-only, SL/TP, confirmation |
| Bottom tab | positions (equity, margin, liquidation price, PNL, SL/TP, position closing), unfilled orders (cancellation), fill history, fund history |
| Foot | Data update time (separate from the person's status and market conditions), service status, guide to the explanation of confidentiality |

- It makes it easy to go back and forth between charts and order entry, and allows you to proceed directly from positions to SL/TP, cancellation, and settlement.
- Information distribution is a policy and does not replicate HL's brand assets.

## 3. trading screen (`/trade`)

| Element | Content |
|---|---|
| Market | Only BTC/ETH perps. Cannot be selected outside the initial allowlist. |
| Order input | Enter quantity and price as strings. Reject input with precision violations at the time of entry and do not round. |
| Market | Explicitly state the slippage limit (default 0.5%). Explain that it is an IOC limit order. |
| Limit | GTC. `expires_after` is explained as the acceptance deadline and not the cancellation deadline for the board. |
| Leverage | Default 3× and maximum 5×. Explicitly stated that it is a restriction for development and not a recommended multiplier. |
| SL/TP | HL `positionTpsl` in positions units, reduce-only. Complex brackets are not provided at the same time as entry. |
| transmission | Immediately after sending, output the local "Processing Confirmation" line. Link `cloid` and `order_id` with the response of `submit_order`. |
| positions | equity, margin, liquidation price, unrealized PnL, SL/TP, partial/full position closing |
| Dangerous operation | Cancel All, closing all positions requires confirmation. If Cancel All also deletes protective SL/TP, it will be explicitly stated. |

- Instead of "showing as sent", show "received and in transit" correctly.
- Do not treat position closing and order cancellation as the same operation.
- An emergency stop does not automatically market-close positions.

## 4. Funds screen (`/funds`) and History screen (`/history`)

### 4.1 Fund screen

| Element | Content |
|---|---|
| balance classification | Display unallocated (custody), in transit, withdrawal reservation, trading account equity, and withdrawable amount separately (Section 7) |
| Deposit | Notice of the signature transfer from the personal HL account to the shared reserve account. Show the confirmation status. |
| allocation | From shared custody to per-user trading account. Display the status of receipt, transmission, and reconciliation. |
| recovery | From trading account to shared custody. Display restrictions when there are unfilled orders and positions. |
| withdrawal | Clearly indicate the order of entering the amount → EOA signature (owner authorization) → reception → recovery → disbursement. Do not select the option for third parties. |
| restriction | Do not pay in advance for unconfirmed recovery/unconfirmed PnL. Do not include the corresponding amount in the withdrawable amount while there is `unknown`. |
| Agent | Current/next generation, approval status, validity period, expiration request. Indicate address, purpose, permissions, and expiration date. |

### 4.2 History screen

| Element | Content |
|---|---|
| Category | fill history, order history, fund history (deposit, allocation, recovery, withdrawal, fees) |
| Pagination | Cursor mode. Display the number limit and do not scan all items with unlimited scrolling. |
| Each row | Time, type, amount or quantity/price, condition, reference ID (shortened display such as cloid) |
| unknown | Display unresolved `unknown` in the dedicated area at the top of the history and leave it until resolved. |
| Data freshness | Show observation times and revision in the list |

## 5. Abnormal screen state

| State | Display | Allowed operation | Prohibited |
|---|---|---|---|
| Unconnected (wallet not connected) | Connection controls and restrictions to read before depositing (Section 9) | Viewing public explanations and public market conditions | Orders and fund operations |
| Session expired | Reason for expiration, re-authentication path. Discard local notes | Reauthentication | Continue using in the previous session |
| Insufficient balance | Insufficient amount, necessary amount, allocation guide | Allocation and deposit information | Order submission (indicate the reason) |
| Sending (Confirmation of receipt → Received → Preparing to send → Sent and confirmed) | Canonical state-machine (`state-machines.md` Section 8). Explicitly indicate the possibility of HL not being reached. | Cancellation request (distinguish between suspension before sending and cancellation on HL) | Filled display, double sending |
| Partial fill | Cumulative fill amount/order quantity, remaining quantity | Cancellation, additional position closing | Displayed as full fill |
| Cancellation confirmation in progress | Status of sending cancellation request | Waiting for reconciliation results | "Cancellation completed" display |
| unknown outcome | "unknown outcome (do not resend)", start time, that it is in reconciliation | Request for inquiry, cancellation request, reduce-only | Encouraging order resubmission |
| Data delay (over 10 seconds) | Observation time, delay amount, reason, recovery conditions | Reference, cancellation, reduce-only, confirmed withdrawal | Increase in new risks (new orders and leverage changes) |
| Reconciliation in progress (immediately after startup or after recovery) | "Exchange state is being reconciled", progress, not starting transmission | reference | New orders/allocation |
| Service suspension (emergency suspension and new application suspension) | Reason for stopping code, start time, notification line | Reference, cancellation, reduce-only position closing, confirmed withdrawal | New orders and new deposits |
| Guard change reservation is pending | Target, executable time, hash of the changes (excluding customer information) | Withdrawal procedure | The expression "You can definitely exit" |
| Cycle reduction | Notification stage (goal 30 days / notification 7 days / suspension 3 days) | Reference and exit | New deposit (3-day stage) |

- Do not display timeouts as failures or cancelled.
- Display the age of the data and provide the reason and recovery conditions when restricting operations.
- The state is updated by comparing `revision` and `observed_at`, and never overwrite newer state with an older response.

## 6. rules for displaying balance

| division | Meaning | Source |
|---|---|---|
| custody balance (unallocated) | The amount that has not yet been allocated to the trading account in the shared reserve account. | `FundStatus.reserve_unallocated` |
| in transit | An amount that has not been confirmed in either account while allocation/recovery is in progress. | `FundStatus.in_transit` |
| withdrawal reservation | The amount that is bound by withdrawal requests | `FundStatus.reserved_for_withdrawal` |
| trading account equity | Equity of user-specific accounts returned by HL | `FundStatus.trading_equity` |
| unrealized PnL | If included in equity, it will be separately indicated as a breakdown. | `FundStatus.trading_unrealized_pnl` |
| withdrawable amount | The amount that can actually be disbursed after confirming the backing and owner authorization | `FundStatus.withdrawable` |

- Do not double count when showing the total value. Include or explicitly indicate unrealized PnL in equity.
- Do not simply add "custody balance + trading account equity" as "total asset". Show in transit and withdrawal reservation separately.
- Do not add the unknown amount to the withdrawable amount. During the `unknown` period, display this information near the amount.
- Do not display anonymity scores without measurement results.

## 7. Description text (foundation)

It must be displayed before deposit. The text will require a review when changed.

### 7.1 about confidentiality

> Information about the trading account is publicly available on Hyperliquid. From the amount, time, and frequency of deposits and withdrawals, you may infer the relationship between accounts. This service does not directly display the connection between the connected wallet and the trading account on the screen or in the public logs, but it does not guarantee that the relationship can be completely hidden. The correlation resistance measurement results will be published separately.

### 7.2 About Canister custody

> Funds are managed by a shared reserve account managed by Canister on the Internet Computer and a per-user trading account. The private key is managed by Canister's threshold signature and cannot be retrieved by the operator of this service alone. However, trust in Canister's code and its change authority remains.

### 7.3 Losing EOA

> If you lose your connected wallet (EOA), you will no longer be able to log in or withdraw with owner authorization. We do not provide recovery via email or other means, nor do we reset authentication by the operator. The recovery of funds is not guaranteed. Please manage your important wallets carefully to avoid losing them.

### 7.4 Recovery restrictions when stopped

> Due to the suspension of Canister, malfunctions, suspension on the Hyperliquid side, unresolved transfers, and margin restrictions, you may not be able to withdraw at your desired time. This service does not guarantee that you can recover your funds on your own at any time. Trading suspension and withdrawal suspension are separate, and even after the suspension of new orders, we will prioritize cancellations, reduce-only settlements, and confirmed withdrawals within the scope of confirmation.

### 7.5 About the 7-day change allowance

> There is a 7-day waiting period for updates to Canister, which handles funds. This is an opportunity to consider withdrawing funds, but it does not guarantee the recovery of funds. Changes made after the waiting period may affect remaining funds or stored information. This waiting period does not apply to screen delivery (front-end).

### 7.6 Expressions not to use

| Prohibited expressions | Reason |
|---|---|
| Explain confidentiality only with badges such as "Private" | Not explaining the scope of protection |
| "Even in DAO, you can't move funds." | Cannot deny the possibility of infringement of change permissions |
| "If you have 7 days, you can definitely leave." | It is not a recovery guarantee |
| Unmeasured anonymity score | Do not provide unmeasured values |
| "Non-custodial" | Canister and trust in change permissions remain |
| "Principal guarantee", "No liquidation" etc. | Misleading the trading risk |

## 8. Chart policy

| Item | Decision |
|---|---|
| adoption | Lightweight Charts (Apache-2.0). Same as the current demo |
| Essential initial function | Candlesticks, time intervals (1m~1M), zoom, crosshair, resize, handling up to 5,000 records |
| following | In-demand indicators, drawing, order/SL/TP operations on charts |
| Not applicable for the time being | Comprehensive reproduction of analysis tools, free multiple chart placement |
| Advanced Charts / Trading Platform | **Not evaluated**. When advanced drawing and indicator are required for the initial mandatory functions, check the public form, attribution, and contract terms and re-evaluate. |
| reference | [TradingView official comparison and provision conditions](https://www.tradingview.com/free-charting-libraries/) |

- We do not base the decision on the assumption that "Advanced Charts cannot be used in commercial closed environments."
- Don't assume that all the HL chart features are included just by the library name, but check each necessary feature individually.
- Maintain display that meets attribution requirements (Lightweight Charts).

## 9. Connection to public market conditions and data freshness

Based on `Implementation.md` 6.1 through 6.3.

| Requirement | Content |
|---|---|
| Direct connection configuration | Connect only to public market status `wss://api.hyperliquid.xyz/ws` (mainnet) / `-testnet` (testnet) |
| Channel | `allMids`, `l2Book`, `trades`, `candle`, `bbo`, `activeAssetCtx` |
| Tab sharing | Do not connect to each tab (`BroadcastChannel` + leader election). Follow the 10 connections/IP limit. |
| maintenance | Ping is regularly sent because it is disconnected without any message for 60 seconds. |
| Reconnection | Detect `isSnapshot: true` and replace the state. Do not apply as a difference. |
| Actual state | Only update the canister reconciliation result with the revision and observation time. It does not confirm by inferring fill from the public market status. |
| Subscription management | Subscribe when you need it and unsubscribe when you leave the screen. |
| Update performance | Even while updating at high frequency, the input, scroll, and cancellation buttons do not freeze. |

## 10. Quality and accessibility acceptance standards

- In Phase 0, fix the representative device and browser and measure in that environment (the representative environment is recorded when starting Phase 1).
- Do not wait for network completion and update local display for transmission operation within 100ms as a target. This is not the HL acceptance time.
- Main operations can be performed with the keyboard, and it does not convey profit, loss, error, or status only by color.
- Long price and quantity, empty data, partial fill, even on a narrow screen, the operation is not hidden.
- On mobile, it switches to a display that allows you to operate balance confirmation, cancellation, position closing, and withdrawal without displaying all panels at the same time.
- In reverse order of reconnection and updating, the old balance and orders are not overwritten as the latest.
- Combine functional testing by Playwright, visual verification of the main screen, and manual operation on the actual device. Funding security is not determined solely by screen testing, but also reconciled with DB and external conditions.

## 11. Minimum alternative client (Phase 2 eligible)

- Design a system that provides the minimum authentication, cancellation, and withdrawal clients available when the UI is stopped.
- It is not an alternative execution basis for the stopped Canister. It only handles the transmission of authentication, cancellation, and withdrawal requests.
- Display is limited to the minimum (state, observation time, results of cancellation/withdrawal) and does not omit the explanation of confidentiality.

## 12. Undecided matters

| Item | Fixed period |
|---|---|
| Representative terminal/browser, performance measurement procedure | Phase 1 |
| Whether to use Advanced Charts | The point when high drawing became a mandatory requirement at the beginning |
| The specific details of mobile switching display | Phase 3 |
| Whether notification (fill, stop, cycles) is necessary | Phase 3 |
| Layout saving and shortcuts | After Phase 3 |
