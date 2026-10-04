# ADR-0006: Separate Workers delivery from ICP processing

- Date: 2026-09-18
- Design status: accepted
- Evidence status: UI infrastructure and demo are subject to local verification; production is unverified.

## Context

Simplify delivery and public SSR while keeping confidentiality and fund management on ICP.

## Options considered

An ICP asset canister, Workers with Static Assets, and a custom Node.js server.

## Decision

Use Workers with Static Assets as the primary delivery layer. SSR public pages and render trading, funds, and history on the client. Do not move the fund database, order API, or signing to Workers. Do not initially add Express/Hono/D1/KV/R2/DO or a custom WebSocket relay. Entry-point geographic restrictions do not replace canister eligibility checks.

## Drawbacks and residual risks

Cloudflare and delivery administrators can alter executed JavaScript. ICP’s seven-day delay does not constrain delivery changes. Initially permit only GET/HEAD in demo mode and return 503 in live mode. Use only CF metadata for country classification, allowing absent metadata for local demos. Production country/VPN/screening checks and token issuance are unimplemented.

## Reconsideration criteria

Reconsider when introducing primary ICP delivery, additional server features, or production eligibility issuance. Complete authority separation, review, artifact verification, and dependency auditing before production.

## Source of verification status

Track execution results and unimplemented scope in frontend/README.md and
docs/implementation-status.md. Do not bypass the production gates in Plan.md,
section 16.
