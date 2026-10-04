# Local M5/M6 verification record

- Scope: loopback ICP replica, synthetic USDC, and deterministic mock HL. These are not testnet or real-fund performance measurements.
- Trading: verify market positions, limit cancellation, SL/TP, partial/full closing, and Cancel All through real canisters.
- Faults: reproduce partial/reject/unknown with one-shot mock scenarios and stale state by stopping info responses. Unknown requests are not retried automatically.
- Separation: do not send EOA or trading-account identities to the public market WebSocket; retrieve personal state only through HPKE-enveloped canister APIs.
- UX target: local pending state within 100 ms of an order click. CI acceptance checks functional regression; device-dependent p50/p95 measurements will be added during testnet measurement.
- Unmeasured: IC testnet signing latency, HL testnet acceptance latency, timer cycles, and real HL rate limits. Record these after GATE 0.
