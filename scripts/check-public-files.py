#!/usr/bin/env python3
"""Reject local-only paths in the Git index; this does not scan secrets or history."""

from pathlib import Path, PurePosixPath
import subprocess
import sys

LOCAL_DIRECTORIES = {
    '.cargo-home', '.cloudflare', '.icp', '.icp-home', '.lake', '.playwright-cli',
    '.pnpm-store', '.pocket-ic', '.tanstack', '.vlmkit', '.wrangler', '__pycache__',
    'node_modules', 'playwright-report', 'target', 'test-results',
}
ENV_EXAMPLES = {'.env.example', '.env.testnet.example', '.dev.vars.example'}


def local_only(name: str) -> bool:
    path = PurePosixPath(name)
    return (
        any(part in LOCAL_DIRECTORIES for part in path.parts)
        or path.name == '.DS_Store'
        or (path.name.startswith(('.env', '.dev.vars')) and path.name not in ENV_EXAMPLES)
        or path.suffix.lower() in {'.pem', '.key', '.seed', '.log', '.pyc', '.pyo'}
    )


def main() -> int:
    root = Path(__file__).resolve().parent.parent
    result = subprocess.run(
        ['git', 'ls-files', '-z'], cwd=root, check=True, stdout=subprocess.PIPE,
    )
    names = result.stdout.decode('utf-8', errors='surrogateescape').split('\0')
    rejected = sorted(name for name in names if name and local_only(name))
    if rejected:
        print('Local-only files are tracked. Remove them from the Git index:', file=sys.stderr)
        for name in rejected:
            print(f'  {name}', file=sys.stderr)
        return 1
    print('Public-file check passed (current Git index; no content or history scan).')
    return 0


if __name__ == '__main__':
    raise SystemExit(main())
