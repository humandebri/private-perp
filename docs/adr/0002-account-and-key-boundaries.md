# ADR-0002: Account and master/agent authority separation

- Date: 2026-09-18
- Design status: accepted
- Evidence status: Not implemented or demonstrated. A UI demo is not evidence of fund safety, encryption, or governance.

## Context

Use standard HL margin and liquidation while avoiding loss sharing between users.

## Options considered

User-held master keys, subaccounts under a shared master, and independent per-user masters managed by canisters.

## Decision

Use a nontrading shared reserve and independent per-user HL trading accounts. funds_vault handles master signatures; trading_core handles per-account, per-generation agent signatures. The EOA signs authentication and withdrawal intent and does not receive the master private key. Initial payouts are restricted to the authenticated EOA’s HL account.

## Drawbacks and residual risks

Users cannot independently withdraw directly or revoke agents. Canister separation alone cannot prevent compromise by the same upgrade authority. Shared reserve assets and trading-account equity must not be double-counted.

## Reconsideration criteria

Reconsider when HL account, agent, or transfer specifications change, keys migrate, or independent recovery becomes a requirement.

## Source of verification status

Track execution results and unimplemented scope in frontend/README.md and
docs/implementation-status.md. Do not bypass the production gates in Plan.md,
section 16.
