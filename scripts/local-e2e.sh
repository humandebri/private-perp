#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"
export ICP_HOME="${ICP_HOME:-$repo_root/.icp-home}"
mock_pid=""
cleanup() {
  if [[ -n "$mock_pid" ]]; then kill "$mock_pid" 2>/dev/null || true; fi
  icp network stop >/dev/null 2>&1 || true
}
trap cleanup EXIT INT TERM

cargo build -p e2e-signer
icp network start -d
icp deploy

canister_id() {
  icp canister status "$1" --json | python3 -c 'import json,sys; print(json.load(sys.stdin)["id"])'
}
funds_id="$(canister_id funds_vault)"
core_id="$(canister_id trading_core)"
MOCK_HL_ADMIN_ORIGINS='http://127.0.0.1:4173' node tools/mock-hl/server.mjs &
mock_pid="$!"
for _ in {1..50}; do
  if nc -z 127.0.0.1 8080; then break; fi
  sleep 0.1
done
if ! nc -z 127.0.0.1 8080; then
  printf 'LOCAL MOCK HL failed to start\n' >&2
  exit 1
fi
bash scripts/bootstrap-local.sh
printf '%s\n' \
  'VITE_APP_STAGE=local' \
  'VITE_IC_HOST=http://127.0.0.1:18100' \
  "VITE_FUNDS_VAULT_CANISTER_ID=$funds_id" \
  "VITE_TRADING_CORE_CANISTER_ID=$core_id" \
  'VITE_MOCK_HL_URL=http://127.0.0.1:8080' > frontend/.env.local

pnpm --dir frontend build
if ! LOCAL_E2E=1 PLAYWRIGHT_HTML_OPEN=never pnpm --dir frontend test:e2e; then
  icp canister logs trading_core || true
  exit 1
fi
