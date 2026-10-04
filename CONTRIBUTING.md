# Contributing

Contributions to private-perp are welcome. Read the [README](README.md) for the
current testnet scope and [single-canister design](docs/phase-3/single-canister.md)
for the deployed architecture. Use English for documentation, issues, and pull
requests.

## Development environment

- Rust: use `rust-toolchain.toml`; install `rustfmt`, `clippy`, and
  `wasm32-unknown-unknown` through rustup.
- Frontend: Node.js 24 and pnpm 12.4.2, pinned in `frontend/package.json`.
- Scripts: Python 3, Bash, and Node.js.
- Local canister flows: `icp` (icp-cli), OpenSSL, and `nc`.
- API regeneration: `candid-extractor`, `didc`, and `ic-wasm` on `PATH`.
- Proofs: `elan`/`lake` with the toolchain in `proofs/lean-toolchain`.

Install frontend dependencies with `pnpm --dir frontend install --frozen-lockfile`
and generate Workers types with `pnpm --dir frontend cf:typegen`.
Commit Cargo and pnpm lockfiles when intentionally changing dependencies.

Copy `frontend/.env.example` to `frontend/.env.local` for manual local setup.
Values prefixed with `VITE_` are bundled into the browser; never put credentials
there. Keep identities, seeds, keys, local environment files, and runtime state
out of Git. Fixture signing keys are public development keys and must never hold
real assets.

## Verification

Run checks relevant to your change. The workflow files are the authoritative
commands for the full Rust and frontend checks.

```sh
python3 scripts/check-public-files.py
python3 scripts/check-documentation.py
gitleaks git --redact=100 --log-opts='--all' .
cargo fmt --all --check
cargo test --locked
bash scripts/check-no-await.sh
bash scripts/check-signing-boundary.sh
node --test tools/mock-hl/server.test.mjs scripts/pocket-ic-runner.test.mjs scripts/fetch-pocket-ic.test.mjs
```

`cargo test --locked` covers default workspace members only. Canister modules
use Wasm-specific SQLite; do not use a host `cargo test --workspace` as a substitute
for canister verification. Run the host and Wasm clippy commands from
[the Rust workflow](.github/workflows/rust.yml) when changing Rust code.

Repository hygiene CI uses Gitleaks 8.30.1 with a verified download checksum.
The allowlist permits only two public Anvil fixture keys in specific fixture
paths. `.gitleaksignore` records exact historical findings from a previously
committed dependency cache: library examples, bundled test tokens, public
telemetry identifiers, and development certificates. Do not add broad exclusions
or baseline an unexplained credential. Filename checks reject new dependency
caches and local signing state.

```sh
bash scripts/pocket-ic-test.sh --no-fail-fast
bash scripts/test-single-canister.sh
pnpm --dir frontend lint
pnpm --dir frontend format:check
pnpm --dir frontend typecheck
pnpm --dir frontend test
pnpm --dir frontend build
pnpm --dir frontend exec playwright install chromium
pnpm --dir frontend test:e2e
```

PocketIC downloads a checksum-checked server and builds production and test Wasm
into separate target directories. Browser tests cover the shell and CSP by default;
run `bash scripts/local-e2e.sh` in an isolated local environment for the canister
and mock venue flow. That script writes local configuration and deploys a canister.
Live testnet helpers require explicit opt-in and can move test funds; they are
not prerequisites for a contribution.

If an API changes, regenerate Candid and frontend bindings with
`scripts/extract-candid.sh` and `scripts/generate-frontend-bindings.sh`.
If modeled Rust behavior changes, review the model correspondence before updating
proof source hashes and run `python3 proofs/verify.py`; see [proofs/README.md](proofs/README.md).

## Pull requests

1. Open an issue first for substantial architecture or protocol changes.
2. Keep changes focused and explain the affected user behavior and assumptions.
3. Add tests for behavior changes and update documentation and generated interfaces.
4. Include commands run, results, and any checks you could not run.
5. Check the diff for local state, credentials, personal wallet information, and
   unrelated changes before submitting.

Never publish a vulnerability in a public issue; follow [SECURITY.md](SECURITY.md).
Keep discussions respectful and specific to the work. Contributions are provided
under the repository's [MIT license](LICENSE); only submit material you are
entitled to contribute.
