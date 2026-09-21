#!/usr/bin/env bash
# ローカルへデプロイしたcanisterを初期設定する（frontendが接続できる状態にする）。
#
# PocketICの試験が各テストで行っている初期化と同じ順序で、controllerのidentityから
# 実行する。秘密は扱わない（identityの選択だけ）。
#
# 前提: `icp network start -d` と `icp deploy` が済んでいること。
# 使い方: bash scripts/bootstrap-local.sh
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"

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
  icp canister call "$canister" "$@" --args-format candid
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
for method in set_operator set_sns_principal set_guard_principal; do
  call policy_registry "$method" "(principal \"$(icp identity principal)\")" >/dev/null
done
# BTC・ETHを許可する（asset indexはmetaから解決する）。
call policy_registry set_policy_version '(1 : nat64, vec { "BTC"; "ETH" })' >/dev/null
# 政策行が無い間は停止扱い（fail-closed）のため、SNS経路で解除する。
call policy_registry clear_emergency_stop '()' >/dev/null

echo "bootstrap-local: trading_core を設定する"
vault_principal="$(canister_id funds_vault)"
policy_principal="$(canister_id policy_registry)"
call trading_core set_vault_principal "(principal \"$vault_principal\")" >/dev/null
call trading_core set_policy_principal "(principal \"$policy_principal\")" >/dev/null
# network・dexとmeta（asset indexとszDecimalsの出所。本番はHL /infoから取得する）。
call trading_core set_market_context '("local", "hyperliquid")' >/dev/null
call trading_core set_meta_cache '("local", "hyperliquid", "[{\"name\":\"SOL\",\"szDecimals\":0},{\"name\":\"ETH\",\"szDecimals\":5},{\"name\":\"BTC\",\"szDecimals\":5}]")' >/dev/null
# ローカルの環境（mock HL。実venueのhostは拒否される）。
call trading_core set_venue_endpoints '("http://localhost:8080/exchange", "http://localhost:8080/info")' >/dev/null
call trading_core set_ecdsa_key_id '("test_key_1")' >/dev/null
# 個人APIの封筒（未生成の個人APIはfail-closedで拒否する）。
call trading_core rotate_hpke_key '()' >/dev/null

echo "bootstrap-local: funds_vault を設定する"
call funds_vault set_network '("local")' >/dev/null
call funds_vault set_venue_endpoints '("http://localhost:8080/exchange", "http://localhost:8080/info")' >/dev/null
call funds_vault set_ecdsa_key_id '("test_key_1")' >/dev/null
call funds_vault rotate_hpke_key '()' >/dev/null

echo "bootstrap-local: 設定を確認する"
echo "  trading_core: $(call_read trading_core get_environment)"
echo "  funds_vault:  $(call_read funds_vault get_environment)"
echo "  policy:       $(call_read policy_registry get_stop_status)"
echo "bootstrap-local: ok"
