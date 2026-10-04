# Local quality gate (2026-09-23)

Scope: local implementation at `617e8f9` plus this change. All checks below completed with exit code 0. This does not establish remote CI or production acceptance.

## Changes

- The first full PocketIC run failed 18 tests across seven files because real account IDs did not match. Migrated successful-path fixtures to real IDs and explicit account observations. Did not correct IDs in generic RPC helpers or relax production authorization/freshness requirements.
- Updated older fixtures subsequently exposed: inner success in cancellation replies, multiple Candid arguments, persisted signing nonces, and positions during SL/TP reconciliation now match current contracts. Also checked rejection reasons for wrong, unobserved, stale, and disallowed-asset accounts.
- The runner stops on lock acquisition failure and releases only its own lock. It propagates server preparation, build, and test failures and prints success only after tests finish. Production and test-venue outputs are separate; identical directories are rejected. CI uses the same runner and regression tests.
- Production canister code, Candid, DB schemas, and frontend behavior were unchanged.

## Results

- Rust: passed `cargo fmt --all --check`, the CI host/Wasm clippy commands (`-D warnings`), 76 host tests, `check-no-await.sh`, and `check-signing-boundary.sh`.
- PocketIC: `bash scripts/pocket-ic-test.sh --no-fail-fast`. Final version passed 85 tests across 29 files twice consecutively, with zero failures and zero ignored tests. Each run built feature-free production Wasm separately from test-venue Wasm.
- Runner: `node --test scripts/pocket-ic-runner.test.mjs`, seven passes covering success, build failure, test failure, server preparation failure, lock contention, empty server path, and duplicate output directories.
- Production Wasm: passed checks that four canister artifacts contain no test-only methods.
- Frontend: passed `pnpm lint`, `pnpm format:check`, `pnpm typecheck`, `pnpm test` (28 tests), and `pnpm build`. New runner tests were formatted and checked with existing Oxfmt configuration.
- Mock HL: `node --test tools/mock-hl/server.test.mjs`, seven passes.
- Live local E2E: `bash scripts/local-e2e.sh`, four Playwright Chromium passes with no skips, using local IC at `http://localhost:18100/` and mock HL. Verified network shutdown and removal of listeners on ports 18100, 8080, and 4173 after exit.

Unverified: remote CI, real MetaMask, real HL, testnet/mainnet, and real funds. Existing frontend bundle-size and crypto externalization warnings remain. No commit, push, or publication was performed.
