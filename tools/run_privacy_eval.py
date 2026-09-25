#!/usr/bin/env python3
"""Run separate generator, public-only attacker and evaluator processes."""

import argparse
import json
import subprocess
import sys
from pathlib import Path

TOOL = Path(__file__).with_name("privacy_eval.py")


def run(*args: str) -> None:
    subprocess.run([sys.executable, str(TOOL), *args], check=True)


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", type=Path, default=Path("target/privacy-eval"))
    args = parser.parse_args()
    summary = []
    for split, seed in (("training", 20260924), ("unseen", 20261001)):
        for users in (20, 100):
            baseline = None
            for arm in ("A", "B0", "B1", "B1Exit"):
                folder = args.output / split / f"{users}-{arm}"
                public = folder / "public.json"
                private = folder / "private" / "truth.json"
                predicted = folder / "attack.json"
                report = folder / "report.json"
                run("generate", "--users", str(users), "--seed", str(seed), "--arm", arm,
                    "--public", str(public), "--private", str(private))
                # The attack subprocess receives only a public trace path. It has no
                # answer-key argument and emits predictions before evaluation begins.
                run("attack", "--public", str(public), "--out", str(predicted))
                flags = () if baseline is None else ("--baseline-top1", str(baseline))
                run("evaluate", "--private", str(private), "--attack", str(predicted),
                    *flags, "--out", str(report))
                data = json.loads(report.read_text(encoding="utf-8"))
                if arm == "A":
                    baseline = data["top1"]
                summary.append({"split": split, **data})
    result = args.output / "summary.json"
    result.write_text(json.dumps(summary, sort_keys=True, indent=2) + "\n", encoding="utf-8")
    print(f"summary={result}")


if __name__ == "__main__":
    main()
