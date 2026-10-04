# Publication and release checklist

This checklist records the steps needed to publish the repository. Adding an
MIT license and documentation does not itself make the GitHub repository public
or confirm that its historical commits are safe to publish.

The repository is already public. During the October 4, 2026 preparation,
Gitleaks 8.30.1 scanned 178 reachable commits. Findings were public Anvil fixture
keys and dependency-cache examples, test tokens, public telemetry identifiers,
and development certificates. Exact historical cache findings and the two
fixture keys are documented in the scanner configuration; the configured scan
passed. A snapshot of tracked and intended new files contained only the known
fixture keys. This check does not establish that every kind of private data is
absent. Private vulnerability reporting is enabled. Branch protection remains a
maintainer configuration decision. The checklist below also applies to future releases and imports.

## Before changing repository visibility

- Confirm the right to release the code, documentation, fixtures, and assets
  under MIT. Dependencies keep their original license terms; preserve any
  required notices when distributing builds.
- Review **all Git history**, branches, tags, and Git LFS objects for credentials,
  seed phrases, operational identities, private data, and proprietary material.
  Use a dedicated secret scanner as well as manual review. The public-file check
  only examines filenames in the current index and is not a secret scanner.
- Rotate or revoke any credentials found in history before publication. Removing
  a file from the current tree or adding an ignore rule does not remove history.
  Coordinate any necessary history rewrite with collaborators.
- Review public deployment identifiers, wallet addresses, hostnames, and recorded
  logs for information that the maintainers intend to disclose.
- Enable private vulnerability reporting and verify the link in `SECURITY.md`.
- Enable secret scanning and push protection where available. Configure branch
  protection or rulesets and require Rust verification, PocketIC integration,
  Frontend verification, and Repository hygiene after confirming they pass on a clean clone.
- Inspect GitHub Actions permissions, repository secrets, collaborators, and
  fork pull-request behavior. Keep verification independent of deployment keys.
- Validate setup instructions on a clean checkout. Local E2E must use a dedicated
  local network; public testnet flows are a separate acceptance exercise.

## Before a release

- Record the source commit, toolchain versions, artifact hashes, and verification
  commands and results. Avoid reusing test-venue Wasm as a release artifact.
- Review dependency licenses for the artifacts being distributed and include
  required third-party notices. The repository MIT license does not relicense
  third-party packages.
- Confirm Candid bindings match production Wasm, proof snapshots match reviewed
  source, and runtime configuration points to the intended environment.
- Publish known limitations: incomplete public testnet fund/trade acceptance,
  failed anonymity baseline, shared controller trust, and proof scope.
- Explain migrations, backup and recovery expectations, and upgrade authority.
  Do not label a release mainnet-ready based solely on local mock tests.
