# Environment separation: local, testnet, and mainnet

- Basis: `Plan.md` 16.1, 16.3, 16.4, and 16.5; `Implementation.md` 2.1, 3.1, and 14.3; `ADR-0006`.
- Status: design contract. **Canister IDs, signing key IDs, and real-fund limits remain unresolved** at this stage. Do not invent values.

> Historical environment contract. The matrix and measurements record the original phase; current deployment instructions are in the root README and later testnet reports.

## 1. Purpose and principles

1. Separate four dimensions: network, keys, endpoints, and eligibility issuer. A single configuration mistake must not let mock behavior reach production.
2. Keep secrets (PEM, seeds, identities, and `.icp/data`) out of the repository using `.gitignore`.
3. Record unresolved values as unresolved and add confirmed values when available. Do not put guesses in configuration.
4. Irreversible production actions (controller removal, SNS launch, and accepting real funds) are outside Phase 0.

## 2. Environment matrix

| Item | local | testnet | mainnet |
|---|---|---|---|
| IC network | Local `icp network start` network (PocketIC based) | IC verification testnet, distinct from `ic` in this design | IC mainnet |
| Canister ID | Dynamically assigned by `icp deploy`; stored in `.icp/data`, **not committed** | **Unresolved**; record after allocation in Phase 1 | **Unresolved**; record before production |
| tECDSA key ID | Local test key, default `test_key_1` | Test key on `fuqsr`; name **unverified**. First candidate: `test_key_1`; override via `set_ecdsa_key_id` | `key_1` on `pzp6e` (34 nodes); rejected in Phase 2 |
| Signing subnet | Local replica | `fuqsr` | `pzp6e` (fiduciary signing subnet) |
| HL REST | Local mock HL | `https://api.hyperliquid-testnet.xyz` | `https://api.hyperliquid.xyz` |
| HL WS, direct browser connection | Mock or disconnected | `wss://api.hyperliquid-testnet.xyz/ws` | `wss://api.hyperliquid.xyz/ws` |
| Assets | Synthetic | Test USDC on HyperCore | USDC on HyperCore |
| Builder fee | 0 | 0 | **Unresolved** business decision; no implicit charge |
| Eligibility issuer | Mock, synthetic attributes | Mock, synthetic attributes | **Unresolved**; register after contractual/legal review |
| Developer controller | Allowed | Allowed | Remove for the production target; separate Phase 4 approval |
| Frontend `APP_STAGE` | `demo` | `demo` | Unset; non-demo requests return 503 in this original contract |
| Real funds | None | Test USDC only | **Outside Phase 0** |

- `key_1` and `pzp6e` come from `Implementation.md` 2.1 and were not reverified for this contract. Confirm through Phase 1 measurements.
- Resolve asset indices from `meta.universe`; do not embed fixed values. Measurements on 2026-09-19 found BTC=3 and ETH=4 on testnet, versus BTC=0 and ETH=1 on mainnet (`docs/phase-1/README.md` section 6).
- Local integration tests use PocketIC server 16.0.0 in `.pocket-ic/`, a separate harness from `icp network start`.
- Local threshold ECDSA requires PocketIC's **test threshold-key subnet**. `PocketIc::new()` has no keys and rejects requests with `existing keys: []`. Use `PocketIcBuilder::new().with_application_subnet().with_test_threshold_keys_subnet().build()` (`crates/pocket-ic-tests/src/lib.rs`).
- The local key ID is **`test_key_1`**, measured on 2026-09-19. Production uses `key_1` on `pzp6e`. The measured PocketIC signing round trip was about 17.9ms; this is not testnet or production subnet performance.
- Reverify production signing keys and subnets in the Phase 4 release candidate.
- Builder-fee caps and recipients require explicit agreement. `approveBuilderFee` needs a separate master signature from agent approval in `api-contract.md`.

## 3. Keys and derivation paths

Rules from `Plan.md` 16.1:

- tECDSA keys are bound to canister ID, derivation path, and key ID. Assume malicious replacement code in the same canister can also sign.
- `funds_vault` manages master keys for the shared reserve and individual trading accounts. Each trading account has an independent master; it is not an HL sub-account under one shared master.
- `trading_core` manages only per-account, per-generation agent keys.
- Paths use a cryptographically random 32-byte `account_id` and `generation`. **Do not embed EOA, Principal, or `user_id` in public paths or cloids.**
- Proposed path names, to be finalized in Phase 1:

| Purpose | Proposed path |
|---|---|
| Shared reserve master | `["private-perp", "vault", "reserve"]` |
| Individual trading master | `["private-perp", "trading", account_id_hex]` |
| Agent-generation key | `["private-perp", "agent", account_id_hex, generation]` |

## 4. Prevent mock settings from reaching production

Required Phase 1 tests based on `Plan.md` 16.5 and `ADR-0006`:

| ID | Separation test | Acceptance condition | Status |
|---|---|---|---|
| E-1 | Present a mock eligibility token with production-equivalent network, keys, and build settings | Rejected | **Not run**; eligibility issuance belongs to Phase 3 and no token exists yet |
| E-2 | Configure a mainnet endpoint under testnet settings | Startup/admission rejects it or fails explicitly | **Implemented and verified** with hl-types host tests, `core_environment.rs`, and `vault_environment.rs` |
| E-3 | Change HPKE AAD network, canister, method, caller, request ID, or exceed expiry | All rejected | |
| E-4 | Reuse a session/challenge from another environment | Rejected with `NetworkMismatch` or `OriginMismatch` | |
| E-5 | Include mock HL endpoint settings in a production build | Detected; cannot start unchanged | |
| E-6 | Serve a non-public response with `APP_STAGE` other than `demo` | 503; no personal data | |

Use startup-verifiable values (network, canister, key ID, and endpoint), not build settings alone, to identify the environment. Mark mock tokens, issuers, and endpoints as development settings and do not copy them to production templates.

### 4.1 Implemented startup configuration

- Store values in `funds_vault.vault_config` and `trading_core.core_config`, rather than build constants. Only controllers may change them. Vault setters: `set_network`, `set_venue_endpoints`, `set_ecdsa_key_id`. Core setters: `set_market_context` (network/dex), `set_venue_endpoints`, `set_ecdsa_key_id`. Public `get_environment` diagnostics contain no secrets.
- Unset network defaults to `local`; endpoints derive from it. **Phase 2 rejects mainnet** with `mainnet_not_enabled`. Accept only hosts matching the network: testnet uses `api.hyperliquid-testnet.xyz`, local uses loopback. **Local configuration rejects real venue hosts.**
- Default local/testnet key ID is `test_key_1`; override the verified testnet name through `set_ecdsa_key_id`. Mainnet `key_1` is unreachable in Phase 2.
- Centralize validation in pure `hl-types::environment` host tests: mainnet rejection, host mismatch, lookalike domains, and key-ID format.
- E-5 is addressed structurally by keeping mock endpoints out of code and supplying them only through configuration. Local defaults are loopback and cannot reach the real venue.

## 5. Configuration sources

| Configuration | Source | Committed? |
|---|---|---|
| Canister build/deployment | `icp.yaml`, added in Phase 0; environments added in Phase 1 | Yes |
| Canister ID | icp-managed `.icp/` data; `icp canister status <name> -i` | No; `.icp/` ignored |
| Identity/PEM | `icp identity` | No |
| Network/root key | `icp network status --json` | No |
| Environment-specific constants such as endpoints | `icp.yaml` environments or canister environment variables | Yes, without secrets |
| Runtime network, endpoints, and key ID | Controller-only canister setters; inspect with `get_environment` | No; set at deployment |
| Production fund limits and fees | Confirmed business decisions | Yes, after confirmation |

Use the local root key only in an explicitly selected local environment, following icp-cli's principle. Pass inter-canister IDs through icp-cli-injected `PUBLIC_CANISTER_ID:<name>` environment variables rather than hardcoding them.

## 6. Unresolved values

| Item | Record in | Confirmation stage |
|---|---|---|
| Testnet/mainnet canister IDs | This table | After allocation in Phase 1 / before production |
| Test key ID and subnet | This table | Phase 1 |
| HL testnet limits, minimum amounts, fees, and final events | `money-and-units.md` section 9 | Phase 1 |
| Production total custody, user, and trading limits | This table | Phases 4–5 |
| Production external eligibility issuer | This table | After contractual/legal review |
| `icp.yaml` environments syntax | `icp.yaml` | Phase 1; verify with `icp project show` |
