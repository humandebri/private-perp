# ADR-0001: Canister custody

- Date: 2026-09-18
- Design status: accepted
- Evidence status: Not implemented or demonstrated. A UI demo is not evidence of fund safety, encryption, or governance.

## Context

Reduce the link between everyday wallets and HL accounts. Agent-only operation leaves the deposit path public and cannot independently meet this goal.

## Options considered

Agent-only operation, a confidential fund layer with per-user HL accounts, and internal position management within a single HL account.

## Decision

Adopt a confidential fund layer that manages customer funds. Separate the operating budget from customer funds. Include double-entry accounting, owner authorization, withdrawals, and recovery. Shared custody alone is not treated as hiding correlations.

## Drawbacks and residual risks

Canister outages or defects can prevent recovery. Distributed keys do not imply noncustodial operation or safety. Legal review, independent audits, and correlation evaluation add work.

## Reconsideration criteria

Reconsider if round-trip transfer safety, correlation tests, or legal compliance fall short. Update the design decision rather than automatically switching to agent-only operation or a single account.

## Source of verification status

Track execution results and unimplemented scope in frontend/README.md and
docs/implementation-status.md. Do not bypass the production gates in Plan.md,
section 16.
