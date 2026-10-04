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
# リリース版を更新するときは、次の4つを同時に更新する。
#   POCKET_IC_RELEASE / POCKET_IC_SHA256_*（4プラットフォーム）/ POCKET_IC_SERVER_MAJOR
POCKET_IC_RELEASE="${POCKET_IC_RELEASE:-release-2026-09-18_03-28-base}"
# 展開後のサーババイナリのsha256（このリリースで**プラットフォーム別に実測**）。
# 資産ごとに別バイナリなので共通の1値は使えない。全プラットフォームで同じ値を使いたい
# 場合だけ `POCKET_IC_SHA256` を設定し、検証をスキップする場合は空文字にする。
POCKET_IC_SHA256_DARWIN_ARM64="${POCKET_IC_SHA256_DARWIN_ARM64:-bb9cb9b9a8293df51d241ed3ea071ed9d1b09c52745222f16d3beb3349213c7e}"
POCKET_IC_SHA256_DARWIN_X86_64="${POCKET_IC_SHA256_DARWIN_X86_64:-0cd62223c4ca24fda4bf6e44e9ccaaa13174d0b32888cf6450f1c0169a2a76b7}"
POCKET_IC_SHA256_LINUX_X86_64="${POCKET_IC_SHA256_LINUX_X86_64:-b780b4194938499e65aa7c05728b52fa72dbde7504d927e6906b546595752f36}"
POCKET_IC_SHA256_LINUX_AARCH64="${POCKET_IC_SHA256_LINUX_AARCH64:-baf46c7723a76483f20d68c0e88e95d9ffb9b9483dc6eb4db539dcb4472dd3cf}"
# `pocket-ic` クレート16.0.0が要求する版（>=16,<17）。
POCKET_IC_SERVER_MAJOR="${POCKET_IC_SERVER_MAJOR:-16.}"

case "$(uname -s)-$(uname -m)" in
  Darwin-arm64)
    asset="pocket-ic-arm64-darwin.gz"
    platform_sha="$POCKET_IC_SHA256_DARWIN_ARM64"
    ;;
  Darwin-x86_64)
    asset="pocket-ic-x86_64-darwin.gz"
    platform_sha="$POCKET_IC_SHA256_DARWIN_X86_64"
    ;;
  Linux-x86_64)
    asset="pocket-ic-x86_64-linux.gz"
    platform_sha="$POCKET_IC_SHA256_LINUX_X86_64"
    ;;
  Linux-aarch64)
    asset="pocket-ic-arm64-linux.gz"
    platform_sha="$POCKET_IC_SHA256_LINUX_AARCH64"
    ;;
  *)
    echo "fetch-pocket-ic: 未対応のプラットフォームです: $(uname -s)-$(uname -m)" >&2
    exit 1
    ;;
esac

# 検証に使う期待値（`POCKET_IC_SHA256` が設定されていればそれを優先）。
expected_sha="${POCKET_IC_SHA256:-$platform_sha}"

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

  if [[ -z "$expected_sha" ]]; then
    echo "fetch-pocket-ic: checksumが未設定のため完全性は検証しません" >&2
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

  if [[ "$actual" != "$expected_sha" ]]; then
    echo "fetch-pocket-ic: sha256が一致しません（プラットフォーム: $(uname -s)-$(uname -m)）" >&2
    echo "  期待: $expected_sha" >&2
    echo "  実際: $actual" >&2
    echo "  .pocket-ic/ を削除して再取得するか、該当プラットフォームのchecksumを更新してください" >&2
    return 1
  fi

  return 0
}

if [[ -x "$dest" ]]; then
  verify "$dest" || exit 1
  echo "fetch-pocket-ic: cached $dest" >&2
  echo "$dest"
  exit 0
fi

mkdir -p "$dest_dir"
url="https://github.com/dfinity/ic/releases/download/${POCKET_IC_RELEASE}/${asset}"
echo "fetch-pocket-ic: $url" >&2
curl -fsSL "$url" -o "$dest_dir/pocket-ic.gz"
gunzip -f "$dest_dir/pocket-ic.gz"
chmod +x "$dest"

verify "$dest" || exit 1

# stdout is a machine-readable path on both download and cached execution.
echo "$dest"
