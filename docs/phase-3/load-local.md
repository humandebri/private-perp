# Phase 3 fixed-seed local load test

Run date: 2026-09-25. Ran `cargo test --locked -p pocket-ic-tests --test local_load -- --nocapture --test-threads=1` with `POCKET_IC_BIN=.pocket-ic/pocket-ic` and `POCKET_IC_WASM_DIR=target/test-venue/wasm32-unknown-unknown/release`. Seed: `20260924`. Created sessions, prepared accounts, and signed eligibility tokens for 20/100 users in one vault, then exercised synthetic deposits, allocation admission, signing, V1 send intents, V2 account-creation/result events, and POSTs to mock HL. Dispatch is synchronous within PocketIC, not concurrent load on a real network.

| Users | HL POSTs | REST weight | Admission p95 (host ms) | Vault cycles | Policy cycles | Journal cycles | Failures |
|---:|---:|---:|---:|---:|---:|---:|---:|
| 20 | 20 | 20 | 22 | 210,918,030,970 | 164,860,297 | 1,713,237,846 | 0 |
| 100 | 100 | 100 | 27 | 1,055,189,790,363 | 836,744,055 | 8,580,576,068 | 0 |

Counted one weight per allocation POST and checked the V1 send-intent high-water mark against the POST count. V2 also includes account-creation events, so the check adds one unreplayed event relative to its measured high-water mark rather than assuming a fixed count. Verified that the next allocation POST did not start. Cycles are total per-canister deltas including account preparation, authentication, admission, signing, outcalls, and persistence; signing/HTTPS/storage costs are not broken down. Real testnet cycle costs, REST wait times, account observation freshness, 100-user concurrency mixing orders and recovery, and failure rates under fault injection remain unmeasured. The overall load-test acceptance criterion is therefore **not met**. This is recorded independently of the failed correlation criterion.
