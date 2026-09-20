#!/usr/bin/env bash
# PocketIC統合試験を1コマンドで実行する。
#
# 1. PocketICサーババイナリを用意する（未取得ならscripts/fetch-pocket-ic.sh）
# 2. Canisterのwasmをビルドする
# 3. POCKET_IC_BINを設定して crates/pocket-ic-tests の試験を実行する
#
# 使い方: bash scripts/pocket-ic-test.sh [cargo test へ渡す引数...]
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"

# 環境に互換性のない POCKET_IC_BIN が設定されている場合があるため、
# ワークスペースの .pocket-ic/ を明示的に使う（POCKET_IC_BIN_OVERRIDE=1 で上書き可能）。
server_bin="$(bash scripts/fetch-pocket-ic.sh | tail -n1)"
if [[ "${POCKET_IC_BIN_OVERRIDE:-}" != "1" ]]; then
  if [[ -n "${POCKET_IC_BIN:-}" && "${POCKET_IC_BIN}" != "$server_bin" ]]; then
    echo "pocket-ic-test: 環境の POCKET_IC_BIN=${POCKET_IC_BIN} は使わず $server_bin を使います"
  fi
  export POCKET_IC_BIN="$server_bin"
fi

echo "pocket-ic-test: POCKET_IC_BIN=$POCKET_IC_BIN"
echo "pocket-ic-test: wasmをビルドします"
# funds-vault はテスト専用の入金計上（test-venue）を有効にしてビルドする。
# 本番ビルド（icp build / CIのwasmビルド）は feature 無しでビルドする。
cargo build --release --target wasm32-unknown-unknown \
  -p policy -p control-guard -p trading-core
cargo build --release --target wasm32-unknown-unknown -p funds-vault --features test-venue

echo "pocket-ic-test: 試験を実行します"
cargo test -p pocket-ic-tests "$@"
