#!/usr/bin/env bash
# Canister wasmからCandidインターフェース（.did）を抽出する。
#
# フロント（`frontend/src/client`）はCandidでcanisterを呼ぶため、契約の型を
# リポジトリ内の `.did` として固定する。生成元は**本番feature無しのwasm**である
# （試験専用のentry pointをCandidへ出さない）。
#
# 使い方: bash scripts/extract-candid.sh
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"

if ! command -v candid-extractor >/dev/null 2>&1; then
  echo "extract-candid: candid-extractor が必要です（cargo install candid-extractor）" >&2
  exit 1
fi

echo "extract-candid: 本番feature無しのwasmをビルドします"
cargo build --release --target wasm32-unknown-unknown \
  -p policy -p funds-vault -p control-guard -p trading-core

wasm_dir="target/wasm32-unknown-unknown/release"
out_dir="candid"
mkdir -p "$out_dir"

# canister名（icp.yaml）: wasmファイル名: package名
for trio in "funds_vault:funds_vault:funds-vault" "trading_core:trading_core:trading-core" \
            "policy_registry:policy:policy" "control_guard:control_guard:control-guard"; do
  canister="${trio%%:*}"
  rest="${trio#*:}"
  wasm="${rest%%:*}"
  package="${rest##*:}"
  interface="$(candid-extractor "$wasm_dir/$wasm.wasm")"
  printf '%s\n' "$interface" > "$out_dir/$canister.did"
  echo "extract-candid: $out_dir/$canister.did を更新（${package}）"
done

# 試験専用のentry pointがCandidへ漏れていないことを検査する（デプロイ成果物と同じ前提）。
if grep -q "test_" "$out_dir/trading_core.did" "$out_dir/funds_vault.did"; then
  echo "extract-candid: 試験専用のメソッドがCandidに含まれています" >&2
  exit 1
fi

echo "extract-candid: ok"
