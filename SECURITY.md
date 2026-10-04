# Security policy

private-perp is experimental software for local development and Hyperliquid
testnet. There are no supported stable release lines or production security
assurances. Security fixes target the current development branch.

## Reporting a vulnerability

Use GitHub's **Security → Report a vulnerability** for this repository when
private vulnerability reporting is enabled:

https://github.com/humandebri/private-perp/security/advisories/new

If that feature is unavailable, contact the repository owner through a private
contact method listed on their GitHub profile. If no private method is available,
open an issue requesting a private reporting channel without disclosing the
vulnerability. Do not include exploit details, credentials, or personal data in
public issues, pull requests, or discussions.

Include the affected commit, reproduction steps using local mocks or testnet,
expected and observed behavior, impact, and a minimal proof of concept when
available. Never send real private keys or seed phrases. Please allow time for
triage and coordination before public disclosure; no response SLA is promised.

## Relevant boundaries

Authentication, HPKE request handling, key derivation and signing, fund ownership,
ledger atomicity, async callbacks, outbox reconciliation, upgrades, and controller
permissions are security-sensitive. The unified canister shares upgrade authority
and rollback state across modules. The privacy evaluation has not met the
anonymity target, and Lean proofs cover selected models rather than the compiled
application. See the [architecture](docs/phase-3/single-canister.md),
[privacy evaluation](docs/phase-3/privacy-local-eval.md), and
[proof limitations](proofs/README.md).

Committed fixture keys are intentionally public development keys. Do not use
these keys for real assets or include operational identities in bug reports.
