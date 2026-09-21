#!/usr/bin/env bash
# Canisterクレートが「生の秘密鍵で署名するテスト専用ヘルパ」を参照していないことを検査する。
#
# 背景: `docs/phase-0/authority-matrix.md` 5節は、funds_vaultが任意digest署名APIを
# 公開しないこと、trading_coreがmaster署名を要求できないことを不変条件としている。
# `hl-sign` の `sign_digest_for_tests` / `sign_action_for_tests` はfixture比較用に
# 秘密鍵を直接受け取るため、canisterクレートの実装から参照してはならない。
#
# 使い方: bash scripts/check-signing-boundary.sh
set -euo pipefail

crates=(
  "crates/funds-vault/src"
  "crates/trading-core/src"
  "crates/control-guard/src"
  "crates/policy/src"
)

# 生の秘密鍵で署名するヘルパ（`public_key_compressed` は導出鍵の照合に使うため許可する）。
# `sign_with_domain` は独自EIP-712型へ秘密鍵で署名するヘルパで、canisterから参照してはならない。
pattern='sign_digest|sign_action|sign_with_domain|_for_tests'

status=0
for dir in "${crates[@]}"; do
  if [[ ! -d "$dir" ]]; then
    echo "check-signing-boundary: $dir が見つかりません" >&2
    status=1
    continue
  fi

  # コメント行は規則の説明を含むため除外する。
  matches="$(grep -rnE "$pattern" "$dir" --include='*.rs' | grep -vE ':[0-9]+:[[:space:]]*(//|/\*|\*)' || true)"
  if [[ -n "$matches" ]]; then
    echo "check-signing-boundary: $dir が生鍵署名ヘルパを参照しています:" >&2
    echo "$matches" >&2
    status=1
  fi
done

if [[ "$status" -eq 0 ]]; then
  echo "check-signing-boundary: ok"
fi

exit "$status"
