#!/usr/bin/env bash
# ローカルへデプロイしたcanisterを初期設定する（frontendが接続できる状態にする）。
#
# PocketICの試験が各テストで行っている初期化と同じ順序で、controllerのidentityから
# 実行する。秘密は扱わない（identityの選択だけ）。
#
# 前提: `icp network start -d` と `icp deploy` が済んでいること。
# 使い方: bash scripts/bootstrap-local.sh
set -euo pipefail

# The mock issuer's private key is supplied to a separate local issuer process.
# Bootstrap accepts only its public address and never reads/stores the secret.
: "${LOCAL_ELIGIBILITY_ISSUER_ADDRESS:?set the local mock issuer public 0x address}"
if [[ ! "$LOCAL_ELIGIBILITY_ISSUER_ADDRESS" =~ ^0x[0-9a-fA-F]{40}$ ]]; then
  echo "bootstrap-local: LOCAL_ELIGIBILITY_ISSUER_ADDRESS must be a 20-byte 0x address" >&2
  exit 1
fi
issuer_bytes="$(python3 -c 'import os; b=bytes.fromhex(os.environ["LOCAL_ELIGIBILITY_ISSUER_ADDRESS"][2:]); print("vec {"+"; ".join(str(x)+" : nat8" for x in b)+"}")')"

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"

# The IC network remains local. Only the HL venue may be changed to testnet.
hl_network="${HL_NETWORK:-local}"
case "$hl_network" in
  local)
    exchange_url='http://localhost:8080/exchange'
    info_url='http://localhost:8080/info'
    universe='[{"name":"SOL","szDecimals":0},{"name":"ETH","szDecimals":5},{"name":"BTC","szDecimals":5}]'
    btc_index=2
    eth_index=1
    history_verified=true
    ;;
  testnet)
    exchange_url='https://api.hyperliquid-testnet.xyz/exchange'
    info_url='https://api.hyperliquid-testnet.xyz/info'
    meta="$(curl --fail --silent --show-error --max-time 30 "$info_url" -H 'Content-Type: application/json' --data '{"type":"meta"}')"
    universe="$(printf '%s' "$meta" | python3 -c 'import json,sys; print(json.dumps(json.load(sys.stdin)["universe"],separators=(",",":")))')"
    btc_index="$(printf '%s' "$universe" | python3 -c 'import json,sys; print(next(i for i,x in enumerate(json.load(sys.stdin)) if x["name"]=="BTC"))')"
    eth_index="$(printf '%s' "$universe" | python3 -c 'import json,sys; print(next(i for i,x in enumerate(json.load(sys.stdin)) if x["name"]=="ETH"))')"
    history_verified=false
    ;;
  *) echo 'bootstrap-local: HL_NETWORK must be local or testnet' >&2; exit 1 ;;
esac
universe_candid="$(printf '%s' "$universe" | python3 -c 'import json,sys; print(json.dumps(sys.stdin.read()))')"

# ローカル開発はプロジェクト専用のICP_HOME（`.icp-home/`、gitignore済み）を使う。
# 他のプロジェクトのidentity・既定を変更しないため、既定identityはこのstoreの中で持つ。
export ICP_HOME="${ICP_HOME:-$repo_root/.icp-home}"

# canister名からIDを引く（IDはデプロイごとに変わり得るためハードコードしない）。
canister_id() {
  icp canister status "$1" --json | python3 -c 'import json,sys; print(json.load(sys.stdin)["id"])'
}

call() {
  local canister="$1"
  shift
  local response
  response="$(icp canister call "$canister" "$@" --args-format candid)"
  if [[ "$response" == *"Err ="* ]]; then
    echo "bootstrap-local: $canister $1 failed: $response" >&2
    return 1
  fi
  printf '%s\n' "$response"
}

# 応答を1行にまとめて表示する（診断用）。
call_read() {
  local canister="$1"
  local method="$2"
  local args="${3:-()}"
  icp canister call "$canister" "$method" "$args" --args-format candid 2>&1 \
    | tr -d '\n' | tr -s ' ' | sed 's/^ *//'
}

echo "bootstrap-local: policy_registry を設定する"
controller_principal="$(icp identity principal)"
guard_principal="$(canister_id control_guard)"
vault_principal="$(canister_id funds_vault)"
core_principal="$(canister_id trading_core)"
policy_principal="$(canister_id policy_registry)"
journal_principal="$(canister_id send_journal)"
call send_journal register_worker "(\"vault\", principal \"$vault_principal\")" >/dev/null
call send_journal register_worker "(\"core\", principal \"$core_principal\")" >/dev/null
call funds_vault set_send_journal "(principal \"$journal_principal\")" >/dev/null
call trading_core set_send_journal "(principal \"$journal_principal\")" >/dev/null
call funds_vault set_journal_guard "(principal \"$guard_principal\")" >/dev/null
call trading_core set_journal_guard "(principal \"$guard_principal\")" >/dev/null
call control_guard set_sns_principal "(principal \"$controller_principal\")" >/dev/null
call policy_registry set_operator "(principal \"$controller_principal\")" >/dev/null
call policy_registry set_sns_principal "(principal \"$controller_principal\")" >/dev/null
call policy_registry set_guard_principal "(principal \"$guard_principal\")" >/dev/null
call policy_registry register_budget_worker "(\"vault\", principal \"$vault_principal\")" >/dev/null
call policy_registry register_budget_worker "(\"core\", principal \"$core_principal\")" >/dev/null
call control_guard configure_rest_budget "(principal \"$policy_principal\", record { capacity = 1200 : nat32; exit_reserve = 300 : nat32 })" >/dev/null
call control_guard configure_eligibility "(principal \"$vault_principal\", 1 : nat64, $issuer_bytes, true)" >/dev/null
# Local estimates only. Testnet floors and reserves require measured calibration.
call control_guard configure_cycles "(principal \"$vault_principal\", 1000000000 : nat, 100000000000 : nat)" >/dev/null
call control_guard configure_cycles "(principal \"$core_principal\", 1000000000 : nat, 100000000000 : nat)" >/dev/null
# BTC・ETHを許可する（asset indexはmetaから解決する）。
# An unconfigured policy returns an application Err on a fresh installation.
# Read it without the strict mutation wrapper so the initialization below runs;
# transport/CLI failures still terminate the script under set -e.
policy_state="$(icp canister call policy_registry get_policy '()' --args-format candid)"
if [[ "$policy_state" != *"Ok"* || "$policy_state" != *"BTC"* || "$policy_state" != *"ETH"* ]]; then
  call control_guard configure_policy_version "(principal \"$policy_principal\", 1 : nat64, vec { \"BTC\"; \"ETH\" })" >/dev/null
  policy_state="$(call policy_registry get_policy '()')"
fi
if [[ "$policy_state" != *"Ok"* || "$policy_state" != *"BTC"* || "$policy_state" != *"ETH"* ]]; then
  echo "bootstrap-local: policy allowlist configuration failed: $policy_state" >&2
  exit 1
fi
# 政策行が無い間は停止扱い（fail-closed）のため、SNS経路で解除する。
call policy_registry clear_emergency_stop '()' >/dev/null

echo "bootstrap-local: trading_core を設定する"
call trading_core set_vault_principal "(principal \"$vault_principal\")" >/dev/null
call trading_core set_policy_principal "(principal \"$policy_principal\")" >/dev/null
# network・dexとmeta（asset indexとszDecimalsの出所。本番はHL /infoから取得する）。
call trading_core set_market_context "(\"$hl_network\", \"hyperliquid\")" >/dev/null
call trading_core set_meta_cache "(\"$hl_network\", \"hyperliquid\", $universe_candid)" >/dev/null
# ローカルの環境（mock HL。実venueのhostは拒否される）。
call trading_core set_venue_endpoints "(\"$exchange_url\", \"$info_url\")" >/dev/null
call trading_core set_ecdsa_key_id '("test_key_1")' >/dev/null
call control_guard configure_market_threshold "(principal \"$core_principal\", record { market = \"BTC\"; expected_index = $btc_index : nat32; min_day_notional_usdc = 1000000 : nat64; max_spread_bps = 20 : nat32; min_each_side_depth_usdc = 10000 : nat64 })" >/dev/null
call control_guard configure_market_threshold "(principal \"$core_principal\", record { market = \"ETH\"; expected_index = $eth_index : nat32; min_day_notional_usdc = 1000000 : nat64; max_spread_bps = 20 : nat32; min_each_side_depth_usdc = 1000 : nat64 })" >/dev/null
# 個人APIの封筒（未生成の個人APIはfail-closedで拒否する）。
call trading_core rotate_hpke_key '()' >/dev/null
call trading_core refresh_market '()' >/dev/null

echo "bootstrap-local: funds_vault を設定する"
call funds_vault set_core_principal "(principal \"$core_principal\")" >/dev/null
call funds_vault set_policy_principal "(principal \"$policy_principal\")" >/dev/null
call funds_vault set_network "(\"$hl_network\")" >/dev/null
call funds_vault set_venue_endpoints "(\"$exchange_url\", \"$info_url\")" >/dev/null
# ローカルmockのページングを検証した環境に限り、未実行判定を許可する。
call funds_vault set_recovery_history_verified "($history_verified)" >/dev/null
call funds_vault set_ecdsa_key_id '("test_key_1")' >/dev/null
call funds_vault rotate_hpke_key '()' >/dev/null

echo "bootstrap-local: 設定を確認する"
echo "  trading_core: $(call_read trading_core get_environment)"
echo "  funds_vault:  $(call_read funds_vault get_environment)"
echo "  policy:       $(call_read policy_registry get_stop_status)"
echo "bootstrap-local: ok"
