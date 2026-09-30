"""Check source drift, Lean proofs, and the axiom closure of every named theorem."""

import hashlib
import json
from pathlib import Path
import re
import subprocess
import sys


def main():
    root = Path(__file__).resolve().parent
    snapshot = json.loads((root / "source-snapshot.json").read_text())
    drift = [
        name for name, expected in snapshot.items()
        if hashlib.sha256((root.parent / name).read_bytes()).hexdigest() != expected
    ]
    if drift:
        sys.exit("Review model correspondence before updating source-snapshot.json: " + ", ".join(drift))

    subprocess.run(["lake", "build", "--wfail"], cwd=root, check=True)
    result = subprocess.run(
        ["lake", "env", "lean", "-DwarningAsError=true", "Audit.lean"],
        cwd=root, text=True, capture_output=True, check=True,
    )
    print(result.stdout, end="")
    named = set()
    for source in (root / "CanisterProofs").glob("*.lean"):
        namespace = re.search(r"^namespace (\S+)", source.read_text(), re.M).group(1)
        named.update(namespace + "." + name for name in
                     re.findall(r"^theorem (\w+)", source.read_text(), re.M))
    audited = set(re.findall(r"^#print axioms (\S+)", (root / "Audit.lean").read_text(), re.M))
    if named != audited:
        sys.exit(f"Audit coverage mismatch: {named ^ audited}")

    closures = re.findall(r"'([^']+)' depends on axioms: \[([^\]]*)\]", result.stdout)
    no_axioms = re.findall(r"'([^']+)' does not depend on any axioms", result.stdout)
    if {name for name, _ in closures} | set(no_axioms) != named:
        sys.exit("Could not account for every theorem's axiom output")
    allowed = {"propext", "Classical.choice", "Quot.sound"}
    for name, deps in closures:
        unexpected = {dep.strip() for dep in deps.split(",") if dep.strip()} - allowed
        if unexpected:
            sys.exit(f"Unapproved axioms in {name}: {unexpected}")
    print(f"Verified {len(named)} theorems; source snapshot matches; no unapproved axioms.")


if __name__ == "__main__":
    main()
