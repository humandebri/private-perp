#!/usr/bin/env bash
# Production and mock Wasm are built separately to avoid shipping test exports.
set -euo pipefail
repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"
export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-$repo_root/target}"
mkdir -p "$CARGO_TARGET_DIR"
export CARGO_TARGET_DIR="$(cd "$CARGO_TARGET_DIR" && pwd)"
test_target="${PRIVATE_PERP_TEST_TARGET:-$CARGO_TARGET_DIR/unified-tests}"
mkdir -p "$test_target"
test_target="$(cd "$test_target" && pwd)"
if [[ "$test_target" == "$CARGO_TARGET_DIR" ]]; then
  echo "Production and mock target directories must differ." >&2
  exit 1
fi
export POCKET_IC_BIN="${POCKET_IC_BIN:-$repo_root/.pocket-ic/pocket-ic}"
if [[ ! -x "$POCKET_IC_BIN" ]]; then
  echo 'Set POCKET_IC_BIN to an installed PocketIC server.' >&2
  exit 1
fi
unset PRIVATE_PERP_UNIFIED_MOCK
cargo build -p private-perp --release --target wasm32-unknown-unknown
export PRIVATE_PERP_UNIFIED_WASM="$CARGO_TARGET_DIR/wasm32-unknown-unknown/release/private_perp.wasm"
python3 scripts/check-single-canister.py "$PRIVATE_PERP_UNIFIED_WASM"
cargo test -p pocket-ic-tests --test single_canister --test market_consensus
CARGO_TARGET_DIR="$test_target" cargo build -p private-perp --features test-venue --release --target wasm32-unknown-unknown
PRIVATE_PERP_UNIFIED_MOCK=1 PRIVATE_PERP_UNIFIED_WASM="$test_target/wasm32-unknown-unknown/release/private_perp.wasm" \
  cargo test -p pocket-ic-tests --test single_canister --test market_consensus
