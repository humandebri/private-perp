#!/usr/bin/env bash
# PocketIC統合試験を1コマンドで実行する。
#
# 1. PocketICサーババイナリを用意する（未取得ならscripts/fetch-pocket-ic.sh）
# 2. Canisterのwasmをビルドする
#    2a. 本番と同じfeature無しのwasm（デプロイ成果物の検査用）
#    2b. test-venue付きのwasm（**別のtargetディレクトリ**へ出し、本番成果物を上書きしない）
# 3. POCKET_IC_BIN と POCKET_IC_WASM_DIR を設定して crates/pocket-ic-tests の試験を実行する
#
# 使い方: bash scripts/pocket-ic-test.sh [cargo test へ渡す引数...]
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"

# 環境に互換性のない POCKET_IC_BIN が設定されている場合があるため、
# ワークスペースの .pocket-ic/ を明示的に使う（POCKET_IC_BIN_OVERRIDE=1 で上書き可能）。
if server_bin="$(bash scripts/fetch-pocket-ic.sh | tail -n1)"; then
  test -n "$server_bin" || { echo "pocket-ic-test: server path is empty" >&2; exit 1; }
else
  status=$?
  echo "pocket-ic-test: server preparation failed (exit=$status)" >&2
  exit "$status"
fi
if [[ "${POCKET_IC_BIN_OVERRIDE:-}" != "1" ]]; then
  if [[ -n "${POCKET_IC_BIN:-}" && "${POCKET_IC_BIN}" != "$server_bin" ]]; then
    echo "pocket-ic-test: 環境の POCKET_IC_BIN=${POCKET_IC_BIN} は使わず $server_bin を使います"
  fi
  export POCKET_IC_BIN="$server_bin"
fi

echo "pocket-ic-test: POCKET_IC_BIN=$POCKET_IC_BIN"

# test-venue 付きのwasmを本番成果物と同じパス（target/wasm32-unknown-unknown/release）へ
# 書かない。icp build / icp deploy が参照するwasmを試験用ビルドで汚さないため、
# 試験用は target/test-venue 配下へ出す（crates/pocket-ic-tests が参照する）。
# 同時実行（別セッションのビルド）と成果物を取り合わないよう直列化する。
lock_dir="$repo_root/target/pocket-ic-test.lock.d"
mkdir -p "$repo_root/target"
lock_owned=false
cleanup() {
  if [[ "$lock_owned" == true ]]; then rmdir "$lock_dir" 2>/dev/null || true; fi
}
trap cleanup EXIT
trap 'exit 130' INT
trap 'exit 143' TERM
for _ in $(seq 1 120); do
  if mkdir "$lock_dir" 2>/dev/null; then
    lock_owned=true
    break
  fi
  sleep 5
done
if [[ "$lock_owned" != true ]]; then
  echo "pocket-ic-test: lock acquisition timed out: $lock_dir" >&2
  exit 1
fi

run_step() {
  local status
  if "$@"; then return 0; else status=$?; fi
  echo "pocket-ic-test: command failed (exit=$status): $*" >&2
  exit "$status"
}

# 別セッションと成果物を取り合わないよう、用途別のディレクトリを指定できる。
test_venue_target="${POCKET_IC_TEST_DIR:-$repo_root/target/test-venue}"
mkdir -p "$test_venue_target"
test_venue_target="$(cd "$test_venue_target" && pwd -P)"
production_target="$(cd "$repo_root/target" && pwd -P)"
if [[ "$test_venue_target" == "$production_target" ]]; then
  echo "pocket-ic-test: test and production target directories must differ" >&2
  exit 1
fi
export POCKET_IC_WASM_DIR="$test_venue_target/wasm32-unknown-unknown/release"

echo "pocket-ic-test: 本番feature無しのwasmをビルドします（デプロイ成果物の検査用）"
CARGO_TARGET_DIR="$production_target" run_step cargo build --locked --release --target wasm32-unknown-unknown \
  -p policy -p funds-vault -p control-guard -p trading-core -p send-journal
CARGO_TARGET_DIR="$production_target" run_step cargo build --locked --release --target wasm32-unknown-unknown \
  -p private-perp

echo "pocket-ic-test: test-venue付きのwasmをビルドします（POCKET_IC_WASM_DIR=${POCKET_IC_WASM_DIR}）"
CARGO_TARGET_DIR="$test_venue_target" run_step cargo build --locked --release --target wasm32-unknown-unknown \
  -p policy -p control-guard -p send-journal
CARGO_TARGET_DIR="$test_venue_target" run_step cargo build --locked --release --target wasm32-unknown-unknown \
  -p funds-vault -p trading-core \
  --features funds-vault/test-venue,trading-core/test-venue
CARGO_TARGET_DIR="$test_venue_target" run_step cargo build --locked --release --target wasm32-unknown-unknown \
  -p private-perp --features private-perp/test-venue
export PRIVATE_PERP_UNIFIED_WASM="$test_venue_target/wasm32-unknown-unknown/release/private_perp.wasm"
export PRIVATE_PERP_UNIFIED_MOCK=1

echo "pocket-ic-test: 試験を実行します"
# 20人/100人のPocketIC負荷試験を同一プロセスで並行実行すると、サーバが
# instanceを削除して試験自体が失敗する。各binary内の試験も既定で直列化する。
export RUST_TEST_THREADS="${RUST_TEST_THREADS:-1}"
run_step cargo test --locked -p pocket-ic-tests "$@"
echo "pocket-ic-test: all requested tests completed successfully (exit=0)"
