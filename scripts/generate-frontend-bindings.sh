#!/usr/bin/env bash
# Candid（`candid/*.did`）から画面用のTypeScriptバインディングを生成する。
#
# 生成物は `frontend/src/client/candid/` へ置き、コミットする（画面のビルドにRust
# ツールチェーンを要求しない）。`.did` の更新は `bash scripts/extract-candid.sh`。
#
# `didc` は `@dfinity/*` をimportするコードを生成するが、2026-09時点でそれらは
# 非推奨（`@icp-sdk/core` へ移行）のため、import先を書き換える。
#
# 使い方: bash scripts/generate-frontend-bindings.sh
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"

if ! command -v didc >/dev/null 2>&1; then
  echo "generate-frontend-bindings: didc が必要です（candid-extractor と同梱のCLI）" >&2
  exit 1
fi

out_dir="frontend/src/client/candid"
mkdir -p "$out_dir"

# `--target js` は実行時（idlFactory・init）、`--target ts` は型定義を生成する。
# 同じ基底名にして `.did.js` + `.did.d.ts` の組で置く（TypeScriptは.jsの型を.d.tsから引く）。
for canister in funds_vault trading_core; do
  header="// 生成物: \`bash scripts/generate-frontend-bindings.sh\`（元: candid/$canister.did）
// 手で編集しない。契約（\`crates/api-types\`）を変えたら .did と本ファイルを再生成する。"
  runtime="$out_dir/$canister.did.js"
  types="$out_dir/$canister.did.d.ts"
  { printf '%s\n' "$header"; didc bind "candid/$canister.did" --target js; } > "$runtime"
  {
    printf '%s\n' "$header"
    didc bind "candid/$canister.did" --target ts
  } | sed \
    -e 's#from '"'"'@dfinity/agent'"'"'#from '"'"'@icp-sdk/core/agent'"'"'#g' \
    -e 's#from '"'"'@dfinity/candid'"'"'#from '"'"'@icp-sdk/core/candid'"'"'#g' \
    -e 's#from '"'"'@dfinity/principal'"'"'#from '"'"'@icp-sdk/core/principal'"'"'#g' \
    > "$types"
  echo "generate-frontend-bindings: $runtime / $types を更新"
done

if grep -q "@dfinity/" "$out_dir"/*.did.d.ts; then
  echo "generate-frontend-bindings: 非推奨のimport先が残っています" >&2
  exit 1
fi

# 生成物もコミット対象なので、毎回同じ整形結果にする。
pnpm --dir frontend exec oxfmt --write "src/client/candid/*"

echo "generate-frontend-bindings: ok"
