#!/usr/bin/env python3
"""Check English Markdown documentation and repository-local link targets."""

from pathlib import Path
import re
import subprocess
import sys
from urllib.parse import unquote, urlsplit

ROOT = Path(__file__).resolve().parents[1]
JAPANESE = re.compile(r"[\u3040-\u30ff\u3400-\u4dbf\u4e00-\u9fff]")
LINK = re.compile(r"!?\[[^\]\n]*\]\(([^)\n]+)\)")


def main():
    names = subprocess.check_output(
        ["git", "ls-files", "--cached", "--others", "--exclude-standard", "-z", "--", "*.md"],
        cwd=ROOT,
    ).decode().split("\0")
    errors = []
    checked = 0
    for name in sorted(set(names) - {""}):
        path = ROOT / name
        if not path.is_file():
            continue
        checked += 1
        source = path.read_text(encoding="utf-8")
        fenced = False
        for number, line in enumerate(source.splitlines(), 1):
            if JAPANESE.search(line):
                errors.append(f"{name}:{number}: documentation must be in English")
            if line.lstrip().startswith(("```", "~~~")):
                fenced = not fenced
                continue
            if fenced:
                continue
            for match in LINK.finditer(line):
                target = match.group(1).strip().split(" ", 1)[0].strip("<>")
                url = urlsplit(target)
                if url.scheme or url.netloc or not url.path or url.path.startswith("/"):
                    continue
                resolved = path.parent / unquote(url.path)
                if not resolved.exists():
                    errors.append(f"{name}:{number}: missing link target {target}")
    if errors:
        print("\n".join(errors), file=sys.stderr)
        return 1
    print(f"Documentation check passed ({checked} English Markdown files; local link targets exist).")
    return 0


if __name__ == "__main__":
    sys.exit(main())
