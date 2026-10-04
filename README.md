# private-perp

An experimental Internet Computer application for confidential fund management
and Hyperliquid perpetual trading, with a TanStack Start and React frontend.

The current backend combines custody, trading, policy, and send-journal modules in
one `private_perp` canister. Personal requests use an HPKE envelope; users authenticate
with an Ethereum wallet. The frontend is built for Cloudflare Workers.

**Status: local development and Hyperliquid testnet.** Mainnet fund handling is
outside the current scope. End-to-end fund movement and trading on the public
testnet deployment remain unverified. The shared reserve reduces direct transfer
links, but the recorded synthetic baseline reached a 100% top-1 matching rate;
this project has not met its anonymity target. See the
[testnet acceptance record](docs/phase-3/testnet-acceptance.md),
[walletless validation record](docs/phase-3/walletless-validation.md), and
[privacy evaluation](docs/phase-3/privacy-local-eval.md).

## Get started

Use the Rust toolchain pinned in `rust-toolchain.toml` (including the Wasm target),
Node.js 24, Python 3, and pnpm 12.4.2. Local canister E2E additionally requires
`icp` (icp-cli), OpenSSL, and `nc`. Candid regeneration requires
`candid-extractor`, `didc`, and `ic-wasm`; generated bindings are committed.

```sh
git clone https://github.com/humandebri/private-perp.git
cd private-perp
cargo test --locked
corepack enable
pnpm --dir frontend install --frozen-lockfile
pnpm --dir frontend cf:typegen
pnpm --dir frontend dev
```

The frontend alone does not start a backend. For a complete local mock flow, use
an isolated checkout and local network, install Chromium, then run:

```sh
pnpm --dir frontend exec playwright install chromium
bash scripts/local-e2e.sh
```

This script deploys and configures a local canister, starts the mock venue, writes
`frontend/.env.local`, and runs browser E2E. It can reuse an existing local network
and modify its canister, so use a dedicated development environment. It stops the
mock process and any network it started itself when it exits.

For manual configuration and test commands, see the
[frontend guide](frontend/README.md) and [contribution guide](CONTRIBUTING.md).

## Repository map

| Path | Purpose |
| --- | --- |
| `crates/` | Rust canister modules, shared types, cryptography, and integration tests |
| `frontend/` | Wallet authentication, encrypted client transport, and trading UI |
| `candid/` | Generated Candid interfaces |
| `scripts/` | Build, local setup, API generation, and verification helpers |
| `tools/mock-hl/` | Deterministic local Hyperliquid mock |
| `tools/hl-fixture-gen/` | SDK-based signing fixture generator |
| `proofs/` | Lean models of selected safety properties; not full application verification |
| `docs/` | Architecture decisions, contracts, and implementation evidence |
| `research/` | Background research and source references |

## Architecture and evidence

Start with the [single-canister architecture](docs/phase-3/single-canister.md).
SQLite modules use separate stable-memory scopes within the same canister.
This composition shares upgrade authority across custody, trading, and journals;
it does not retain an independent journal rollback boundary or the standalone
control guard's upgrade delay.

The [requirements](Plan.md), [technical design](Implementation.md), and
[implementation roadmap](Implementation-Roadmap.md) include earlier multi-canister
plans. Consult the [implementation status](docs/implementation-status.md) and
[architecture decisions](docs/adr/) for their scope and evolution.
The [shared reserve](docs/phase-3/shared-reserve.md) and
[Lean proof guide](proofs/README.md) document further limits.

## Contributing and security

Read [CONTRIBUTING.md](CONTRIBUTING.md) before opening a pull request.
Use [SECURITY.md](SECURITY.md) to report vulnerabilities privately.
The [publication checklist](docs/oss-readiness.md) covers repository settings,
history review, and release preparation.

## License

[MIT](LICENSE). Third-party dependencies retain their own licenses.

## Manual recovery and market admission

Failed backend work stays stopped until its owner grants a single retry or result check. Unknown sends are never retransmitted. Healthy position monitoring continues with on-demand workers; deposit and trading observations have explicit refresh controls. BTC/ETH admission validates metadata, volume, spread, depth, and timestamp bounds through replicated reads. See [manual recovery](docs/phase-3/manual-retry-plan.md) and [market admission](docs/phase-3/market-consensus.md).
