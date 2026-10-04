# Validation without the owner's wallet

Checked on 2026-09-30. The deposit owner's wallet private key was not used. Preserved the funded IC gateway at `127.0.0.1:18100` and shared account. UI flows ran on a separate gateway at `127.0.0.1:18101` with a dedicated administrator, test wallet, and mock HL.

## Passed checks

| Scope | Result |
| --- | --- |
| Full PocketIC integration suite | 157 passes covering authentication, multiple users, funds, orders, cancellation, closing, HTTP outcalls, journals, recovery, upgrades, and 20/100-user load. |
| Rust host tests | 82 passes covering cryptography, signatures, independent SDK fixtures, API types, and pure logic. |
| Rust lint and boundary checks | Host all-targets, standalone/unified canister Wasm all-targets, no-async, and raw-key signing boundaries passed. |
| Frontend unit tests | 46 passes, including Candid decoding of the added writer-contention error. |
| Frontend static checks | Type-aware lint, typechecking, and formatting passed on macOS. |
| Mock HL and PocketIC runner | 17 passes, including explicit fees and exit-code propagation for build/test failures. |
| UI integration E2E | Four passes using dedicated local IC and mock HL: login, deposit, agents, orders, cancellation, closing, recovery, withdrawal, history pagination, logout, mobile view, and HTTP headers. |
| UI rendering and interaction | vlmkit 0.22.0 integrity, horizontal-scroll, handler, and keyboard-interaction checks reported zero suspects. Handler/interaction checks had warnings for noninteractive elements and other cases; they do not guarantee completeness of every interaction. Skill-specified 0.23.0 was not published in the registry. |

## Fixes made during testing

- Anonymous callers in a fresh IC identity store cannot administer canisters, so E2E startup creates a dedicated test administrator. Startup refuses to switch an existing HL testnet environment to the mock and stops only networks it started itself.
- Updated PocketIC runner checks for separate production/mock builds of standalone and unified Wasm.
- Added explicit `fee: "0"` to mock `internalTransfer` responses to match strict receipt-amount reconciliation. Deduction of actual fees is checked separately in PocketIC.
- Added `JournalWriterBusy` to the frontend envelope response codec. Tested an error encoded from generated Candid contracts to ensure it becomes an ordinary canister error rather than a decode error. Only the history-pagination E2E fixture waits for pre-admission writer contention. This does not add automatic retries for production fund transfers or orders with unknown outcomes.

## Linux environment

In an Apple container with Node 24 on Linux amd64, frozen-lockfile installation, Cloudflare type generation, formatting, typechecking (exit code 0), 46 current-code unit tests, build, and three public-shell Chromium E2E tests passed. The one IC integration test was not run in the Linux container; it passed against the isolated macOS gateway.

Linux type-aware lint ends with `SIGKILL` in the `oxlint-tsgolint` subprocess. Attempts on ARM64/amd64, with 4/8 GB and varying Go memory settings, did not resolve it. macOS type-aware lint and typechecking pass, but this is not full Linux CI acceptance.

## Remaining live-environment checks

- Sign in with the deposit owner's wallet and confirm the deposited 10 USDC balance.
- Verify real HL orders, cancellation or closing, and the allocation/recovery/payout round trip.
- Measure real public ICP cycle costs and verify external HTTP response agreement.

Passing mock flows does not demonstrate real HL trading, recovery, or public deployment. Logs, UI gate JSON, traces, and validation copies are stored outside Git in `.icp-home/hl-testnet/walletless-validation/` and `target/ui-e2e-snapshot/`.
