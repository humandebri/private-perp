#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"
export ICP_HOME="${ICP_HOME:-$repo_root/.icp-home}"
mock_pid=""
network_owned=false
cleanup() {
  if [[ -n "$mock_pid" ]]; then kill "$mock_pid" 2>/dev/null || true; fi
  if [[ "$network_owned" == true ]]; then icp network stop >/dev/null 2>&1 || true; fi
}
trap cleanup EXIT INT TERM

if [[ -z "${LOCAL_ELIGIBILITY_ISSUER_KEY:-}" ]]; then
  export LOCAL_ELIGIBILITY_ISSUER_KEY="$(openssl rand -hex 32)"
fi
cargo build -p e2e-signer -p eligibility-issuer
export LOCAL_ELIGIBILITY_ISSUER_ADDRESS="$(target/debug/eligibility-issuer address)"
# A fresh project identity store defaults to anonymous, which cannot administer custody.
if [[ "$(icp identity principal)" == '2vxsx-fae' ]]; then
  mkdir -p "$ICP_HOME"
  if ! icp identity principal --identity local-e2e-admin >/dev/null 2>&1; then
    icp identity new local-e2e-admin --storage plaintext --output-seed "$ICP_HOME/local-e2e-admin.seed" >/dev/null
    chmod 600 "$ICP_HOME/local-e2e-admin.seed"
  fi
  icp identity default local-e2e-admin >/dev/null
fi
if ! icp network status local --json >/dev/null 2>&1; then
  icp network start -d
  network_owned=true
fi
# Do not repoint an existing testnet custody canister at the mock venue.
existing_environment="$(icp canister call private_perp vault_get_environment '()' --args-format candid 2>/dev/null || true)"
if [[ "$existing_environment" == *'Testnet'* ]]; then
  echo 'local-e2e: use a separate project/network for mock E2E; this canister uses HL testnet' >&2
  exit 1
fi
icp deploy private_perp --args "(principal \"$(icp identity principal)\")"

canister_id() {
  icp canister status "$1" --json | python3 -c 'import json,sys; print(json.load(sys.stdin)["id"])'
}
app_id="$(canister_id private_perp)"
# Financial E2E issues several outcalls concurrently; their cycles escrow must
# not exhaust the local canister's exit reserve while the test is running.
icp canister top-up "$app_id" --amount 10t
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
  "VITE_PRIVATE_PERP_CANISTER_ID=$app_id" \
  'VITE_MOCK_HL_URL=http://127.0.0.1:8080' \
  'VITE_MARKET_WS_URL=ws://127.0.0.1:8080/ws' > frontend/.env.local

pnpm --dir frontend build
if ! LOCAL_E2E=1 PLAYWRIGHT_HTML_OPEN=never pnpm --dir frontend test:e2e; then
  icp canister logs private_perp || true
  exit 1
fi
