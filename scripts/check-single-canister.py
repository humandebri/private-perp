#!/usr/bin/env python3
"""Check the production unified Candid against actual Wasm exports."""
from pathlib import Path
import re
import subprocess
import sys

root = Path(__file__).resolve().parents[1]
wasm = sys.argv[1] if len(sys.argv) == 2 else str(root / "target/wasm32-unknown-unknown/release/private_perp.wasm")
did = root / "candid/private_perp.did"
subprocess.run(["didc", "check", str(did)], check=True)
info = subprocess.run(["ic-wasm", wasm, "info"], check=True, capture_output=True, text=True).stdout
actual = {(kind, name) for kind, name in re.findall(r'"canister_(query|update) ([^"]+)"', info) if not name.startswith("<ic-cdk internal>")}
expected = set()
for name, signature in re.findall(r"^  (\w+) : (.*);$", did.read_text().split("service :", 1)[1], re.M):
    expected.add(("query" if signature.endswith(" query") else "update", name))
if expected != actual:
    raise SystemExit(f"missing exports: {sorted(expected - actual)}; undocumented exports: {sorted(actual - expected)}")
print(f"Unified Candid matches {len(actual)} Wasm endpoints")
