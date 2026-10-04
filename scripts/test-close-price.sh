#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")/.."
# trading-core depends on Wasm-only SQLite. Test its pure rounding module directly.
mkdir -p target/close-price-test
cat > target/close-price-test/harness.rs <<'RUST'
#[path = "../../crates/trading-core/src/close_price.rs"]
mod close_price;
RUST
rustc --edition=2024 --test target/close-price-test/harness.rs -o target/close-price-test/tests
target/close-price-test/tests
