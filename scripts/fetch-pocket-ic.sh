#!/usr/bin/env bash
# PocketICサーババイナリを取得する（テスト専用）。
#
# `crates/pocket-ic-tests` は `POCKET_IC_BIN` でサーババイナリを指定する。
# バイナリはリポジトリへコミットせず、`.pocket-ic/` に置く（.gitignore済み）。
#
# 使い方: bash scripts/fetch-pocket-ic.sh
set -euo pipefail

# クレート版（Cargo.toml の pocket-ic）と対応するリリースを固定する。
# 変更するときは crates/pocket-ic-tests のテストが通ることを確認する。
POCKET_IC_RELEASE="${POCKET_IC_RELEASE:-release-2026-09-18_03-28-base}"

case "$(uname -s)-$(uname -m)" in
  Darwin-arm64) asset="pocket-ic-arm64-darwin.gz" ;;
  Darwin-x86_64) asset="pocket-ic-x86_64-darwin.gz" ;;
  Linux-x86_64) asset="pocket-ic-x86_64-linux.gz" ;;
  Linux-aarch64) asset="pocket-ic-arm64-linux.gz" ;;
  *)
    echo "fetch-pocket-ic: 未対応のプラットフォームです: $(uname -s)-$(uname -m)" >&2
    exit 1
    ;;
esac

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
dest_dir="${POCKET_IC_DIR:-$repo_root/.pocket-ic}"
dest="$dest_dir/pocket-ic"

if [[ -x "$dest" ]]; then
  echo "fetch-pocket-ic: 取得済み $dest"
  echo "$dest"
  exit 0
fi

mkdir -p "$dest_dir"
url="https://github.com/dfinity/ic/releases/download/${POCKET_IC_RELEASE}/${asset}"
echo "fetch-pocket-ic: $url"
curl -fsSL "$url" -o "$dest_dir/pocket-ic.gz"
gunzip -f "$dest_dir/pocket-ic.gz"
chmod +x "$dest"
echo "fetch-pocket-ic: $dest"
