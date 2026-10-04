# private-perp UI

A TanStack Start and React development UI connecting to a local ICP replica and
Hyperliquid mock. It includes public market data, positions, stop-loss/take-profit,
closing and cancelling orders, and owner-controlled recovery of stopped work.

The [public HL testnet UI](https://private-perp-ui-testnet.hude.workers.dev) connects
to the unified IC canister `xis3j-paaaa-aaaai-axumq-cai`. Signed login and encrypted
API calls have been checked on that deployment; live HL orders and the complete
fund round trip remain unverified. See the
[single-canister design](../docs/phase-3/single-canister.md).

## Setup and local E2E

Use Node.js 24 and pnpm 12.4.2. From the repository root:

```sh
corepack enable
pnpm --dir frontend install --frozen-lockfile
pnpm --dir frontend cf:typegen
pnpm --dir frontend exec playwright install chromium
bash scripts/local-e2e.sh
```

The E2E script starts mock HL and a local IC network, deploys and bootstraps the
unified canister, writes `frontend/.env.local`, builds the frontend, and runs
Playwright. On exit it stops its child mock process and the IC network if it
started that network. Use an isolated local network: the script can reuse and
modify an existing deployment. Playwright injects the test-only `e2e-signer` as an
EIP-1193 provider instead of embedding a fixed signing key in product code.

For manual setup, copy `.env.example` to `.env.local` and set
`VITE_PRIVATE_PERP_CANISTER_ID` after deployment. `VITE_APP_STAGE=local`, the IC
host, and mock HTTP/WS URLs are required; non-loopback hosts are rejected in local
mode. Start the UI with `pnpm dev` from this directory. All `VITE_` variables are
public browser configuration and must not contain secrets.

To use the mock admin API from another frontend port, set `MOCK_HL_ADMIN_ORIGINS`
to a comma-separated list of exact origins when starting the mock. Defaults are
`http://127.0.0.1:4173` and `http://127.0.0.1:5173`.

## Supported flows

- EOA authentication through MetaMask `eth_requestAccounts` and
  `eth_signTypedData_v4`.
- Short-lived Ed25519 identity, session handle, and HPKE private key held only in
  page memory.
- Mock deposits into a shared reserve, later allocation to the user's trading
  account, agent creation and approval, market/limit orders, cancellation,
  automatic recovery of withdrawal shortfalls, signed withdrawals, and fund,
  order, and fill history. Deposit and allocation are separate actions; balance
  adjustment returns trading funds to reserve balance.
- Deposits originate from the authenticated EOA's HL account. Resistance to
  amount/time correlation is incomplete; see the
  [shared reserve scope](../docs/phase-3/shared-reserve.md).
- Personal queries and cancellations use a centralized Candid envelope codec.
  Request-ID reuse and decryption failures do not trigger automatic retries.
- Deterministic LOCAL MOCK execution: market orders fill immediately, limit
  orders rest, and admin endpoints bind to loopback only.

## Verification

Run from `frontend/`:

```sh
pnpm lint
pnpm format:check
pnpm typecheck
pnpm test
pnpm build
pnpm test:e2e                    # Shell/CSP; live canister cases are skipped
bash ../scripts/local-e2e.sh     # Full local canister flow
node --test ../tools/mock-hl/server.test.mjs
```

Default Playwright tests use Workers preview at `127.0.0.1:4173` after a build.
Only the local integration E2E requires a replica and mock venue.

## Testnet configuration

Copy `.env.testnet.example` to `.env.testnet.local` and set the dedicated testnet
canister ID. `pnpm build --mode testnet` selects `wrangler.testnet.jsonc`.
This builds the frontend; publishing it requires a separate deployment step.
Testnet mode uses real HL testnet endpoints and does not run mock deposit actions.
Mainnet fund acceptance is outside the current scope.

## Safety and behavior boundaries

- Workers permit only loopback access in local mode and public GET/HEAD in testnet
  mode. CSP connections are restricted to configured IC and market hosts.
- Charts fetch 24 hours of BTC/ETH one-minute candles from testnet
  `candleSnapshot` and update through the same testnet WebSocket. Timestamps use
  UTC. Fetch errors are shown without fabricating replacement candles. Local
  mode uses the local mock.
- Orders accept Long/Short, USDC margin or asset quantity, and 1–5× leverage.
  Market price limits use the latest mid and slippage, rounded to asset precision.
  Estimates exclude fees and funding. Available funds conservatively subtract
  used margin and order reservations from equity.
- Five-minute, fifteen-minute, and hourly candles aggregate the fetched minute
  data; the initial view shows the latest 120 bars. Volume, the user's entry price,
  and SL/TP are shown. On mobile the chart comes first and a fixed Trade button
  opens the same order form. Deposits, allocations, and orders require explicit
  user actions.

- Personal data is not passed to SSR/server functions. Keys and sessions are not
  stored in localStorage, sessionStorage, or cookies.
- Unknown outcomes remain visible and are not retried automatically. Stale
  snapshots, unapproved agents, or unobserved accounts block new orders.
- Snapshot age includes elapsed time after receipt. Age over ten seconds or a
  failed required fetch blocks new orders; cancellation and reduce-only operations
  are handled separately.
- A lost order acknowledgement is reconciled through HPKE
  `get_order_by_request`. An unobserved result does not establish failure or permit
  new orders to resume. HTTP automatic retries are disabled.
- Logout or expiry immediately invalidates the session generation and discards
  account data, additional history pages, and unresolved requests. Old responses
  cannot affect a new session. Unresolved requests are not restored after reload;
  reloading is not a reconciliation mechanism.
- Mock seed operations are labeled `LOCAL MOCK` in both the UI and responses.
  They are test tools rather than normal user or production API features.

## Explicit refresh and recovery

Use **Confirm deposit** to reconcile one shared-reserve history page and **Refresh trading information** to request fresh account and market observations. Ordinary display polling does not resume failed work. The **Stopped work and manual checks** panel lists owner-specific stopped operations and grants one retry or observation, using the displayed generation. Unknown sends are checked without retransmission. See the [manual recovery contract](../docs/phase-3/manual-retry-plan.md). The minimal fallback route and administrator assignment of unmatched deposits have been removed. Historical journal decoding and evidence-bound Spot ownership claims remain supported.
