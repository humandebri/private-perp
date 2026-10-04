# Local canisters with HL testnet

Checked on 2026-09-26. These results describe the former five-canister topology, not verification of the current unified `private_perp` Wasm. The IC network was local; only the external venue was real HL testnet. No public ICP deployment or real cycle-cost measurement was performed.

## Results from this check

- Deployed five local canisters without production feature overrides, set vault/core network to `testnet`, and configured `https://api.hyperliquid-testnet.xyz` as the HL endpoint.
- Retrieved BTC index 3 / szDecimals 5 and ETH index 4 / szDecimals 4 from real `meta`. Mock HL indices were not reused.
- Real `metaAndAssetCtxs` volume had more than six decimal places, causing the existing parser to reject market observations. Truncated market volume only using integer arithmetic while retaining strict precision checks for deposits, withdrawals, prices, and quantities. After the fix, confirmed BTC/ETH observations and new-risk permission through the canister.
- Used a random test-only EOA key to verify challenge authentication, trading/reserve account creation through HPKE, synthetic eligibility signature registration, and testnet deposit instructions. The observed reserve balance on HL was zero. No order or fund-transfer `/exchange` POST was performed.
- Reserve deposit address: `0xf9b2b86555bde4bd83ce5b5590ae56d0fd9d1a0f`, derived from this local state and for **HL testnet only**. Check the current value in the untracked `.icp-home/hl-testnet/public.json`. Resetting network state does not guarantee continued control of the same account; preserve state during round-trip transfers.
- `recovery_history_verified` remains false. Completeness of real HL history is unverified.
- Passed one integration smoke test, one PocketIC market-monitoring test with real-HL-style volume, 33 frontend unit tests, typechecking, lint, and Rust format checking. Native trading-core unit tests could not run because SQLite is Wasm-only; PocketIC checked the canister parser path.

## Re-run

Start an empty dedicated local network, deploy the unified Wasm with an administrator principal, and explicitly provide the installed ID:

```sh
export TESTNET_APP_ID="$(icp canister status private_perp --json | python3 -c 'import json,sys; print(json.load(sys.stdin)["id"])')"
bash scripts/prepare-local-testnet.sh
```

The script uses this repository's `.icp-home` and saves mock issuer and test EOA private keys there with mode 0600. It does not display keys, add them to Git, or embed them in the frontend. It does not use the known E2E key. IC connections are loopback-only and HL connections are testnet-only. Real testnet account preparation is isolated from ordinary unit tests.

This check uses a dedicated CLI/HPKE client, not mock deposit seeding. `bootstrap-local.sh` defaults to the mock and connects to real HL only with explicit `HL_NETWORK=testnet`. Do not run mock bootstrap against testnet-configured state. There is no recorded re-run of the current unified-canister script yet.

## Remaining work

After depositing test USDC, confirm the owner's credited balance through canister history reconciliation. Then perform a small allocation, agent approval, an order, cancellation or closing, recovery, and withdrawal, comparing HL history and balances. Real HL signature acceptance, transfer acceptance, recovery from unknown outcomes, and two-account separation are not included in the current pass results.
