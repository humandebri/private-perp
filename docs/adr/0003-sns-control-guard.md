# ADR-0003: Seven-day delay through SNS and an immutable guard

- Date: 2026-09-18
- Design status: accepted
- Evidence status: Not implemented or demonstrated. A UI demo is not evidence of fund safety, encryption, or governance.

## Context

Direct upgrades through SNS root can change fund processing and confidential-data handling immediately after proposal approval.

## Options considered

Direct SNS root control, a delay within an upgradeable canister, and an immutable external guard.

## Decision

Schedule changes from SNS governance through the guard. Permit upgrades only after seven days from confirmed scheduling and only when hashes match. The production target is a guard as the sole controller of fund canisters, with an empty controller list for the guard itself. Do not expose arbitrary management calls, reinstall, controller additions, independent stop/delete, or shortening the delay.

## Drawbacks and residual risks

Bugs in the guard cannot be fixed through upgrades. Seven days provide an opportunity to exit, not a recovery guarantee. Malicious changes after the delay can access remaining funds and stored information. The delay does not constrain frontend delivery.

## Reconsideration criteria

Reconsider if SNS integration, bypass rejection, cycle replenishment, or exit drills fail. Removing production controllers requires separate approval after an audit.

## Source of verification status

Track execution results and unimplemented scope in frontend/README.md and
docs/implementation-status.md. Do not bypass the production gates in Plan.md,
section 16.
