# Single-canister composition

The HL testnet backend deploys as one `private_perp` canister. The UI connects to a single ID from Cloudflare Workers.

## Implementation and authority

`crates/private-perp` links funds, orders, policy, and journal modules into one Wasm. It does not link the guard. Each SQLite database occupies a separate stable-memory slot; async processing selects its storage scope on every poll. Conflicting public method names are prefixed with `vault_`, `core_`, `policy_`, or `journal_`.

Internal principals are fixed to the canister itself at installation, with no mutation API. Authentication, policy, and journal self-calls retain message boundaries. Journal `vault` and `core` streams remain logically separate; role-scoped entry points accept only self-calls.

The administrator specified by `init(administrator : principal)` is persisted. Policy, REST budget, cycle settings, market thresholds, eligibility settings, and resuming journal reconciliation require that administrator. Anonymous, management-canister, and self principals are rejected. Controllers do not automatically receive application administration rights. Existing controller operations for keys, environment, migration settings, and IC upgrades remain. There is no administrator-change API.

After upgrades, vault/core dispatch is paused until the designated administrator reconciles each journal and resumes it. One upgrade authority covers fund keys, order keys, and business records; there is no independent-canister journal rollback-detection guarantee or guard-enforced upgrade delay.

## Build and verify

Put `candid-extractor`, `didc`, and `ic-wasm` on `PATH`.

```sh
bash scripts/extract-candid.sh
bash scripts/generate-frontend-bindings.sh
icp build private_perp
bash scripts/test-single-canister.sh
```

Candid is generated from the source modules and compared with production Wasm query/update exports. The unified API has 121 methods. The init argument in `candid/private_perp.did` and UI bindings are synchronized.

PocketIC checks administrator/nonadministrator authorization, signed login, HPKE, account creation, eligibility registration, insufficient balances, allocation idempotency and over-allocation rejection, journal self-calls, upgrade persistence, and reconciliation resumption. Test-only Wasm additionally checks three signed allocation sends, mock receipts, agent approval, and order dispatch through the unified path. HL replies are mocked and do not establish real testnet acceptance. Test-only Wasm uses a separate target; production omits mock deposit entry points.

## Local and public deployments

For a fresh local deployment, use `icp deploy private_perp --args '(principal "ADMIN_PRINCIPAL")'`, setting the bootstrap identity as administrator. `scripts/local-e2e.sh` sets that argument and the unified ID. Do not run it on a shared existing local network. After composition, browser E2E verified login, deposit, allocation, orders, recovery, and withdrawal on an isolated local network.

The UI uses only `VITE_PRIVATE_PERP_CANISTER_ID`. The public test example is `frontend/.env.testnet.example`. Save settings in `.env.testnet.local`; `pnpm --dir frontend build --mode testnet` selects `wrangler.testnet.jsonc`. Testnet screens show HL testnet transfer sources/destinations and do not execute mock deposits.

Public canister: `xis3j-paaaa-aaaai-axumq-cai`. Administrator: `r75h6-lqd7b-5jack-at55d-vvti2-lg5qy-ly73a-5ezve-odnkc-kagu3-nae`. UI: https://private-perp-ui-testnet.hude.workers.dev . Deployed on 2026-09-30 with HL testnet connectivity, markets, eligibility, and HPKE configured. Signed login, encrypted APIs, account derivation, and balance queries were checked on public IC. Real deposits, allocations, orders, recovery, and withdrawals on the public deployment remain unverified.

The public shared reserve is `0xc7ed130680612632b22ff0b23f9350977a33cd70`, separate from local `0x793dcb0dc098ff33c5aa8a115ee142a7b6b0ee1d`. Public deployment does not move the local 10 test USDC. Confirm the connected wallet and destination in UI deposit instructions. Public new orders and allocations also require eligibility registration, tied to the login IC principal. HTTP queries temporarily deduct cycles, so execution needs additional headroom.
