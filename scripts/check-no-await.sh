#!/usr/bin/env bash
# hl-sign と db は非asyncでなければならない（Implementation.md 3.2）。
#
# ic-sqlite-vfs は「トランザクションが await / inter-canister call / call_perform を
# 跨がない」ことを実行時の前提にしている。同じ制約を型だけでなくCIで守る。
#
# 使い方: bash scripts/check-no-await.sh
set -euo pipefail

targets=(
  "crates/hl-sign/src"
  "crates/db/src"
)

# 検査対象のパターン。`ic_cdk::call` は `ic_cdk::caller` に部分一致しないように末尾を限定する。
pattern='\.await|async[[:space:]]+fn|call_perform|ic_cdk::call([^[:alnum:]_]|$)|call_raw'

status=0
for dir in "${targets[@]}"; do
  if [[ ! -d "$dir" ]]; then
    echo "check-no-await: $dir が見つかりません" >&2
    status=1
    continue
  fi

  # コメント行（// と * で始まる行）は規則の説明を含むため除外する。
  matches="$(grep -rnE "$pattern" "$dir" --include='*.rs' | grep -vE ':[[:space:]]*(//|/\*|\*)' || true)"
  if [[ -n "$matches" ]]; then
    echo "check-no-await: $dir に非同期・inter-canister callの記述があります:" >&2
    echo "$matches" >&2
    status=1
  fi
done

if [[ "$status" -eq 0 ]]; then
  echo "check-no-await: ok"
fi

exit "$status"
