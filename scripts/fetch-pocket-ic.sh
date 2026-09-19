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
# リリース版を更新するときは、次の3つを同時に更新する。
#   POCKET_IC_RELEASE / POCKET_IC_SHA256 / POCKET_IC_SERVER_MAJOR
POCKET_IC_RELEASE="${POCKET_IC_RELEASE:-release-2026-09-18_03-28-base}"
# 展開後のサーババイナリのsha256（このリリースで実測）。空にすると検証をスキップする。
POCKET_IC_SHA256="${POCKET_IC_SHA256:-bb9cb9b9a8293df51d241ed3ea071ed9d1b09c52745222f16d3beb3349213c7e}"
# `pocket-ic` クレート16.0.0が要求する版（>=16,<17）。
POCKET_IC_SERVER_MAJOR="${POCKET_IC_SERVER_MAJOR:-16.}"

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

# キャッシュ済み・新規取得のどちらでも、版と完全性を検証してから使う。
verify() {
  local binary="$1"

  if [[ ! -x "$binary" ]]; then
    echo "fetch-pocket-ic: $binary が実行できません" >&2
    return 1
  fi

  local version
  version="$("$binary" --version 2>/dev/null || true)"
  if [[ "$version" != "pocket-ic-server ${POCKET_IC_SERVER_MAJOR}"* ]]; then
    echo "fetch-pocket-ic: サーバの版が想定と違います: '${version:-unknown}'" >&2
    echo "  期待: pocket-ic-server ${POCKET_IC_SERVER_MAJOR}x" >&2
    echo "  .pocket-ic/ を削除して再取得するか、POCKET_IC_RELEASE を見直してください" >&2
    return 1
  fi

  if [[ -z "$POCKET_IC_SHA256" ]]; then
    echo "fetch-pocket-ic: POCKET_IC_SHA256 が空のため完全性は検証しません" >&2
    return 0
  fi

  local actual
  if command -v sha256sum >/dev/null 2>&1; then
    actual="$(sha256sum "$binary" | awk '{print $1}')"
  elif command -v shasum >/dev/null 2>&1; then
    actual="$(shasum -a 256 "$binary" | awk '{print $1}')"
  else
    echo "fetch-pocket-ic: sha256ツールが無いため完全性を検証できません（続行します）" >&2
    return 0
  fi

  if [[ "$actual" != "$POCKET_IC_SHA256" ]]; then
    echo "fetch-pocket-ic: sha256が一致しません" >&2
    echo "  期待: $POCKET_IC_SHA256" >&2
    echo "  実際: $actual" >&2
    echo "  .pocket-ic/ を削除して再取得するか、POCKET_IC_SHA256 を更新してください" >&2
    return 1
  fi

  return 0
}

if [[ -x "$dest" ]]; then
  verify "$dest" || exit 1
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

verify "$dest" || exit 1

echo "fetch-pocket-ic: $dest"
