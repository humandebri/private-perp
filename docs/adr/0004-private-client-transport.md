# ADR-0004: Personal-data transport boundary

- Date: 2026-09-18
- Design status: accepted
- Evidence status: Not implemented or demonstrated. A UI demo is not evidence of fund safety, encryption, or governance.

## Context

Protect the mapping between users and trading accounts separately from public HL account information.

## Options considered

Direct browser subscriptions to personal accounts, relaying through Workers, and retrieval through confidential canisters.

## Decision

Connect directly to HL only for public market data. Personal data communicates directly with canisters through authentication and HPKE encryption. Do not send plaintext orders, personal balances, or wallet signatures to Workers. Do not persist session keys or personal caches in the browser.

## Drawbacks and residual risks

Polling and reconciliation increase latency and cost. This does not eliminate observation by HL itself or at transport entry points. Encryption alone cannot prevent frontend tampering.

## Reconsideration criteria

Reconsider if reconciliation budget or latency misses its targets. Evaluate improvements while retaining the protection boundary; adding a relay requires approval in a separate ADR.

## Source of verification status

Track execution results and unimplemented scope in frontend/README.md and
docs/implementation-status.md. Do not bypass the production gates in Plan.md,
section 16.
