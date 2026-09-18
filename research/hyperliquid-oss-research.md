# Reusable open-source software for a Hyperliquid perp trading frontend

**Verified:** 2026-09-18 (UTC). Star counts, licenses and `pushed_at` dates were read from the GitHub REST API
(`https://api.github.com/repos/<owner>/<repo>`) on that date; licenses not reported by that API were read from the
repository `LICENSE` file. Feature claims for SDKs were verified by reading source/type files, not only READMEs.

**Conventions:** every claim has a URL. Anything not evidenced by a source is explicitly marked
**[unverified]**. "No license" means the GitHub API reported `license: null`, i.e. no license file was detected —
which legally means all rights reserved by default (I state the legal consequence as an interpretation, not as a
source claim).

---

## 1. Full open-source Hyperliquid trading terminals / frontends

### 1a. Single-venue Hyperliquid UIs

| Project | License | Lang / framework | Stars | Last push | Maintained? | Reusable parts |
|---|---|---|---|---|---|---|
| [vipineth/hypeterminal](https://github.com/vipineth/hypeterminal) | MIT | TypeScript (React 19, TanStack Start, Vite 7, Tailwind v4, Zustand) | 17 | 2026-08-14 (repo created 2025-12-16) | Yes (recent pushes) | **Highest reuse value.** pnpm monorepo: `apps/terminal` is the full app (order entry, chart, order book per its README map); `packages/hl-react` = transports, info/sub/exchange hooks, **signing, agent wallets, WS reliability**; `packages/ui` = design system; `packages/hyperliquid-api` = an agent skill. Source-shipped packages (`"main": "./src/index.ts"`) so they can be consumed without a build step. |
| [nilesjarvis/kerosene](https://github.com/nilesjarvis/kerosene) | MIT | Rust (iced desktop) | 12 | 2026-09-17 | Yes | Whole desktop app: candlestick charts (1m–1M), live L2 order book with tick grouping, positions with streaming PnL, order entry, drag-to-move limit orders on chart, journal. Useful as a Rust/iced reference or fork; not embeddable in a web frontend. |
| [Superior-Trade/trading-terminal](https://github.com/Superior-Trade/trading-terminal) | Apache-2.0 | TypeScript / Next.js 16 | 143 | 2026-09-18 | Yes | Self-hosted "AI trading terminal" for Hyperliquid: chart annotation → strategy → live deployment, bracket orders, deposits/withdrawals. Chart is **TradingView Advanced Charts**, with a bundled Lightweight Charts preview; README says you must supply your own TradingView access before the chart builds. |
| [Co-Messi/HyperData-Terminal](https://github.com/Co-Messi/HyperData-Terminal) | Apache-2.0 | Python (TUI) | 17 | 2026-07-22 | Yes | Terminal (TUI) dashboard: order flow, whale tracking, liquidation cascades across Hyperliquid/Binance/Bybit/OKX/Deribit. Reference for data flows only; no web UI components. |
| [Virmage/HlOne](https://github.com/Virmage/HlOne) | MIT | TypeScript | 1 | 2026-06-25 | Low activity (1 star, 0 forks) | Claims perps + options + copy trading + whale tracking. **[unverified]** I did not audit its component structure or feature completeness. |
| [hyperliquid-dex/hyperliquid-stats-web](https://github.com/hyperliquid-dex/hyperliquid-stats-web) | MIT | TypeScript | 39 | 2024-12-02 | **Effectively stale** (~21 months) | Fork of [thunderhead-labs/hyperliquid-stats-web](https://github.com/thunderhead-labs/hyperliquid-stats-web) (MIT). It is a *stats explorer*, not a trading terminal (repo description: "Explore the Hyperliquid protocol's statistics"). Reusable as chart/data-viz reference only. |

**Unlicensed (do not reuse without permission):**

- [eugene-gourevitch/hypertrade-platform](https://github.com/eugene-gourevitch/hypertrade-platform) — TypeScript, 2 stars, last push 2025-10-23, **no license file**, README is 14 bytes. Description claims a "Bloomberg-style trading interface for Hyperliquid DEX with real-time data, order execution, and position management". Not usable commercially as-is.
- [jestersimpps/hyperscalper](https://github.com/jestersimpps/hyperscalper) — TS, 13 stars, last push 2026-05-14, no license.
- [laomoai/tide-hyperliquid](https://github.com/laomoai/tide-hyperliquid) — TS, 3 stars, last push 2026-05-09, no license.
- [PetaTechSolution/HL-OrderBook](https://github.com/PetaTechSolution/HL-OrderBook) — JS, 0 stars, last push 2025-09-20, no license.
- [Jon-Becker/hyperliquid-frontend](https://github.com/Jon-Becker/hyperliquid-frontend) — TS "Hyperliquid-style perpetuals DEX frontend prototype (testnet)", 0 stars, last push 2026-08-26, no license.
- [liquary-xyz/liquary-xyz](https://github.com/liquary-xyz/liquary-xyz) — "Hypersight: self-custodial trading frontend for Hyperliquid", 0 stars, last push 2026-08-07, no license. **[unverified]** (description only; I did not audit it).
- [aiwebarchitects/Hyperliquid-Trading-Bot](https://github.com/aiwebarchitects/Hyperliquid-Trading-Bot) — Python + GUI panel, 3 stars, last push 2025-11-09, no license.

**Official frontend:** the `hyperliquid-dex` GitHub organisation publishes SDKs, node software and an order-book
server, but **no trading web UI** — its public repo list is: `hyperliquid-python-sdk`, `contracts`, `historical_data`,
`hyperliquid-stats`, `hyperliquid-stats-web`, `hyperliquid-rust-sdk`, `ts-examples`, `node`, `hyper-evm-sync`,
`block-importer`, `order_book_server` ([org repo list](https://github.com/orgs/hyperliquid-dex/repositories)).
So `app.hyperliquid.xyz` itself is **not open source as far as I could verify** — I found no official repository for
it. [unverified: absence of an official OSS app repo is a negative finding, not a positive statement about the app's
licence.]

### 1b. Multi-venue "trading terminal" projects that list Hyperliquid

| Project | License | Lang | Stars | Last push | Hyperliquid support (evidence) | UI? |
|---|---|---|---|---|---|---|
| [hummingbot/hummingbot](https://github.com/hummingbot/hummingbot) | Apache-2.0 | Python | 20,046 | 2026-09-17 | Yes — spot connector `hummingbot/connector/exchange/hyperliquid/hyperliquid_exchange.py` and perp connector `hummingbot/connector/derivative/hyperliquid_perpetual/hyperliquid_perpetual_derivative.py` both exist (**HTTP 200 on raw.githubusercontent**, verified). | No trading UI (bot engine). |
| [hummingbot/dashboard](https://github.com/hummingbot/dashboard) | Apache-2.0 | Python | 367 | 2025-10-27 | Companion UI for Hummingbot instances | Yes, but it **manages/deploys bot instances**, it is not an order-book/order-ticket terminal. Stale-ish (last push ~11 months). |
| [freqtrade/freqtrade](https://github.com/freqtrade/freqtrade) | **GPL-3.0** | Python | 54,490 | 2026-09-18 | Yes — [freqtrade docs](https://www.freqtrade.io/en/stable/exchanges/): "Hyperliquid futures isolated, cross limit", "Hyperliquid spot ❌ (not available)", plus Hyperliquid Subaccount / Vault / HIP-3 DEX sections. Auth = `walletAddress` + API-wallet `privateKey` (ccxt). | No; UI is freqUI. |
| [freqtrade/frequi](https://github.com/freqtrade/frequi) | **GPL-3.0** | Vue | 1,077 | 2026-09-17 | Frontend for Freqtrade | Yes (bot dashboard, not a live order-book terminal). |
| [Drakkar-Software/OctoBot](https://github.com/Drakkar-Software/OctoBot) | **GPL-3.0** | Python | 6,586 | 2026-09-17 | Repo description lists Hyperliquid as a supported venue. | Has a web UI (bot automation). |
| [NoFxAiOS/nofx](https://github.com/NoFxAiOS/nofx) | **AGPL-3.0** | Go | 12,922 | 2026-09-05 | README mentions Hyperliquid (verified by grepping the README). | "AI trading terminal" — UI yes. |
| [buddies2705/awesome-perp-dex](https://github.com/buddies2705/awesome-perp-dex) | MIT | Markdown | 8 | 2026-05-08 | Curated list of perp-DEX terminals/analytics/bots incl. Hyperliquid | Not an app — useful **discovery index**. |

Related but not a UI: [passivbot](https://github.com/enarjord/passivbot) (Unlicense, last push 2026-09-18, description
lists Hyperliquid) and [hyperliquid-dex/node](https://github.com/hyperliquid-dex/node) (Apache-2.0, node software).

**Bottom line for §1:** there is **no mature, well-known, permissively licensed full Hyperliquid trading UI**. The best
candidates by reuse value are `hypeterminal` (MIT, componentised monorepo) and `kerosene` (MIT, complete desktop
app); `Superior-Trade/trading-terminal` (Apache-2.0) is the most-starred but is agent/strategy-centric and depends on
TradingView Advanced Charts (see §3/§5).

---

## 2. Hyperliquid SDKs and their licenses

| SDK | License | Language | Maintenance | Order signing | Agent/API-wallet approval | WS subscriptions | Batch orders |
|---|---|---|---|---|---|---|---|
| [hyperliquid-dex/hyperliquid-python-sdk](https://github.com/hyperliquid-dex/hyperliquid-python-sdk) (official) | **MIT** ([LICENSE.md](https://github.com/hyperliquid-dex/hyperliquid-python-sdk/blob/master/LICENSE.md)) | Python | 1,831★, last push 2026-06-04, 102 open issues, not archived | Yes — `exchange.py` has `_post_action(action, signature, nonce)` + `sign` path | Yes — `Exchange.approve_agent()` (`hyperliquid/exchange.py`) | Yes — `hyperliquid/websocket_manager.py` (subscribe/unsubscribe, `send_ping`) | Yes — `bulk_orders`, `bulk_modify_orders_new`, `bulk_cancel`, `bulk_cancel_by_cloid` |
| [nktkas/hyperliquid](https://github.com/nktkas/hyperliquid) (`@nktkas/hyperliquid`) | **MIT** ([README](https://github.com/nktkas/hyperliquid#license), [LICENSE](https://github.com/nktkas/hyperliquid/blob/main/LICENSE)) | TypeScript (Node/Deno/Bun/RN) | 441★, last push 2026-09-16, npm `0.33.3` | Yes — `esm/signing/` (`_l1`, `_userSigned`, `_multiSig`, `_abstractWallet`) | Yes — `approveAgent` method present in the published package (`esm/api/exchange/_methods/approveAgent.d.ts`), plus `approveBuilderFee` | Yes — `SubscriptionClient`: `orderUpdates`, `userFills`, `userEvents`, `webData3`, `l2Book`, `trades`, `candle`, `allMids`, `activeAssetData`, `userTwapSliceFills`, … (31 subscription methods) | Yes — `order` (orders array), `batchModify`, `cancel`, `modify`, `twapOrder` |
| [ccxt/ccxt](https://github.com/ccxt/ccxt) | **MIT** ([LICENSE.txt](https://github.com/ccxt/ccxt/blob/master/LICENSE.txt)) | JS/TS, Python, PHP, C#, Go, Java, Rust | 44,027★, last push 2026-09-17 (very active) | Yes — EIP-712 `signL1Action`/phantom-agent signing in `ts/src/hyperliquid.ts` | **No `approveAgent`** (`grep -c approveAgent` = 0 in `ts/src/hyperliquid.ts`). `approveBuilderFee` exists. Agent/API-wallet trading is done by supplying `walletAddress` + API-wallet `privateKey` in options (documented by freqtrade: [docs](https://www.freqtrade.io/en/stable/exchanges/)) | Yes — `ts/src/pro/hyperliquid.ts` implements `watchOrderBook`, `watchTicker`, `watchTickers`, `watchTrades`, `watchMyTrades`, `watchOHLCV`, `watchBalance`, `watchPositions`, `watchOrders` | Yes — `has['createOrders'] = true`; `createOrders()` implemented |
| [hyperliquid-dex/hyperliquid-rust-sdk](https://github.com/hyperliquid-dex/hyperliquid-rust-sdk) (official) | **MIT** | Rust | 472★, **last push 2025-10-21** (~11 months stale), 60 open issues | Yes (signing is the core of the crate) | **[unverified]** — README is absent; I did not audit `approve_agent` in source | Yes (WS module exists) | **[unverified]** |
| [infinitefield/hypersdk](https://github.com/infinitefield/hypersdk) (community, most active Rust) | **MPL-2.0** | Rust | 216★, last push 2026-09-12 | Yes — README: "Type-safe EIP-712 signing for all operations", `PrivateKeySigner`, `client.place(&signer, BatchOrder …)` | **[unverified]** — README shows no agent-approval example; I did not audit the full API | Yes — README: "Full HyperCore API support (HTTP and WebSocket)", `ws.subscribe(Subscription::Trades/L2Book/UserEvents/ActiveAssetData)` | Yes — `BatchOrder` |
| [ControlCplusControlV/ferrofluid](https://github.com/ControlCplusControlV/ferrofluid) | MIT | Rust | 130★, last push 2026-01-19 | **[unverified]** | **[unverified]** | **[unverified]** | **[unverified]** — README is a single line ("An actually good Hyperliquid Rust SDK"); treat as a skeleton. |

Other clients (not in the question, listed for completeness, all license-verified via the GitHub API on 2026-09-18):
[sonirico/go-hyperliquid](https://github.com/sonirico/go-hyperliquid) (MIT, 122★, HTTP+WS, pushed 2026-09-15);
[Logarithm-Labs/go-hyperliquid](https://github.com/Logarithm-Labs/go-hyperliquid) (Apache-2.0, 70★);
[quiknode-labs/hyperliquid-sdk](https://github.com/quiknode-labs/hyperliquid-sdk) (MIT, Python, 291★).

### Closed-source / restrictively licensed SDK-adjacent code

- **`hyperliquid-dex/order_book_server`** — the official order-book server: **no license file** (GitHub API
  `license: null`), 161★. Unlicensed ⇒ not reusable. <https://github.com/hyperliquid-dex/order_book_server>
- **`hyperliquid-dex/ts-examples`** — official TS examples: **no license file**, 17★. Mainly a doc resource.
- **`nomed/hyperliquid`** (npm package name `hyperliquid`, 282★, TS) — GitHub reports **no license file**, while the
  npm registry metadata for `hyperliquid@1.7.7` claims **MIT** and points at a different repo
  (`github.com/nomeida/hyperliquid-api`) whose `LICENSE` returns **404**. Provenance is inconsistent ⇒ treat as
  legally unclear. ([GitHub](https://github.com/nomed/hyperliquid), [npm](https://www.npmjs.com/package/hyperliquid))
- **No closed-source SDK was found among the four SDKs you asked about** — official Python, official Rust, nktkas and
  ccxt are all MIT. The restrictive cases are apps/components, not SDKs (see §5).

---

## 3. Charting and order-book components

### 3a. TradingView Lightweight Charts vs. the full Charting Library

| Option | License | Commercial closed-source use? | Maintenance |
|---|---|---|---|
| [tradingview/lightweight-charts](https://github.com/tradingview/lightweight-charts) | **Apache-2.0** ([LICENSE](https://github.com/tradingview/lightweight-charts/blob/master/LICENSE)) | **Yes.** Apache-2.0 grants commercial use; no attribution-to-TradingView requirement beyond normal Apache-2.0 NOTICE/licence preservation, and **no ban on paywalled/private deployments**. TradingView's own page: "Lightweight Charts™ … it's open-source under the Apache 2.0 license" ([free-charting-libraries](https://www.tradingview.com/free-charting-libraries/)). | Very active: 17,296★, last push 2026-09-16 |
| **TradingView Advanced Charts** (the former "Charting Library") | **Not open source**; free under conditions | **No — this is the blocker.** [Official docs introduction](https://www.tradingview.com/charting-library-docs/latest/introduction.md): "Advanced Charts is available for free provided that the TradingView attribution remains visible **and the implementation environment is public (not for private use or behind a paywall)**." A commercial closed-source/paywalled product violates that condition. Distributed as a provided library (source not public), and [Superior-Trade's README](https://github.com/Superior-Trade/trading-terminal) notes you must obtain your own TradingView access before the chart will build. | Actively developed by TradingView (proprietary release cadence) |
| **TradingView Trading Platform** (the former "Trading Terminal") | Same free-use conditions as above | **No**, same paywall/private restriction; adds trading features ("Account Manager", multi-chart layouts, [docs](https://www.tradingview.com/charting-library-docs/latest/introduction.md)) | Active |

Difference in short: **lightweight-charts is a library you own and may use commercially; Advanced Charts / Trading
Platform are free-but-conditional products whose licence forbids private/paywalled deployments** and whose source is
not published.

### 3b. Open-source order-book / depth-chart components

| Component | License | Stack | Stars / last push | Notes |
|---|---|---|---|---|
| [0xhappyboy/orderbook](https://github.com/0xhappyboy/orderbook) (`@happyboy_/orderbook`) | Apache-2.0 | React/TS | 0★ / 2025-12-11 | npm `@happyboy_/orderbook@1.2.0` published 2025-12-11. Small, permissive, verifiable licence. |
| [kmgreg/RabbitX](https://github.com/kmgreg/RabbitX) | BSD-2-Clause | React/TS | 0★ / 2024-06-24 | Stale. |
| [dantedimon/orderflow](https://github.com/dantedimon/orderflow) | MIT | TypeScript | 1★ / 2026-07-16 | Order-flow component; low activity. |
| [bad-auth/l4book_visualizer](https://github.com/bad-auth/l4book_visualizer) | MIT | TypeScript | 13★ / 2026-02-24 | **Hyperliquid-specific** L4 order-book visualiser — good reference for HL's `l2Book`/L4 data shapes. |
| [tapedelta/kline-orderbook-chart](https://github.com/tapedelta/kline-orderbook-chart) (`kline-orderbook-chart`) | **Proprietary commercial** ("MRD CHART ENGINE — COMMERCIAL SOFTWARE LICENSE AGREEMENT … proprietary and confidential … licensed, not sold … subject to … payment of applicable fees"); GitHub reports `NOASSERTION` | Framework-agnostic (canvas, zero deps) | 38★ / 2026-07-16 | Feature-richest match (candles + order-book heatmap + footprint + liquidation heatmap + DOM ladder), but **not usable commercially without paying**. npm `kline-orderbook-chart@1.6.2`. |
| [loadshoo/depth-chart](https://github.com/loadshoo/depth-chart) (`exchange-depth-chart`) | **No license** | React + d3 | 0★ / 2026-04-20 | "A standalone React depth chart component for order book visualization" (npm description). Unlicensed ⇒ not reusable. |
| [xbc30/vue-depth-chart](https://github.com/xbc30/vue-depth-chart) (`vue-depth-chart`) | **No license** | Vue | 11★ / 2020-09-03 | Unmaintained since 2020. |
| [sp00kid/orderkit](https://github.com/sp00kid/orderkit) (`orderkit`) | **No license** | React | 0★ / 2026-03-28 | 3.1 KB gzipped, zero-dependency animated order book. Unlicensed ⇒ not reusable. |
| [kev065/binance-orderbook](https://github.com/kev065/binance-orderbook) | **No license** | React/JS | 0★ / 2024-05-01 | Simple CEX order book, stale. |

There is **no widely adopted, permissively licensed, actively maintained React order-book/depth-chart package** that I
could verify — the field is dominated by tiny 0-star repos and one commercially licensed product. In practice the
common path is to render the order book yourself from `l2Book` (see §4) with a virtualised table + d3/canvas, or use a
general charting kit.

### 3c. Trading UI component kits

| Kit | License | Stars / last push | Notes |
|---|---|---|---|
| [shadcn-ui/ui](https://github.com/shadcn-ui/ui) | MIT | 124,084★ / 2026-09-17 | Generic (not trading-specific) but the de-facto base for dense trading UIs; components are copied into your repo, which simplifies commercial closed-source use. `hypeterminal`'s UI package uses Base UI + Tailwind instead ([README](https://github.com/vipineth/hypeterminal)). |
| [tremorlabs/tremor](https://github.com/tremorlabs/tremor) | Apache-2.0 | 3,619★ / 2025-10-10 | Copy-paste React components for dashboards/charts; not trading-specific. |
| [suenot/ui-profitmaker-cc](https://github.com/suenot/ui-profitmaker-cc) | **Unlicense** (public domain) | 2★ / 2026-05-22 | "Open-source React component library for the Profitmaker.cc trading terminal" — closest thing to a real trading UI kit; tiny project. |
| [suenot/profitmaker](https://github.com/suenot/profitmaker) | **MIT + Commons Clause** (GitHub: NOASSERTION) | 364★ / 2026-08-25 | Full multi-exchange trading platform (100+ exchanges). The Commons Clause forbids selling — see §5. |
| [dhedge/ui-kit](https://github.com/dhedge/ui-kit) | MIT | 0★ / 2025-10-23 | "Trading Widget for dHEDGE vaults" — narrow scope. |
| [alejandropalo/terminal-ui-kit](https://github.com/alejandropalo/terminal-ui-kit) | **No license** (GitHub API `license: null`) | 0★ / 2026-08-25 | "Dense Canvas-first React components for trading terminals." Unlicensed ⇒ not reusable. |
| [purrdict/hip4-ui](https://github.com/purrdict/hip4-ui) | MIT | 1★ / 2026-08-29 | "Public shadcn-compatible React source registry for Hyperliquid HIP-4 prediction markets" (search-result description). **[unverified]** — I did not audit the component list or confirm it is Hyperliquid-perp relevant. |
| [@orderly.network/react](https://www.npmjs.com/package/@orderly.network/react) | npm metadata reports **ISC**; `@orderly.network/ui` npm entry has **no license field**, and I could not locate the monorepo source | npm `@orderly.network/react@1.5.17` | Orderly Network's React trading SDK/UI components ([docs](https://orderly.network/docs/sdks/react/overview)). Potentially the most complete "trading UI kit", but it targets the Orderly venue (not Hyperliquid) and its licensing is only partially verified — **[unverified]**. |

---

## 4. Hyperliquid realtime API (official docs)

### Endpoints
Source: [Websocket](https://hyperliquid.gitbook.io/hyperliquid-docs/for-developers/api/websocket)
- **Mainnet:** `wss://api.hyperliquid.xyz/ws`
- **Testnet:** `wss://api.hyperliquid-testnet.xyz/ws`

The same page documents the subscribe message format and states: "all automated users should handle disconnects from
the server side and gracefully reconnect. Disconnection from API servers may happen periodically and without
announcement. Missed data during the reconnect will be present in the snapshot ack on reconnect."
([source](https://hyperliquid.gitbook.io/hyperliquid-docs/for-developers/api/websocket))

### Subscription channels (all 24, verbatim from the docs)
Source: [Subscriptions](https://hyperliquid.gitbook.io/hyperliquid-docs/for-developers/api/websocket/subscriptions)

`allMids` (`dex` optional) · `notification` · `webData3` · `twapStates` · `clearinghouseState` · `openOrders` ·
`candle` (intervals 1m,3m,5m,15m,30m,1h,2h,4h,8h,12h,1d,3d,1w,1M) · `l2Book` (optional `nSigFigs`, `mantissa`,
`fast` → 5 levels fast / 20 levels slow) · `trades` · `orderUpdates` · `userEvents` (delivered on channel name
`"user"`) · `userFills` (optional `aggregateByTime`) · `userFundings` · `userNonFundingLedgerUpdates` ·
`activeAssetCtx` · `activeAssetData` (perps only) · `userTwapSliceFills` · `userTwapHistory` · `bbo` · `spotState` ·
`allDexsClearinghouseState` · `allDexsAssetCtxs` · `outcomeMetaUpdates` · `fastAssetCtxs` (base64 + raw-DEFLATE).

Acks: "The server will respond to successful subscriptions with a message containing the `channel` property set to
`"subscriptionResponse"`"; snapshot messages are tagged `isSnapshot: true`.
([source](https://hyperliquid.gitbook.io/hyperliquid-docs/for-developers/api/websocket/subscriptions))

### 4a. Is there a channel for a user's own order updates and fills?
**Yes, verified.** `orderUpdates` — `{ "type": "orderUpdates", "user": "<address>" }`, data format `WsOrder[]`; and
`userFills` — `{ "type": "userFills", "user": "<address>" }`, data format `WsUserFills`. Related user channels:
`userEvents`, `userFundings`, `userNonFundingLedgerUpdates`, `openOrders`, `clearinghouseState`, `activeAssetData`,
`webData3`, `userTwapSliceFills`, `userTwapHistory`, `spotState`, `allDexsClearinghouseState`.
([Subscriptions](https://hyperliquid.gitbook.io/hyperliquid-docs/for-developers/api/websocket/subscriptions))

### 4b. Does a user-specific subscription require a signature, or only the wallet address?
**Only the wallet address, per the documented message format — but the docs never say so in one explicit sentence.**
Evidence:

1. Every user-specific subscription in the official docs is a two-field message with **no signature field**, e.g.
   `{ "type": "orderUpdates", "user": "<address>" }`
   ([Subscriptions](https://hyperliquid.gitbook.io/hyperliquid-docs/for-developers/api/websocket/subscriptions)).
2. The same docs distinguish signed writes from reads: the WebSocket supports posting "either info requests or
   **signed actions**", where the signed-action path mirrors the Exchange endpoint's `action` + `signature` payload
   ([Post requests](https://hyperliquid.gitbook.io/hyperliquid-docs/for-developers/api/websocket/post-requests),
   [Exchange endpoint](https://hyperliquid.gitbook.io/hyperliquid-docs/for-developers/api/exchange-endpoint)).
3. The official Python SDK's `websocket_manager.py` sends `{"method": "subscribe", "subscription": subscription}`
   verbatim with no signature attached
   ([source](https://github.com/hyperliquid-dex/hyperliquid-python-sdk/blob/master/hyperliquid/websocket_manager.py)).

⚠️ **[unverified]** I could not find an official sentence literally stating "user-specific WebSocket subscriptions do
not require authentication". The conclusion above is an inference from the documented message schemas plus the
info-vs-signed-action distinction.

⚠️ **Related gotcha (official):** user data must be queried with the **master/sub-account address**, not the agent/API
wallet address — "A common pitfall is to use the agent wallet which leads to an empty result."
([Nonces and API wallets](https://hyperliquid.gitbook.io/hyperliquid-docs/for-developers/api/nonces-and-api-wallets),
[Info endpoint](https://hyperliquid.gitbook.io/hyperliquid-docs/for-developers/api/info-endpoint))

### 4c. Documented limits
Source: [Rate limits and user limits](https://hyperliquid.gitbook.io/hyperliquid-docs/for-developers/api/rate-limits-and-user-limits) — "The following rate limits apply per IP address":
- Maximum of **10 websocket connections**
- Maximum of **30 new websocket connections per minute**
- Maximum of **1000 websocket subscriptions**
- Maximum of **10 unique users across user-specific websocket subscriptions**
- Maximum of **2000 messages sent to Hyperliquid per minute** across all websocket connections
- Maximum of **100 simultaneous in-flight `post` messages** across all websocket connections
- REST shares an aggregated weight limit of **1200 per minute**; `l2Book`, `allMids`, `clearinghouseState`,
  `orderStatus`, `spotClearinghouseState`, `exchangeStatus` have weight 2
- Address-based limits are separate (1 request per 1 USDC traded cumulatively, 10,000-request starting buffer,
  then 1 request / 10 s; cancels get a more generous limit; default 1000 open orders, capped at 5000)

Ping/pong — source: [Timeouts and heartbeats](https://hyperliquid.gitbook.io/hyperliquid-docs/for-developers/api/websocket/timeouts-and-heartbeats):
- "The server will close any connection if it hasn't sent a message to it in the last 60 seconds."
- Keep-alive: client sends `{ "method": "ping" }`; the server responds `{ "channel": "pong" }`.

Note on `candle` history (relevant to charts): freqtrade's docs report "hyperliquid only offers 5000 historic candles"
([freqtrade docs](https://www.freqtrade.io/en/stable/exchanges/)).

---

## 5. Legal / licensing warnings for a commercial closed-source product

**Hard blockers**

1. **TradingView Advanced Charts / Trading Platform** — free only while "the implementation environment is public
   (not for private use or behind a paywall)" ([official docs](https://www.tradingview.com/charting-library-docs/latest/introduction.md)).
   Using it in a paywalled or private commercial app is outside the free grant. It is also not open source, so you
   cannot fork it. `Superior-Trade/trading-terminal` (Apache-2.0) inherits this dependency
   ([README](https://github.com/Superior-Trade/trading-terminal)).
2. **`suenot/profitmaker` — MIT *with Commons Clause*** — "the grant of rights … does not include … the right to
   **Sell the Software**", where selling includes "fees for hosting or consulting/support services related to the
   Software" ([LICENSE](https://github.com/suenot/profitmaker/blob/master/LICENSE)). Blocks a paid product/SaaS built
   on it. GitHub reports it as `NOASSERTION`, so automated licence scanners may not flag it.
3. **`kline-orderbook-chart` — proprietary commercial licence** — "proprietary and confidential. The Software is
   licensed, not sold … subject to the terms of this agreement **and payment of applicable fees**"
   ([LICENSE](https://github.com/tapedelta/kline-orderbook-chart/blob/main/LICENSE)). Not usable without buying a
   licence tier.
4. **Unlicensed repos (no licence file ⇒ all rights reserved).** Do not ship code from: `eugene-gourevitch/hypertrade-platform`,
   `jestersimpps/hyperscalper`, `laomoai/tide-hyperliquid`, `PetaTechSolution/HL-OrderBook`,
   `Jon-Becker/hyperliquid-frontend`, `liquary-xyz/liquary-xyz`, `sp00kid/orderkit`, `kev065/binance-orderbook`,
   `loadshoo/depth-chart`, `xbc30/vue-depth-chart`, `alejandropalo/terminal-ui-kit`,
   **`hyperliquid-dex/order_book_server`** (official but unlicensed), `hyperliquid-dex/ts-examples`, and
   `nomed/hyperliquid` (GitHub: no licence; npm metadata claims MIT against a repo whose LICENSE 404s — legally
   unclear). Note especially that **the official order-book server being unlicensed** rules it out for reuse.

**Copyleft — usable but with obligations**

5. **GPL-3.0:** `freqtrade/freqtrade`, `freqtrade/frequi`, `Drakkar-Software/OctoBot`. Linking/deriving a
   closed-source product from these requires releasing it under GPL-3.0.
6. **AGPL-3.0:** `NoFxAiOS/nofx` (and `Drakkar-Software/Triangular-Arbitrage`). AGPL's network clause extends the
   copyleft to users interacting with the software over a network — the strongest blocker of the copyleft set for a
   hosted terminal.
7. **MPL-2.0:** `infinitefield/hypersdk`. MPL-2.0 is file-level copyleft: you may use it in a closed-source product,
   but modifications to MPL-covered files must be made available under MPL-2.0. [standard licence property, per
   <https://opensource.org/license/mpl-2-0>]

**Safe for commercial closed-source (with normal notice/attribution duties)**

8. **MIT:** official Python SDK, official Rust SDK, `nktkas/hyperliquid`, `ccxt`, `vipineth/hypeterminal`,
   `nilesjarvis/kerosene`, `Virmage/HlOne`, `hyperliquid-dex/hyperliquid-stats-web`, `shadcn-ui/ui`,
   `bad-auth/l4book_visualizer`, `dantedimon/orderflow`, `purrdict/hip4-ui`.
9. **Apache-2.0:** `tradingview/lightweight-charts`, `Superior-Trade/trading-terminal` (subject to its TradingView
   dependency, see #1), `Co-Messi/HyperData-Terminal`, `hummingbot/hummingbot`, `hummingbot/dashboard`,
   `tremorlabs/tremor`, `0xhappyboy/orderbook`. Apache-2.0 permits commercial and closed-source use; preserve
   licence/NOTICE files.
10. **Unlicense (public domain):** `suenot/ui-profitmaker-cc`; **BSD-2-Clause:** `kmgreg/RabbitX`; **ISC (npm
    metadata):** `@orderly.network/react`.
11. **Apache-2.0 note on lightweight-charts:** the Apache-2.0 grant itself has no non-commercial or
    "public-deployment" restriction, unlike TradingView's free-but-conditional Advanced Charts. TradingView's
    marketing page does phrase lightweight-charts as being "for personal projects — it's open-source under the Apache
    2.0 license" ([free-charting-libraries](https://www.tradingview.com/free-charting-libraries/)), but the
    repository LICENSE is unmodified Apache-2.0, which permits commercial use.

**Not verified / open questions**

12. **[unverified]** I found **no Hyperliquid API terms-of-service page** in the official docs (`llms.txt` index,
    <https://hyperliquid.gitbook.io/hyperliquid-docs/llms.txt>) that restricts building a commercial third-party
    frontend. Do not read this as approval — absence of a found document is not absence of terms; check
    hyperliquid.xyz's site terms separately.
13. **[unverified]** No Hyperliquid brand/trademark usage policy was located; naming/logo use in a commercial product
    is unassessed.
14. **[unverified]** `@orderly.network/*` licensing is only partially verified (npm metadata only; the source
    monorepo and its LICENSE could not be located), so it should not be relied on until confirmed.
15. **[unverified]** `purrdict/hip4-ui`, `Virmage/HlOne`, `liquary-xyz/liquary-xyz` and the two build-your-own-client
    repos (`Jon-Becker/hyperliquid-frontend`, `smoltz29j/hl-terminal`) were not code-audited — only their metadata and
    descriptions were read.

---

## Practical recommendation (labelled as my synthesis, not a sourced fact)

Build the frontend yourself on **`@nktkas/hyperliquid` (MIT, TypeScript, covers signing + `approveAgent` + all WS
channels + batch orders)** or **ccxt (MIT, if a multi-venue abstraction matters, but it has no `approveAgent`)**,
chart with **`tradingview/lightweight-charts` (Apache-2.0)**, and use **`vipineth/hypeterminal` (MIT)** as the closest
permissively licensed prior art for an order-entry/order-book layout and for a reusable `hl-react` hook layer. Avoid
TradingView Advanced Charts, `kline-orderbook-chart`, `profitmaker`, all AGPL/GPL projects, and every unlicensed
repository listed above.
