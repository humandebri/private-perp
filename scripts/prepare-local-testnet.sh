#!/usr/bin/env bash
# Local IC + real HL testnet. Never uses a mainnet endpoint or mock deposit seed.
set -euo pipefail
repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"
export ICP_HOME="$repo_root/.icp-home"
export PATH="$repo_root/.cargo-home/bin:$PATH"
icp network status local --json >/dev/null

# The script changes venue endpoints and keys. Require an explicit target and
# the same identity that was installed as the application's administrator.
: "${TESTNET_APP_ID:?set the dedicated private_perp canister ID before running this script}"
deployed_id="$(icp canister status private_perp --json | python3 -c 'import json,sys; print(json.load(sys.stdin)["id"])')"
if [[ "$deployed_id" != "$TESTNET_APP_ID" ]]; then
  echo "prepare-local-testnet: private_perp ID does not match TESTNET_APP_ID" >&2
  exit 1
fi
caller="$(icp identity principal)"
administrator="$(icp canister call private_perp application_administrator '()' --args-format candid)"
if [[ "$administrator" != *"principal \"$caller\""* ]]; then
  echo "prepare-local-testnet: current identity is not the application administrator" >&2
  exit 1
fi

# Separate test-only keys live in the ignored local identity directory.
python3 - <<'PY'
from pathlib import Path
import os, secrets
folder = Path('.icp-home/hl-testnet')
folder.mkdir(mode=0o700, parents=True, exist_ok=True)
for name in ('issuer.key', 'owner.key'):
    path = folder / name
    if not path.exists():
        fd = os.open(path, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
        with os.fdopen(fd, 'w') as out:
            out.write(secrets.token_hex(32))
PY
export LOCAL_ELIGIBILITY_ISSUER_KEY="$(cat .icp-home/hl-testnet/issuer.key)"
export PRIVATE_PERP_TESTNET_EOA_KEY="$(cat .icp-home/hl-testnet/owner.key)"
cargo build --locked -p e2e-signer -p eligibility-issuer
export LOCAL_ELIGIBILITY_ISSUER_ADDRESS="$(target/debug/eligibility-issuer address)"
HL_NETWORK=testnet bash scripts/bootstrap-local.sh
export TESTNET_SMOKE=1
pnpm --dir frontend exec vitest run --config vitest.testnet.config.ts
