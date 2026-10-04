# Predeployment validation (2026-09-29)

**Historical record:** these results concern working changes on `feat/single-canister-testnet` at `cd795ce5ff041550d24e9acecd03410940f42cdb`. They do not establish the results of the later integration with main. No public IC or Cloudflare deployment occurred.

## Completed checks

| Scope | Result |
| --- | --- |
| Rust host workspace, excluding Wasm-only crates and PocketIC | 81 passed, none ignored |
| Full PocketIC suite | Initially 144 passed and four failed. After fixing one market-monitor, two load, and one Vault-upgrade failures, the complete affected targets passed on rerun. 148 passing tests accounted for, none ignored |
| Production unified Wasm in PocketIC | One `single_canister` and four `market_consensus` tests passed |
| Frontend unit tests | 47 passed |
| Browser E2E, local real canister and mock HL | Four passed, none skipped. Covered authentication, deposits, custody, allocation, agents, orders, SL/TP, cancellation, closing, recovery, withdrawal, lost responses, logout, over 110 history entries, and account separation |
| Real HL testnet read smoke | One passed using local IC: testnet authentication, account preparation, eligibility registration, and real `/info` balance reads. No `/exchange` POST |
| Cloudflare prepublication check | Testnet build with the specified canister ID, followed by successful `wrangler deploy --dry-run` against generated `dist/server/wrangler.json`. `APP_STAGE=testnet`, `IC_HOST=https://icp-api.io`, gzip 421.32 KiB. No upload |
| Node tests | Seven PocketIC runner and eight mock-HL tests passed |
| Rust static checks | Formatting, host/all-target and standalone/unified Wasm Clippy, no-await and signing-boundary checks passed |
| Frontend static checks and build | Lint, formatting, TypeScript and production build passed |
| Candid/public API | Unified 112-endpoint contract matched. Extracted interfaces from six production Wasm files; synchronized only Vault comments/order. No test-only exports |
| UI | vlmkit 0.22.0: three-viewport trade integrity CLEAN; scroll, handlers, interactions, and breakpoints passed with warnings |

PocketIC 16 was used. Production, standalone mock, and unified mock Wasm were built in separate targets. The full run was `RUST_TEST_THREADS=1 cargo test --locked -p pocket-ic-tests --no-fail-fast`; reruns covered `--test market_monitor`, `--test mixed_load`, and `--test vault_upgrade`. Additional production checks used `--test single_canister --test market_consensus` without `PRIVATE_PERP_UNIFIED_MOCK`. Rust/frontend CI checks were also run locally.

Production unified Wasm SHA-256: `24bed4e14713cfd3b466e74c43d340abcbab4409f7e2070ae23d49184b79c74c`. Unified mock SHA-256: `c8f6c6ca018f6fb0074094e1ad2f87b9e254c3f1ac540c8bcaac1f3fb235d6bd`.

## Fixes made during validation

- The local deposit control previously waited after mock deposit without reconciling. It now calls `confirmDeposit` once from the owner's action. A unit test prevents automatic retry of deposit, confirmation, or allocation after reconciliation failure.
- The handwritten encrypted-response decoder lacked `JournalWriterBusy`. Added it and verified decoding into a normal `CanisterError`. TypeScript checks coverage against the generated API variants to detect future omissions.
- Updated PocketIC runner build-count and target expectations for unified Wasm.
- Market checks fetch metadata and book per market context: four requests and 44 weight. Added coin/time to book fixtures.
- Mixed-load tests cover allocation, orders, cancellation, and recovery for 20/100 users. The 1,200 limit and 300 exit reserve remain; when use exceeds 300, advance 61 seconds before the next workflow and explicitly refresh market data. This verifies sequential workflows within rate limits, not simultaneous acceptance performance for 100 users.
- Vault-upgrade tests verify stopped state and owner-authorized history observation after upgrade without reposting uncertain transfers.
- Browser steps explicitly refresh trading information before initial orders, when returning to the screen, and after network recovery. Wait for both SL/TP orders to become Open and withdrawals to become Settled. Fund history lists requests, so the fixture creates 110 actual allocation requests. Only the fixture handles `JournalWriterBusy` using the same request ID to test pagination and account separation; no product automatic retry was added.

## Limits

Only the lost-leverage-callback snapshot test disables PocketIC install rate limiting, allowing upgrade immediately after restoration. It is not evidence of production budget or performance behavior.

UI warnings remain for disabled unauthenticated controls and unexecuted chart/other events. These checks do not certify the entire authenticated screen. Skill version 0.23.0 was unpublished when checked, so installed 0.22.0 was used.

Build warnings concern large bundles and Node `crypto` externalization in HPKE dependencies. Encrypted communication was separately exercised through the actual browser flow.

These local results establish neither public-canister cycle sufficiency, real signed HL transfers/fund round trips, nor public UI connectivity.

## Reproduction and cleanup

- Local E2E used `PATH=/private/tmp/private-perp-test-tools/bin:$PATH bash scripts/local-e2e.sh`, temporary candid-extractor 0.1.6, and a nonanonymous test identity within project-specific `ICP_HOME`. The final run passed four tests in 3.2 minutes. Overall timeout was increased to 600 seconds while individual operation/state limits remained.
- Testnet reads used the same isolated `ICP_HOME`, a fresh local network and installed unified canister. Its local ID was passed as `TESTNET_APP_ID` to `scripts/prepare-local-testnet.sh`. Only dedicated test keys were used; public canisters were untouched.
- UI used `VITE_APP_STAGE=testnet`, `IC_HOST=https://icp-api.io`, `PRIVATE_PERP_CANISTER_ID=xis3j-paaaa-aaaai-axumq-cai`, and `MARKET_WS_URL=wss://api.hyperliquid-testnet.xyz/ws` through the corresponding `VITE_*` variables for `pnpm --dir frontend build --mode testnet`, followed by `pnpm --dir frontend exec wrangler deploy --dry-run --config dist/server/wrangler.json --outdir /private/tmp/private-perp-cf-dryrun`.
- Removed the newly created `frontend/.env.local` and temporary seed. Script cleanup stopped local network and mock services. Ignored build artifacts and the local test identity remained reusable.

Logs were saved under `/private/tmp/private-perp-{host-tests,full-pocketic,production-pocketic,load-retest,upgrade-retest,local-e2e,testnet-smoke,testnet-build,cf-dryrun}.log`. Read the initial full-suite log containing four failures together with successful reruns. Validation concerned the modified working tree; no commit, push, or public deployment occurred at that point.

## GitHub Actions status on 2026-09-29

[PR #2](https://github.com/humandebri/private-perp/pull/2) showed failed Rust, PocketIC, and Frontend jobs, but GitHub reported that jobs could not start because of failed payment or usage limits. No test steps ran, so CI had not judged the code. The same revision needed rerunning after resolving that account issue. This status is distinct from the historical local checks above and from any subsequent integrated-PR CI result.
