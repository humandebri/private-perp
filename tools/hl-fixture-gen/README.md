# hl-fixture-gen

Rust 実装 (`crates/hl-sign`) の **digest / 署名検証**に使う Hyperliquid の署名テストベクタ（fixture）を、
公式相当の TypeScript SDK で生成して `crates/hl-sign/tests/fixtures/` に固定するためのツール。

- SDK: **`@nktkas/hyperliquid` バージョン `0.33.3`（exact 固定）**
- ウォレット / 検証: **`viem` バージョン `2.56.8`（exact 固定）**
- 出力先: `crates/hl-sign/tests/fixtures/*.json`（1 ファイル 1 オブジェクト）

## 生成コマンド

```sh
cd tools/hl-fixture-gen && pnpm install --frozen-lockfile && pnpm generate
```

- `pnpm` は `packageManager: pnpm@12.4.2`（frontend と同じ）。corepack で解決できる。
- `node_modules/` はコミットしない（リポジトリ直下の `.gitignore` の `node_modules/` が効く）。
  `pnpm-lock.yaml` はコミット対象。
- `pnpm-workspace.yaml` に `storeDir: .pnpm-store` を置いている。pnpm は既定でプロジェクトと同じ
  ドライブのルートに store を作るため、これが無いと `pnpm install` がリポジトリ直下に
  未追跡の `.pnpm-store/` を作ってしまう。この指定で `tools/hl-fixture-gen/.pnpm-store/` に閉じる
  （`.gitignore` で無視）。
- ネットワークは **不要**（`pnpm install` 以外）。生成処理は SDK をローカルで呼ぶだけで、
  どこにもリクエストを送らない。transport は捕獲用のスタブに差し替えている。

## 使用している秘密鍵（テスト専用）

**実資金の鍵は一切使っていない。**すべて Hardhat / Anvil の既定アカウント（広く公開されている
開発用の鍵）で、mainnet に資産は無い。

| 用途 | 秘密鍵 | アドレス |
| --- | --- | --- |
| master（署名者） | `0x59c6995e998f97a5a0044966f0945389dc9e86dae88c7a8412f4603b6b78690d` | `0x70997970c51812dc3a010c7d01b50e0d17dc79c8` |
| agent（agent 署名の署名者 / `approveAgent` の agentAddress） | `0x5de4111afa1a4b94908f83103eb1f1706367c2e68ca870fc3fb9a804cdab365a` | `0x3c44cdddb6a900fa2b585dd299e03d12fa4293bc` |
| `usdSend` の destination | （鍵は使わない） | `0x90f79bf6eb2c4f870365e785982e1f101e93b906` |
| `vaultAddress` | （鍵は使わない） | `0x15d34aaf54267db7d7c367839aaf71a00a2c6a65` |

master の鍵は Anvil アカウント #1。Anvil #0 は `approveAgent` の例などで使われることがあるが、
本 fixture では使っていない。`address` は **小文字**（SDK の `getWalletAddress()` の戻り値に合わせる）。

## fixture 一覧

`crates/hl-sign/tests/fixtures/` に以下を生成する。`network` はすべて `"testnet"`。

| ファイル | 署名方式 | 内容 | `connection_id_hex` | `signature.v` |
| --- | --- | --- | --- | --- |
| `order_limit_btc.json` | L1 (phantom agent) | BTC perp の GTC 指値（`tif: "Gtc"`） | `0x3b3f5c5508dc8e8c59185f767bb5ba7ac0bcdf74993d62f2e48658d9d969dcd8` | 28 |
| `order_market_ioc_eth.json` | L1 | ETH perp の IOC（指値がスリッページ上限、`tif: "Ioc"`） | `0x31de2f241b8ae7fd0cf73ecc7e2fa42ba94867f23ceb93dce582fcaad4bead83` | 27 |
| `cancel.json` | L1 | oid 指定の取消 | `0x1cd673d1288bdf9f2367b83f147c107b6ef056f7ab2065ff123c4d9ed2ead3cc` | 27 |
| `cancel_large_oid.json` | L1 | oid が 2^32 以上（`o: 4294967297`）の取消（**要件外の追加ケース**、msgpack 整数符号化の境界用） | `0x9c8632695c288dbe0a21cf5d24a31f0ae18b50c1efb17d18443b4dc13b75b740` | 28 |
| `cancel_by_cloid.json` | L1 | cloid 指定の取消 | `0x6cb641b58354f44d668751e03483ac0e7e3e60a6ab914da635f21975dda39393` | 27 |
| `update_leverage.json` | L1 | レバレッジ更新（cross、10x） | `0x3fa6e66ded7cb0fc2b73342cadb0ca11c21e9a126a5816ac14ec857c800488b1` | 28 |
| `order_limit_vault_expires.json` | L1 | `vaultAddress` + `expiresAfter` 付きの BTC 指値（**要件外の追加ケース**） | `0x14763ac3c5f9ef33529a5965eef796bbcc597876bf45fb991f073eb56b4512de` | 27 |
| `order_expires_only.json` | L1 | `expiresAfter` のみ（`vaultAddress` なし）の BTC 指値（**要件外の追加ケース**、連結の確認用） | `0x8b6cef43542256268b4b29c869b6491c1af5b1d5a0c0090e3d733c22e9f6fa89` | 27 |
| `approve_agent.json` | User-signed EIP-712 | Agent 承認（master 署名、`is_agent: false`） | `null`（user-signed に connectionId は無い） | 27 |
| `usd_send.json` | User-signed EIP-712 | USDC 送金（`usdSend`） | `null` | 27 |
| `order_limit_agent.json` | L1 | Agent 鍵で署名した BTC 指値（`is_agent: true`） | `0x1de09e5c979dbef93d74a3b50a93fe413b1729453c6c1345554d7f5cc8976dd0` | 27 |

要件の 8 ケースに対する追加は 3 件:

- `order_limit_vault_expires.json` … `vaultAddress` / `expiresAfter` の連結
  （`0x01` マーカー + 20 バイト、`0x00` マーカー + 8 バイト big-endian）を Rust 側で検証するため、
  **両方が null でない**唯一の fixture。
- `order_expires_only.json` … `expiresAfter` のみ。`vaultAddress` なしでは vault マーカーが `0x00`
  1 バイトだけになり、連結は
  `msgpack(action) ‖ nonce(8B BE) ‖ 0x00 ‖ 0x00 ‖ expiresAfter(8B BE)` になる
  （`_l1.js:28-38`。vault の 20 バイトは入らず、`expiresAfter` があるので `0x00` マーカーは入る）。
- `cancel_large_oid.json` … msgpack の整数符号化の境界（下記「msgpack の整数・文字列の扱い」参照）。

## fixture の各フィールド

| フィールド | 意味 |
| --- | --- |
| `name` | ケース名（ファイル名から `.json` を除いたもの） |
| `sdk` | 生成に使った SDK 名とバージョン。`{"name": "@nktkas/hyperliquid", "version": "0.33.3"}` |
| `network` | `"testnet"`。transport の `isTestnet = true` を意味し、phantom agent の `source` が `"b"`、user-signed の `hyperliquidChain` が `"Testnet"` になる |
| `private_key_hex` | 署名に使ったテスト専用秘密鍵 |
| `address` | 署名者のアドレス（小文字）。**復元アドレスがこれと一致することを検証済み** |
| `is_agent` | 署名鍵が agent（API ウォレット）かどうか。`true` でも**署名方式は同じ L1 phantom agent**（後述） |
| `vault_address` | `createL1ActionHash` に渡された vault アドレス。無指定なら `null` |
| `expires_after` | `createL1ActionHash` に渡された有効期限（ms）。無指定なら `null` |
| `nonce` | 固定 nonce（`Date.now()` は使っていない）。L1 では signed payload の `nonce`、user-signed では `action.nonce`（`approveAgent`）または `action.time`（`usdSend`）に入る |
| `action` | **SDK が transport へ渡した action オブジェクトそのもの**（SDK が `canonicalize` 済み。キー順も SDK のスキーマ順） |
| `msgpack_hex` | `msgpack(action)` のバイト列。**SDK の公開 API から取得できないため全 fixture で `null`**（後述） |
| `payload_hex` | keccak256 の入力（`action` の msgpack + nonce + vault + expires）。**同じく `null`** |
| `connection_id_hex` | **L1 action のみ**。phantom agent の `connectionId` = `createL1ActionHash(...)` の戻り値（`@nktkas/hyperliquid/signing` の公開関数）。user-signed action では `null` |
| `digest_hex` | 実際に ECDSA で署名される EIP-712 digest。**SDK の公開 API から取得できないため全 fixture で `null`** |
| `signature_hex` | `r ‖ s ‖ v`（65 バイト、`v` は 27/28 の 1 バイト）。**このツールが定義した表現**であり SDK の出力そのものではない（SDK/HL API は `{r, s, v}` のオブジェクトを送る） |
| `signature` | SDK が返した署名コンポーネント。`v` は **27 または 28**（0/1 ではない） |

金額・数量・価格は **SDK へ渡した文字列のまま**になっている（丸めなし）。ただし SDK は
`UnsignedDecimal` スキーマで「先頭の余分な 0・末尾の 0」を落とす正規化を行うため
（`esm/api/_schemas.js` の `normalizeDecimalString`）、fixture は正規化後も同じ値になる文字列だけを使い、
生成時に「入力文字列 == action の値」を assert している。`.5` や `1.2000` のような入力は
SDK 側で書き換わるので fixture には入れていない。

### 署名バイト列の並び

- HL API / SDK の payload は `{ action, signature: { r, s, v }, nonce, vaultAddress?, expiresAfter? }` で、
  署名は **オブジェクト**として送られる（バイト列の連結は存在しない）。
- `signature_hex` は本ツールが定義した `r(32B) ‖ s(32B) ‖ v(1B)` の連結（`v` は 27/28）。
  `v ‖ r ‖ s` ではない。
- `s` は **low-s 正規化済み**（EIP-2）。viem の `sign()` が `lowS: true` で署名している。

## SDK から直接取得できた値と、そうでない値

| 値 | 取得元 | 状態 |
| --- | --- | --- |
| `action`（canonicalize 済み） | `IRequestTransport.request("exchange", payload)` に SDK が渡した payload（`esm/api/exchange/_methods/_base/_shell.js:30-35`）。transport 差し替えは公開インターフェース `IRequestTransport`（`esm/transport/_base.d.ts`） | **取得できた**（加工なしでそのまま記録） |
| `signature` / `signature_hex` | 同じ payload の `signature` | **取得できた** |
| `nonce` / `vault_address` / `expires_after` | 同じ payload のフィールド | **取得できた** |
| `connection_id_hex` | 公開関数 `createL1ActionHash()`（`esm/signing/mod.js:6`） | **取得できた（L1 action のみ）** |
| `msgpack_hex` | — | **取得できない**（下記） |
| `payload_hex` | — | **取得できない**（下記） |
| `digest_hex` | — | **取得できない**（下記） |

### `msgpack_hex` / `payload_hex` が取得できない理由

`@nktkas/hyperliquid/signing` の公開エクスポートは
`AbstractWalletError, getWalletAddress, getWalletChainId, canonicalize, createL1ActionHash,
signL1Action, signUserSignedAction, signMultiSigL1, signMultiSigUserSigned` のみ
（`esm/signing/mod.js`）。msgpack エンコーダは内部依存 `@std/msgpack` を
`esm/_deps/jsr.io/@std/msgpack/1.0.3/encode.js` から直接 import しており、
`package.json` の `exports` にも `_deps` は公開されていない。`_l1.js` の `createL1ActionHash` の内部で
`encodeMsgpack(adjust(action))` として呼ばれるだけで、バイト列を返す公開関数は無い。

辿った経路: `signL1Action`（公開）→ `createL1ActionHash`（公開。戻り値は keccak256 ハッシュのみ）
→ `encodeMsgpack`（`esm/_deps/.../encode.js`、非公開）。

### `digest_hex` が取得できない理由

`signL1Action` / `signUserSignedAction`（公開）は EIP-712 の typed data を組み立てて
`signTypedData`（`esm/signing/_abstractWallet.js:167`）へ渡すが、**戻り値は署名だけ**で digest は返さない。
digest を返す公開関数は存在しない。`createL1ActionHash` が返すのは phantom agent の
`connectionId`（= EIP-712 message のフィールド値）であって、署名対象の digest ではない。
そのため `digest_hex` は全 fixture で `null` とし、**推測値・再計算値で埋めていない**。

なお README の「検証」節に、**viem で再構成した digest**（SDK の値ではない）を記録している。
Rust 側は署名一致（復元アドレス一致）で検証できるため、fixture の必須項目ではない。

## SDK ソースから確認した署名方式

すべてインストール済みの `tools/hl-fixture-gen/node_modules/@nktkas/hyperliquid/` を読んで確認した事実。

### 1. L1 action（phantom agent）の署名 — `esm/signing/_l1.js`

- `signL1Action`（114-148 行）が署名する EIP-712 typed data:

  | 項目 | 値 |
  | --- | --- |
  | domain.name | `"Exchange"` |
  | domain.version | `"1"` |
  | domain.chainId | `1337`（**固定**。testnet/mainnet で変わらない。`isTestnet` は message 側に入る） |
  | domain.verifyingContract | `0x0000000000000000000000000000000000000000` |
  | primaryType | `"Agent"` |
  | types.Agent | `[ {name: "source", type: "string"}, {name: "connectionId", type: "bytes32"} ]`（130-141 行） |
  | message.source | `"b"`（testnet）/ `"a"`（mainnet）（144 行） |
  | message.connectionId | `createL1ActionHash(...)` の戻り値（32 バイト hex） |

  → **`is_agent: true` の fixture でも、署名される構造はこれと同じ**。`is_agent` は署名に使う鍵の役割
  （master か agent か）を示すだけで、SDK の署名経路は master / agent で差がない。
  `order_limit_agent.json` の `connection_id_hex`（`0x1de09e5c…`）は、そのまま phantom agent の
  `message.connectionId` の値である。

- `createL1ActionHash`（24-40 行）が keccak256 に掛けるバイト列:

  ```
  keccak256( msgpack(adjust(action)) ‖ uint64_be(nonce) ‖ vault ‖ expires )
  vault   = [0x00]                        (vaultAddress なし)
          = [0x01] ‖ vaultAddress(20 bytes) (vaultAddress あり)
  expires = (なし)                          (expiresAfter === undefined)
          = [0x00] ‖ uint64_be(expiresAfter) (expiresAfter あり)
  ```

  28-38 行の実装そのまま。注意点:
  - vault のマーカーは**常に 1 バイト入る**（`0x00` または `0x01`）。
  - `expiresAfter` が無いときはマーカーすら入らない（`0x00` 1 バイトも入らない）。
  - 判定は `expiresAfter !== undefined`。`null` を渡すと「有効」と解釈され
    `[0x00] + uint64_be(0)` が入ってしまうので、未指定は `undefined` にすること。
  - nonce / expiresAfter は `DataView.setBigUint64` による **8 バイト big-endian**（60-64 行）。
  - ハッシュは `keccak_256`（`@noble/hashes/sha3.js`）。Ethereum の keccak256 であって SHA3-256 ではない。

### 2. User-signed action（`approveAgent` / `usdSend`） — `esm/signing/_userSigned.js`

- `signUserSignedAction`（64-78 行）の EIP-712 domain:

  | 項目 | 値 |
  | --- | --- |
  | domain.name | `"HyperliquidSignTransaction"` |
  | domain.version | `"1"` |
  | domain.chainId | `parseInt(action.signatureChainId)`（10 進変換。`"0x66eee"` → `421614`） |
  | domain.verifyingContract | `0x0000000000000000000000000000000000000000` |
  | primaryType | `Object.keys(types)[0]`（`"HyperliquidTransaction:ApproveAgent"` / `"HyperliquidTransaction:UsdSend"`） |
  | message | `action` オブジェクトそのもの |

- types は SDK の公開定数 `ApproveAgentTypes` / `UsdSendTypes`
  （`esm/api/exchange/_methods/approveAgent.js:53-72`、`usdSend.js:46-65`）:
  - `HyperliquidTransaction:ApproveAgent`: `hyperliquidChain(string), agentAddress(address), agentName(string), nonce(uint64)`
  - `HyperliquidTransaction:UsdSend`: `hyperliquidChain(string), destination(string), amount(string), time(uint64)`
    注意: `destination` は `address` ではなく **`string`**。`amount` も **`string`**。
- `executeUserSignedAction`（`esm/api/exchange/_methods/_base/execute.js:84-129`）が
  `type`, `signatureChainId`, `hyperliquidChain` を先頭に付与し、
  `"nonce"` フィールドを持つ型（approveAgent）は `nonce`、
  持たない型（usdSend）は `time` に transport の nonce を入れる。
- `signatureChainId` は `config.signatureChainId` があればそれを使い、無ければウォレットの
  chainId（viem のローカルアカウントでは `"0x1"`）にフォールバックする（`execute.js:141-148`）。
  本ツールは HL の testnet 値 **`"0x66eee"`（Arbitrum Sepolia = 421614）** を明示的に渡している
  （SDK 自身のサンプルも同じ値: `esm/signing/_userSigned.js:25`）。
  `signatureChainId` は **action の一部として EIP-712 で署名される**（domain.chainId と
  message の両方に効く）ので、Rust 側もこの値の扱いに注意。

### 3. EIP-712 の型解決・digest 計算はウォレット側（viem）

`signTypedData` は `types[primaryType]` に無いキーを message から落としてから
ウォレットに委譲する（`_abstractWallet.js:167-184`）。viem のローカルアカウントでは
`privateKeyToAccount(...).signTypedData()` → `viem/_esm/utils/signature/hashTypedData.js` →
`viem/_esm/accounts/utils/sign.js` の順で処理される。つまり
**EIP-712 の domain separator / struct hash の実装は viem**（`EIP712Domain` 型は viem が
`getTypesForEIP712Domain` で domain から導出する）。

### 4. action のキー順（canonicalize） — `esm/signing/_canonicalize.js`

msgpack の map はキー順が結果に影響するため、SDK は送信前に action をスキーマ順へ並べ替える
（`reorderObject`、75-99 行）。

- キー順は valibot スキーマの定義順（各 `*Request.entries.action.entries`）。
- スキーマに無いキーがあると `CanonicalizeError`。
- 必須キーが欠けても `CanonicalizeError`。
- `optional` なキーは **値が無ければキーごと省略**（`null` は入れない）。
  例: `cancel` の `f`、`order` の `c` / `builder` は本 fixture の action に存在しない。
- ネストしたオブジェクト（`t.limit.tif` など）も同じ規則で並ぶ。

生成した `action` は SDK が canonicalize した後のオブジェクトをそのまま記録しているので、
Rust 側は **JSON のキー順をそのまま msgpack に書けばよい**（並べ替え不要）。
`order_limit_btc.json` の `action` は `type, orders, grouping` の順、`orders[0]` は
`a, b, p, s, r, t` の順、`t` は `limit` → `tif` の順で入っている。

### 5. msgpack の整数・文字列の扱い

- 前処理 `adjust()`（`esm/signing/_l1.js:45-59`）:
  - `undefined` のプロパティを削除する（エンコーダは `undefined` を扱えず例外になる）。
  - `Number.isInteger` かつ `value >= 0x100000000` または `value < -0x80000000` の数値を
    **`BigInt` に変換**する。理由: `@std/msgpack` はその範囲の number を float64 で書いてしまうため。
- エンコーダ `esm/_deps/jsr.io/@std/msgpack/1.0.3/encode.js` の実際の挙動:
  - 整数 (`encodeNumber`, 49-106 行): 非整数は **float64 (`0xcb`)**。
    正の整数は最小表現 — `<= 0x7f` は positive fixint、`< 2^8` は `0xcc`、`< 2^16` は `0xcd`、
    `< 2^32` は `0xce`。`>= 2^32` は float64 になるが、上記 `adjust()` により
    BigInt 化されるので実際には `0xcf` (uint64) になる。
    負の整数は negative fixint / `0xd0` (int8) / `0xd1` (int16) / `0xd2` (int32)、
    それ未満は float64。
  - `bigint` (130-149 行): 負は `0xd3` (int64)、非負は `0xcf` (uint64)。64 ビットを超えると例外。
  - **境界の注意**: 非負の値でも `adjust()` で BigInt 化されたものは **`0xcf` (uint64)** になり、
    `0xd3` (int64) にはならない（`0xd3` は負の bigint のみ）。
    また `[2^31, 2^32)` の整数は `number` のままなので最小表現の **`0xce` (uint32)** になる。
    つまり `2^32` を境に `0xce` → `0xcf` へ切り替わり、いずれも 8 バイト/4 バイトの big-endian。
    `cancel_large_oid.json`（`o: 4294967297` = 2^32 + 1）はこの `0xcf` 表現を固定するための fixture で、
    Rust 側が別の表現（`0xce` や `0xd3` や float64）で書くと
    `connection_id_hex` と署名が一致しなくなる。
  - `string` (150-177 行): UTF-8。長さ `< 32` は fixstr (`0xa0 | len`)、`< 256` は `0xd9` (str8)、
    `< 65536` は `0xda` (str16)、それ以上は `0xdb` (str32)。
  - `null` は `0xc0`、`false` は `0xc2`、`true` は `0xc3`。
  - 配列・map のヘッダは長さに応じて fixarray/fixmap または `0xdc`/`0xdd`/`0xde`/`0xdf`。
  - map のキーは `Object.entries` の順（= insertion order）。キー順が変わればバイト列が変わる。
- したがって、**文字列フィールド（価格 `p` / 数量 `s` / `amount` など）は必ず msgpack の str として
  エンコードされる**。数値として送ってはいけない。asset ID や oid のような整数は int、
  nonce は action 内ではなく payload 末尾の uint64 に入る。
- `leverage: 10` のような整数は fixint (`0x0a`) 1 バイトになる。

### 6. 署名の決定性・`v` の値

- SDK は EIP-712 のハッシュ計算をウォレットに委譲する（前述）。本ツールのウォレットは viem の
  ローカルアカウントで、`viem/_esm/accounts/utils/sign.js` が
  `@noble/curves/secp256k1` の `sign()` を `lowS: true` / `extraEntropy: false`（既定値。
  `setSignEntropy` を呼ばない限り `false`）で呼ぶ。
  → **RFC 6979 の決定的署名**であり、`s` は low-s に正規化される。
- `v` は `recovery ? 28 : 27` として **27 / 28 で返る**（同ファイル）。SDK の `parseSignature`
  （`esm/signing/_abstractWallet.js:14-30`）も受け取った `v` が 0/1 なら +27 して 27/28 に正規化し、
  それ以外は例外にする。生成した 9 fixture の `signature.v` は 27 が 7 件、28 が 2 件で、
  0/1 は現れない。
- 実際に `pnpm generate` を 2 回（別プロセスで）実行し、`signature_hex` を含む出力が
  **バイト単位で完全一致**することを `diff -r` で確認済み。
  さらに `generate.mjs` 自身が「1 プロセス内で 2 回生成して全 fixture が一致すること」を
  assert しているので、`pnpm generate` が成功するだけで決定性が確認される。

## 検証（復元アドレス一致）

`generate.mjs` は各 fixture について毎回次を検証し、失敗すれば非 0 で終了する。

1. SDK の公開関数でもう一度署名し、transport から捕獲した署名と **完全一致**することを確認
   （L1 は `signL1Action`、user-signed は `signUserSignedAction`）。
   → 記録した `action` / `nonce` / `vaultAddress` / `expiresAfter` が本当に署名対象だったことの裏取り。
2. `getWalletAddress()`（SDK 公開関数）が `address` と一致することを確認。
3. **viem で EIP-712 typed data を再構成**し、`hashTypedData()` → `recoverAddress({hash, signature})`
   で復元したアドレスが `address` と一致することを確認（下記の表が実測値）。

typed data の再構成は viem の API（`hashTypedData` / `recoverAddress`）で行っており、
SDK の内部関数の直接 import はしていない。`viem@2.56.8` の `recoverAddress` は **async**（`await` が必要）。

### 検証結果（2026-09-19 実行、全 11 件一致）

`digest` 列は **SDK の値ではなく、上記の再構成で viem が計算した値**。fixture の `digest_hex` は
`null` のまま（前節参照）で、ここには記録としてのみ残す。Rust 側の cross-check 用の参考値。

| fixture | 再構成 digest（SDK 値ではない） | 復元アドレス | `address` と一致 |
| --- | --- | --- | --- |
| `order_limit_btc.json` | `0x8083f20d41b0bffac1de40d2b1c64135424320907c8e590e118162d909b64cb3` | `0x70997970c51812dc3a010c7d01b50e0d17dc79c8` | ✅ |
| `order_market_ioc_eth.json` | `0x559680b91caf029d5b6f424b4c32d0be114612e287dad34530660d4ec7174307` | `0x70997970c51812dc3a010c7d01b50e0d17dc79c8` | ✅ |
| `cancel.json` | `0x4782a518499246a93043a3777a35c5c470833c998e6799201df74347263ba859` | `0x70997970c51812dc3a010c7d01b50e0d17dc79c8` | ✅ |
| `cancel_large_oid.json` | `0x3d6bb741f3c852a4d209a2b77ccfd6e1fd0007f65646ccaf7c1af814b63170a1` | `0x70997970c51812dc3a010c7d01b50e0d17dc79c8` | ✅ |
| `cancel_by_cloid.json` | `0xf54b1a6e3b8ca2f767df60fa1f1712794b613f1a23fe61ad2cf4eec853ce8d94` | `0x70997970c51812dc3a010c7d01b50e0d17dc79c8` | ✅ |
| `update_leverage.json` | `0x6a56b626efe882eca8995a2e4b6d515489fbb1cc93b1f2ccec0c194057262d10` | `0x70997970c51812dc3a010c7d01b50e0d17dc79c8` | ✅ |
| `order_limit_vault_expires.json` | `0x617edc9f54aafaac167d1f2989eccda8934b2e06e65693ff729265f00def96da` | `0x70997970c51812dc3a010c7d01b50e0d17dc79c8` | ✅ |
| `order_expires_only.json` | `0xc0a14615a2a3d0a90720284cfa1b87842eccacb5c76034abd4c1545c6caa39a1` | `0x70997970c51812dc3a010c7d01b50e0d17dc79c8` | ✅ |
| `approve_agent.json` | `0x1815f2d17a37917551f3de984cf495384ae9c9b65da19202d275422db61bc69f` | `0x70997970c51812dc3a010c7d01b50e0d17dc79c8` | ✅ |
| `usd_send.json` | `0xc83eead0693f221a6a25cbe66acd652c15be17d16c1cc76c4e98e972057a1188` | `0x70997970c51812dc3a010c7d01b50e0d17dc79c8` | ✅ |
| `order_limit_agent.json` | `0x7d1c7c7445df14e57f56be3747f75a1121d1ed5def4953aae20371a4b58a921c` | `0x3c44cdddb6a900fa2b585dd299e03d12fa4293bc` | ✅ |

`order_limit_agent.json` の復元アドレスは agent のアドレス（`0x3c44…`）で、`address` と一致する。

## `createL1ActionHash` の入力依存性（公開 API で実測）

`order_limit_btc.json` の action を固定し、入力だけ変えて `createL1ActionHash()` を呼んだ結果
（`generate.mjs` が毎回出力する）。

| 条件 | nonce | vaultAddress | expiresAfter | 結果 |
| --- | --- | --- | --- | --- |
| base | 1758000000000 | null | null | `0x3b3f5c5508dc8e8c59185f767bb5ba7ac0bcdf74993d62f2e48658d9d969dcd8` |
| nonce + 1 | 1758000000001 | null | null | `0x7e072e6400662515658a80a0f013e83fc6738152c62eb7d2fdf63f06bd5258b9` |
| vaultAddress 付き | 1758000000000 | `0x15d34aaf54267db7d7c367839aaf71a00a2c6a65` | null | `0x49bb8722d79c31186763ca631f5172e39edd352d3126353ed7ddfdb780fa74aa` |
| expiresAfter 付き | 1758000000000 | null | 1758000600000 | `0x0e39ff686d7a151ceb360c9451a04e75de3cce1285ef1c59db7bb35da534cd42` |
| 両方 | 1758000000000 | `0x15d34aaf54267db7d7c367839aaf71a00a2c6a65` | 1758000600000 | `0x76f984c0917d42c183f9fed62cfaf81a3e847ffff3f77d725b2e773f0eba5a07` |
| action のキー順を入れ替え（`orders, grouping, type`） | 1758000000000 | null | null | `0x1b8cde8f870fbd0f838f105f5e6033a6257b92ab9fdd1d311315c5ae336f0662` |

- base の値は `order_limit_btc.json` の `connection_id_hex` と一致する。
- **nonce / vaultAddress / expiresAfter / action のキー順のいずれを変えてもハッシュは変わる。**
  特に最後の行は「キー順が違うと同じ action でもハッシュが変わる」ことの実測（canonicalize が必要な理由）。

## perp の asset index は testnet と mainnet で異なる

`action` の `a` / `asset` は perp の asset index。**mainnet と testnet で並びが違う。**

- mainnet: `meta.universe` は `0:BTC, 1:ETH, 2:ATOM, 3:MATIC, 4:DYDX, 5:SOL`
- **testnet: `0:SOL, 1:APT, 2:ATOM, 3:BTC, 4:ETH, 5:MATIC`** → BTC は `3`、ETH は `4`

SDK の `SymbolConverter`（`@nktkas/hyperliquid/utils`）と `meta`（`@nktkas/hyperliquid/api/info`）で
実際に確認した値:

```
isTestnet=true  → BTC assetId=3, szDecimals=5 / ETH assetId=4, szDecimals=4
isTestnet=false → BTC assetId=0, szDecimals=5 / ETH assetId=1, szDecimals=4
```

本 fixture は `network: "testnet"` なので **testnet の値（BTC=3, ETH=4）** をそのまま使っている。
asset index は固定値ではなく **`meta.universe` の並びから解決する必要がある**
（生成スクリプト側も、決定的な出力にするため解決結果を定数として持っている）。
mainnet 基準で BTC=0 / ETH=1 と決め打ちすると testnet では誤った index になる。

## Rust 側への注意: user-signed の 2 fixture は L1 とは別経路

`approve_agent.json` と `usd_send.json` は **L1 action ではなく user-signed EIP-712 action** である
（SDK では `executeUserSignedAction` を通る）。したがって:

- この 2 つには **connectionId が存在しない**ため `connection_id_hex` は `null`。
  phantom agent の action ハッシュを計算しても、その値は署名に一切使われない（入れると誤誘導になる）。
- この 2 つは `createL1ActionHash` / phantom agent の digest では検証できない。検証には次の
  EIP-712 typed data が必要（`esm/signing/_userSigned.js:64-78` と
  `esm/api/exchange/_methods/approveAgent.js:53-72` / `usdSend.js:46-65`）:

  | 項目 | `approve_agent.json` | `usd_send.json` |
  | --- | --- | --- |
  | domain.name | `"HyperliquidSignTransaction"` | 同左 |
  | domain.version | `"1"` | 同左 |
  | domain.chainId | `421614`（`action.signatureChainId` = `0x66eee` を 10 進変換） | 同左 |
  | domain.verifyingContract | `0x0000000000000000000000000000000000000000` | 同左 |
  | primaryType | `"HyperliquidTransaction:ApproveAgent"` | `"HyperliquidTransaction:UsdSend"` |
  | フィールド順 | `hyperliquidChain(string), agentAddress(address), agentName(string), nonce(uint64)` | `hyperliquidChain(string), destination(string), amount(string), time(uint64)` |

  構造体ハッシュは **上の表のフィールド順**（= SDK の型定義順）で計算する。`action` のキー順
  （`type` が先頭など）とは異なる点に注意。`destination` は `address` ではなく `string`、
  `amount` も `string` として扱う。
- 本 README の「検証」節の再構成 digest は、この 2 件についてもこの typed data で計算した値である。

`crates/hl-sign/tests/fixtures.rs` は現状すべての fixture を L1 として扱うため
（`connection_id_hex` を必須とし `sign_action` で再署名する）、この 2 件では分岐が必要。

## 未確認・注意点

- `digest_hex` / `msgpack_hex` / `payload_hex` は SDK の公開 API から取得できないため全 fixture で
  `null`。値の正しさは Rust 側が `action` + `nonce` から自分で計算して、`signature_hex` の
  復元アドレス一致（または README の再構成 digest との一致）で確認する。
- `signature_hex` の `r ‖ s ‖ v` という並びは本ツールの定義であり、SDK / HL API の仕様ではない。
  HL API は `{r, s, v}` オブジェクトを送る。
- README の再構成 digest は viem による計算であり、SDK が返した値ではない。
  （SDK の署名経路もウォレット = viem に委譲しているため辻褄は合うが、SDK の公開 API の戻り値では
  ないことに変わりはない。）
- `approveAgent` / `usdSend` の `signatureChainId` は `"0x66eee"` を明示的に渡している。
  この値は SDK の既定ではなく（既定はウォレットの chainId）、SDK 自身のサンプルと
  Hyperliquid の testnet 値に合わせたもの。mainnet では `"0xa4b1"` になる。
- `agentName` は 16 文字以内というスキーマ制約がある（`esm/api/exchange/_methods/approveAgent.js:19-26`）。
  本 fixture は `"pp-test-agent"`（13 文字）。
- cloid は `0x` + 32 hex（34 文字）でなければならない（`esm/api/_schemas.js` の `Cloid`）。
- `order` の grouping は省略時に既定値 `"na"` が入るため、`action` に必ず現れる。
- 本ツールは HL API へ一切リクエストを送らない（asset index の確認は別途 1 回だけ行った）。
- 既定の `pnpm` 設定に `minimumReleaseAge` があるため、依存の追加時は指定バージョンの公開から
  一定時間が経過している必要がある。現在の固定バージョンは解決済み。
