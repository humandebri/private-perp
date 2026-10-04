# hl-fixture-gen

> Fixture design and verification record from September 19, 2026. Counts and measurements below describe that run; later Rust tests also cover user-signed actions and Spot conversion fixtures.

The Hyperliquid signature test vector (fixture) used for **digest / signature verification** in Rust implementation (`crates/hl-sign`),
A tool that generates equivalent to the official TypeScript SDK and fixes it to `crates/hl-sign/tests/fixtures/`.

- SDK: **`@nktkas/hyperliquid` version `0.33.3` (exact fixed)**
- Wallet / Verification: **`viem` version `2.56.8` (exact fixed)**
- Output location: `crates/hl-sign/tests/fixtures/*.json` (1 file 1 object)

## Generation command

```sh
cd tools/hl-fixture-gen && pnpm install --frozen-lockfile && pnpm generate
```

- `pnpm` is `packageManager: pnpm@12.4.2` (same as frontend). Can be resolved with corepack.
- `node_modules/` is not committed (the `node_modules/` in the `.gitignore` directly below the repository is effective).
  `pnpm-lock.yaml` is subject to commit.
- `pnpm-workspace.yaml` contains `storeDir: .pnpm-store`. pnpm is by default the same as the project
  To create a store in the drive route, if there is no store, `pnpm install` will be installed directly under the repository
  Creates an untracked `.pnpm-store/` directory. Close `tools/hl-fixture-gen/.pnpm-store/` with this specification.
  (Ignored by `.gitignore`).
- Network is **not required** (except `pnpm install`). The generation process is just calling the SDK locally,
  Don't send requests anywhere. transport has been replaced with a capture stub.

## Using private key (for test only)

**No real-fund keys are used.** These are widely published Hardhat / Anvil development accounts. Never fund them or use them to protect real assets.

| a use | private key | an address |
| --- | --- | --- |
| master (signature holder) | `0x59c6995e998f97a5a0044966f0945389dc9e86dae88c7a8412f4603b6b78690d` | `0x70997970c51812dc3a010c7d01b50e0d17dc79c8` |
| agent (the signer of the agent signature / agentAddress of `approveAgent`) | `0x5de4111afa1a4b94908f83103eb1f1706367c2e68ca870fc3fb9a804cdab365a` | `0x3c44cdddb6a900fa2b585dd299e03d12fa4293bc` |
| `usdSend` destination | (Do not use the key) | `0x90f79bf6eb2c4f870365e785982e1f101e93b906` |
| `vaultAddress` | (Do not use the key) | `0x15d34aaf54267db7d7c367839aaf71a00a2c6a65` |

The master key is Anvil Account #1. Anvil #0 can be used in examples like `approveAgent`, but
This fixture does not use `address`. `address` should be **lowercase** (to match the return value of the SDK's `getWalletAddress()`).

## Fixture list

Generates the following in `crates/hl-sign/tests/fixtures/`: `network` is all `"testnet"`.

| a file | signature method | Content | `connection_id_hex` | `signature.v` |
| --- | --- | --- | --- | --- |
| `order_limit_btc.json` | L1 (phantom agent) | GTC order limit for BTC perp (`tif: "Gtc"`). | `0x3b3f5c5508dc8e8c59185f767bb5ba7ac0bcdf74993d62f2e48658d9d969dcd8` | 28 |
| `order_market_ioc_eth.json` | L1 | IOC for ETH perp (limit price at the slippage bound, `tif: "Ioc"`). | `0x31de2f241b8ae7fd0cf73ecc7e2fa42ba94867f23ceb93dce582fcaad4bead83` | 27 |
| `cancel.json` | L1 | oid specified cancellation | `0x1cd673d1288bdf9f2367b83f147c107b6ef056f7ab2065ff123c4d9ed2ead3cc` | 27 |
| `cancel_large_oid.json` | L1 | cancellation where oid is greater than 2^32 (`o: 4294967297`) (**extra case outside the requirement**, for msgpack integer encoding boundary) | `0x9c8632695c288dbe0a21cf5d24a31f0ae18b50c1efb17d18443b4dc13b75b740` | 28 |
| `cancel_by_cloid.json` | L1 | cloid specified cancellation | `0x6cb641b58354f44d668751e03483ac0e7e3e60a6ab914da635f21975dda39393` | 27 |
| `update_leverage.json` | L1 | Leverage update (cross, 10x) | `0x3fa6e66ded7cb0fc2b73342cadb0ca11c21e9a126a5816ac14ec857c800488b1` | 28 |
| `order_limit_vault_expires.json` | L1 | BTC order with `vaultAddress` + `expiresAfter` (**additional cases not required**) | `0x14763ac3c5f9ef33529a5965eef796bbcc597876bf45fb991f073eb56b4512de` | 27 |
| `order_expires_only.json` | L1 | BTC limit orders only with `expiresAfter` (without `vaultAddress`) (**additional cases not required, for linking verification**) | `0x8b6cef43542256268b4b29c869b6491c1af5b1d5a0c0090e3d733c22e9f6fa89` | 27 |
| `approve_agent.json` | User-signed EIP-712 | Agent approval (master signature, `is_agent: false`) | `null` (there is no connectionId in user-signed) | 27 |
| `usd_send.json` | User-signed EIP-712 | USDC transfer (`usdSend`) | `null` | 27 |
| `order_limit_agent.json` | L1 | BTC limit order signed with Agent key (`is_agent: true`) | `0x1de09e5c979dbef93d74a3b50a93fe413b1729453c6c1345554d7f5cc8976dd0` | 27 |

There are 3 additional cases for the 8 required cases:

- `order_limit_vault_expires.json` ... concatenation of `vaultAddress` / `expiresAfter`
  (`0x01` marker + 20 bytes, `0x00` marker + 8 bytes big-endian) to verify on the Rust side,
  **Both are not null** The only fixture.
- `order_expires_only.json` ... only `expiresAfter`. Without `vaultAddress`, the vault marker is one `0x00` byte. The concatenation is
  `msgpack(action) ‖ nonce(8B BE) ‖ 0x00 ‖ 0x00 ‖ expiresAfter(8B BE)`
  (`_l1.js:28-38`). No 20-byte vault address is included; the expiry marker is present because `expiresAfter` is set.
- `cancel_large_oid.json` ... msgpack integer encoding boundary (see below for "handling of integers and strings in msgpack").

## Each field of the fixture

| a field | Meaning |
| --- | --- |
| `name` | Case name (excluding `.json` from the filename) |
| `sdk` | The name and version of the SDK used for generation. `{"name": "@nktkas/hyperliquid", "version": "0.33.3"}` |
| `network` | `"testnet"` means that `transport`'s `isTestnet = true`, and the `source` of the phantom agent becomes `"b"`, and the `hyperliquidChain` of the user-signed becomes `"Testnet"` |
| `private_key_hex` | Private key used for testing for signature |
| `address` | Signature holder's address (lowercase). **The recovered address has been verified to match this one** |
| `is_agent` | Whether signing key is an agent (API wallet). Even if `true`, the **signature method is the same L1 phantom agent** (as mentioned later). |
| `vault_address` | The vault address passed to `createL1ActionHash`. If not specified, it is `null`. |
| `expires_after` | The expiration date (ms) passed to `createL1ActionHash`. If not specified, it is `null`. |
| `nonce` | Fixed nonce; `Date.now()` is not used. L1 actions use the signed payload nonce; user-signed actions use `action.nonce` (`approveAgent`) or `action.time` (`usdSend`). |
| `action` | **The action object itself that the SDK passed to transport** (SDK has been canonicalized. The key order is also in the SDK schema order) |
| `msgpack_hex` | Bytes of `msgpack(action)`. **Not exposed by the SDK public API; `null` in every fixture.** |
| `payload_hex` | The keccak256 input (action msgpack + nonce + vault + expiry); also `null`. |
| `connection_id_hex` | **Only L1 action**. The `connectionId` of the phantom agent is the return value of `createL1ActionHash(...)` (the public function `@nktkas/hyperliquid/signing`). For user-signed actions, it is `null`. |
| `digest_hex` | The EIP-712 digest actually signed with ECDSA. **Because it cannot be obtained from the public API of the SDK, it is `null` in all fixtures.** |
| `signature_hex` | `r ‖ s ‖ v` (65 bytes, `v` is 1 byte of 27/28). **This is an expression defined by this tool**, not the output of the SDK (SDK/HL API sends an object `{r, s, v}`). |
| `signature` | The signature component returned by the SDK. `v` is **27 or 28** (not 0/1) |

The amount, quantity, and price remain as the string passed to the **SDK** (without rounding). However, the SDK is
To normalize by removing "leading extra 0s and trailing 0s" in the `UnsignedDecimal` schema.
(`esm/api/_schemas.js` `normalizeDecimalString`), fixture uses only strings that are the same value after normalization,
Asserts that "input string == action value" when generated. Input such as `.5` or `1.2000` is
It will be changed on the SDK side, so I don't put it into the fixture.

### Signature byte order

- The payload of the HL API / SDK is `{ action, signature: { r, s, v }, nonce, vaultAddress?, expiresAfter? }`,
  The signature is sent as an **object** (there is no concatenation of byte sequences).
- `signature_hex` is the concatenation of `r(32B) ‖ s(32B) ‖ v(1B)` defined by this tool (`v` is 27/28).
  `v ‖ r ‖ s` is not.
- `s` is **low-s normalized** (EIP-2). viem's `sign()` is signed with `lowS: true`.

## Values that were directly obtained from the SDK and values that were not

| cost | Source of acquisition | State |
| --- | --- | --- |
| `action` (canonicalized) | The payload passed by the SDK to `IRequestTransport.request("exchange", payload)` (`esm/api/exchange/_methods/_base/_shell.js:30-35`). The transport replacement is the public interface `IRequestTransport` (`esm/transport/_base.d.ts`). | **Obtained** (recorded as is without processing) |
| `signature` / `signature_hex` | The same payload's `signature` | **Obtained** |
| `nonce` / `vault_address` / `expires_after` | Fields with the same payload | **Obtained** |
| `connection_id_hex` | Public function `createL1ActionHash()` (esm/signing/mod.js:6) | **Acquired (only L1 action)** |
| `msgpack_hex` | Bytes of `msgpack(action)`. **Not exposed by the SDK public API; `null` in every fixture.** |
| `payload_hex` | The keccak256 input (action msgpack + nonce + vault + expiry); also `null`. |
| `digest_hex` | — | **Cannot be obtained** (below) |

### Reasons why `msgpack_hex` / `payload_hex` cannot be obtained

The public exports of `@nktkas/hyperliquid/signing` are `AbstractWalletError`, `getWalletAddress`, `getWalletChainId`, `canonicalize`, `createL1ActionHash`, `signL1Action`, `signUserSignedAction`, `signMultiSigL1`, and `signMultiSigUserSigned` (`esm/signing/mod.js`). The internal msgpack encoder comes from `esm/_deps/jsr.io/@std/msgpack/1.0.3/encode.js`; `_deps` is not exported by `package.json`. There is no public function that returns the bytes produced by `encodeMsgpack(adjust(action))` inside `_l1.js`.

The call path is `signL1Action` (public) → `createL1ActionHash` (public; returns only the keccak256 hash) → `encodeMsgpack` (private).

### Why you can't get `digest_hex`

`signL1Action` / `signUserSignedAction` (public) constructs the typed data of EIP-712
Pass to `signTypedData` (`esm/signing/_abstractWallet.js:167`), but **only the signature is returned** and the digest is not returned.
There is no public function that returns digest. `createL1ActionHash` returns the phantom agent's
It is not the digest of the signing payload, but `connectionId` (= the field value of the EIP-712 message).
Therefore, `digest_hex` is set to `null` for all fixtures and is **not filled with estimated or recalculated values**.

Additionally, the README section of the "Verification" section records a **digest** (not the SDK value) reconstructed with **viem**.
The Rust side can be verified by signature matching (address recovery matching), so it is not a required fixture.

## Signature method verified from SDK source

The fact that all of the installed `tools/hl-fixture-gen/node_modules/@nktkas/hyperliquid/` were read and verified.

### 1. Signature of L1 action (phantom agent) - `esm/signing/_l1.js`

- `signL1Action` (lines 114-148) signs EIP-712 typed data:

  | an item | cost |
  | --- | --- |
  | domain.name | `"Exchange"` |
  | domain.version | `"1"` |
  | domain.chainId | `1337` (**fixed**. Does not change on testnet/mainnet. `isTestnet` goes into the message side) |
  | domain.verifyingContract | `0x0000000000000000000000000000000000000000` |
  | primaryType | `"Agent"` |
  | types.Agent | `[ {name: "source", type: "string"}, {name: "connectionId", type: "bytes32"} ]` (130-141 lines) |
  | message.source | "b" (testnet) / "a" (mainnet) (144 lines) |
  | message.connectionId | The return value of `createL1ActionHash(...)` (32-byte hex) |

  → Even with a fixture with `is_agent: true`, the structure that gets signed is the same as this one. `is_agent` is the role of the key used for signing.
  Just indicating whether it is master or agent, the SDK signature path is the same for master / agent.
  The `connection_id_hex` (`0x1de09e5c...`) in `order_limit_agent.json` is the same as the phantom agent's
  The value of `message.connectionId`.

- `createL1ActionHash` (24-40 lines) byte sequence passed to keccak256:

  ```
  keccak256( msgpack(adjust(action)) ‖ uint64_be(nonce) ‖ vault ‖ expires )
  vault = [0x00]                        (no vaultAddress)
          = [0x01] ‖ vaultAddress(20 bytes) (vaultAddress present)
  expires = (none)                          (expiresAfter === undefined)
          = [0x00] ‖ uint64_be(expiresAfter) (expiresAfter is present)
  ```

  This follows the implementation at lines 28–38. Notes:
  - The vault marker **always takes 1 byte** (`0x00` or `0x01`).
  - If `expiresAfter` is not present, the marker will not even be inserted (no `0x00` byte will be inserted).
  - The condition is `expiresAfter !== undefined`. If `null` is passed, it is interpreted as "valid".
    Since `[0x00] + uint64_be(0)` will be inserted, any unspecified value will be set to `undefined`.
  - nonce / expiresAfter is **8 bytes big-endian** (60-64 lines) by `DataView.setBigUint64`.
  - The hash is `keccak_256` (`@noble/hashes/sha3.js`). It is keccak256 from Ethereum and not SHA3-256.

### 2. User-signed action（`approveAgent` / `usdSend`） — `esm/signing/_userSigned.js`

- EIP-712 domain of `signUserSignedAction` (64-78 lines):

  | an item | cost |
  | --- | --- |
  | domain.name | `"HyperliquidSignTransaction"` |
  | domain.version | `"1"` |
  | domain.chainId | `parseInt(action.signatureChainId)` (base-10 conversion. `"0x66eee"` → `421614`) |
  | domain.verifyingContract | `0x0000000000000000000000000000000000000000` |
  | primaryType | `Object.keys(types)[0]`（`"HyperliquidTransaction:ApproveAgent"` / `"HyperliquidTransaction:UsdSend"`） |
  | message | `action` the object itself |

- types are the public constants of the SDK `ApproveAgentTypes` / `UsdSendTypes`
  （`esm/api/exchange/_methods/approveAgent.js:53-72`、`usdSend.js:46-65`）:
  - `HyperliquidTransaction:ApproveAgent`: `hyperliquidChain(string), agentAddress(address), agentName(string), nonce(uint64)`
  - `HyperliquidTransaction:UsdSend`: `hyperliquidChain(string), destination(string), amount(string), time(uint64)`
    Note: `destination` is not `address` but **`string`**. `amount` is also **`string`**.
- `executeUserSignedAction` (`esm/api/exchange/_methods/_base/execute.js:84-129`) assigns `type`, `signatureChainId`, and `hyperliquidChain`, then puts the transport nonce in `nonce` for actions such as `approveAgent`, or `time` for actions such as `usdSend`.
- The SDK uses `config.signatureChainId` when provided, otherwise the wallet chain ID (`"0x1"` for a viem local account; `execute.js:141-148`). This tool explicitly supplies **`"0x66eee"` (Arbitrum Sepolia = 421614)**, also shown in the SDK sample (`esm/signing/_userSigned.js:25`). Rust must use the same value when deriving the signed EIP-712 domain and action context.

### 3. Type resolution and digest calculation of EIP-712 are on the wallet side (viem)

`signTypedData` drops keys that are not in `types[primaryType]` from the message
Delegate to wallet (`_abstractWallet.js:167-184`). In the local account of viem
`privateKeyToAccount(...).signTypedData()` → `viem/_esm/utils/signature/hashTypedData.js` →
Processed in order of `viem/_esm/accounts/utils/sign.js`.in other words
**The implementation of domain separator / struct hash for EIP-712 is viem** (the `EIP712Domain` type is viem
`getTypesForEIP712Domain` to derive the domain from the domain).

### 4. The order of keys in action (canonicalize) — `esm/signing/_canonicalize.js`

Since the order of keys in the map of msgpack affects the result, the SDK reorders the action in schema order before sending.
(`reorderObject`, lines 75-99).

- The key order is defined in the order of the valibot schema (each `*Request.entries.action.entries`).
- If there is a key that is not in the schema, it will be `CanonicalizeError`.
- Even if a required key is missing, a `CanonicalizeError` is returned.
- Optional keys are **omitted for each key if the value is not provided** (do not include `null`).
  For example, `f` in `cancel` and `c` / `builder` in `order` do not exist in the action of this fixture.
- Nested objects (like `t.limit.tif`) are also sorted according to the same rules.

The generated `action` records the object as-is after the SDK canonicalizes it, so
On the Rust side, simply write the JSON keys in the same order as they appear in msgpack (no need to reorder).
`order_limit_btc.json`'s `action` is in the order of `type, orders, grouping`, `orders[0]` is
`a, b, p, s, r, t` in order, `t` is in order of `limit` → `tif`.

### 5. Handling integers and strings in msgpack

- Preprocessing `adjust()` (esm/signing/_l1.js:45-59):
  - Remove properties that are `undefined` (encoding will throw an exception if it cannot handle `undefined`).
  - Numbers that are `Number.isInteger` and `value >= 0x100000000` or `value < -0x80000000`
    **Convert to `BigInt`**. Reason: `@std/msgpack` will write the number in that range as float64.
- Actual behavior of the encoder `esm/_deps/jsr.io/@std/msgpack/1.0.3/encode.js`:
  - Integers (`encodeNumber`, 49-106 lines): Non-integers are **float64 (`0xcb`)**.
    Positive integers are represented by the smallest expression — `<= 0x7f` is a positive fixint, `< 2^8` is `0xcc`, `< 2^16` is `0xcd`,
    `< 2^32` is `0xce`. `>= 2^32` becomes float64, but due to the above `adjust()`
    It will be converted to BigInt, so it will actually become `0xcf` (uint64).
    Negative integers are negative fixint / `0xd0` (int8) / `0xd1` (int16) / `0xd2` (int32),
    Anything less than that is float64.
  - `bigint` (130-149 lines): Negative is `0xd3` (int64), positive is `0xcf` (uint64). Exception if exceeds 64 bits.
  - **Boundary Note**: Even non-negative values converted to BigInt with `adjust()` will become **`0xcf` (uint64)**,
    It does not use `0xd3` (int64); `0xd3` is used only for negative bigints.
    Also, the integer `[2^31, 2^32)` remains as `number`, so it becomes the minimum representation **`0xce` (uint32)**.
    In other words, at the boundary of `2^32`, it switches from `0xce` to `0xcf`, using 4 and 8 bytes respectively, both big-endian.
    `cancel_large_oid.json` (`o: 4294967297` = 2^32 + 1) is a fixture to fix this `0xcf` representation,
    If Rust writes it in a different representation (`0xce`, `0xd3`, or float64)
    `connection_id_hex` and signature will no longer match.
  - `string` (150-177 lines): UTF-8. Length `< 32` is fixstr (`0xa0 | len`), `< 256` is `0xd9` (str8),
    `< 65536` is `0xda` (str16), and above is `0xdb` (str32).
  - `null` is `0xc0`, `false` is `0xc2`, and `true` is `0xc3`.
  - The header of arrays/maps is fixarray/fixmap or `0xdc`/`0xdd`/`0xde`/`0xdf` depending on the length.
  - The keys of the map are in the order of `Object.entries` (= insertion order). If the order of keys changes, the byte array changes.
- Therefore, **string fields (price `p` / quantity `s` / `amount` etc.) must be msgpack's str type.
  encoded.** Cannot be sent as a number. Integers such as asset ID or oid are int,
  nonce is not included in action but in the uint64 at the end of payload.
- Integers like `leverage: 10` become 1 byte of fixint (`0x0a`).

### 6. Signature determinism and `v`

- The SDK delegates EIP-712 hashing to the wallet. This tool uses a viem local account; `viem/_esm/accounts/utils/sign.js` calls `@noble/curves/secp256k1` with `lowS: true` and `extraEntropy: false` by default (unless `setSignEntropy` changes it). The result is a deterministic RFC 6979 signature with low-s normalization.
- viem returns `v` as **27 or 28** (`recovery ? 28 : 27`). The SDK parser (`esm/signing/_abstractWallet.js:14-30`) converts 0/1 to 27/28 by adding 27 and rejects unsupported values. In the original 9-fixture run, 7 signatures had `v = 27` and 2 had `v = 28`; none had 0/1.
- Two separate `pnpm generate` runs matched byte for byte, including `signature_hex`, using `diff -r`. `generate.mjs` also generates twice within one process and asserts that every fixture matches, so a successful generation checks determinism.

## Verification (matching recovery address)

`generate.mjs` verifies the next step for each fixture and terminates with non-zero if it fails.

1. Sign the SDK's public functions again and verify that the signature captured from transport is **completely consistent** with the signature.
   (L1 is `signL1Action`, user-signed is `signUserSignedAction`).
   → Proof that the recorded `action` / `nonce` / `vaultAddress` / `expiresAfter` were indeed signing payloads.
2. Verify that `getWalletAddress()` (SDK public function) matches `address`.
3. **Reconstruct EIP-712 typed data with viem**, and `hashTypedData()` → `recoverAddress({hash, signature})`
   Verify that the recovered address matches `address` (the table below is the actual values).

The reconstruction of typed data is done through the viem API (`hashTypedData` / `recoverAddress`),
We do not directly import internal SDK functions. `viem@2.56.8`'s `recoverAddress` is **async** (requires `await`).

### Verification results (2026-09-19 execution, all 11 matches)

The `digest` column is **not the value of the SDK, but the value viem calculated in the above reconstruction**. fixture's `digest_hex` is
It will be left as `null` (see previous section) for the purpose of recording only here. Reference value for cross-check on the Rust side.

| fixture | Reconstructed digest (not SDK value) | Recovery address | Matches `address` |
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

The recovery address of `order_limit_agent.json` is the agent's address (`0x3c44...`), which matches `address`.

## Input dependency of `createL1ActionHash` (measured in the public API)

Fixed the action in `order_limit_btc.json` and only changed the input, resulting in calling `createL1ActionHash()`
(`generate.mjs` is outputed each time).

| Condition | nonce | vaultAddress | expiresAfter | Result |
| --- | --- | --- | --- | --- |
| base | 1758000000000 | null | null | `0x3b3f5c5508dc8e8c59185f767bb5ba7ac0bcdf74993d62f2e48658d9d969dcd8` |
| nonce + 1 | 1758000000001 | null | null | `0x7e072e6400662515658a80a0f013e83fc6738152c62eb7d2fdf63f06bd5258b9` |
| with vaultAddress | 1758000000000 | `0x15d34aaf54267db7d7c367839aaf71a00a2c6a65` | null | `0x49bb8722d79c31186763ca631f5172e39edd352d3126353ed7ddfdb780fa74aa` |
| expiresAfter attached | 1758000000000 | null | 1758000600000 | `0x0e39ff686d7a151ceb360c9451a04e75de3cce1285ef1c59db7bb35da534cd42` |
| Both | 1758000000000 | `0x15d34aaf54267db7d7c367839aaf71a00a2c6a65` | 1758000600000 | `0x76f984c0917d42c183f9fed62cfaf81a3e847ffff3f77d725b2e773f0eba5a07` |
| Change the order of keys for action (`orders, grouping, type`) | 1758000000000 | null | null | `0x1b8cde8f870fbd0f838f105f5e6033a6257b92ab9fdd1d311315c5ae336f0662` |

- The base value matches the `connection_id_hex` in `order_limit_btc.json`.
- **Changing nonce, vaultAddress, expiresAfter, or the action key order changes the hash.**
  In particular, the last line is a real-world observation that "even if the order of keys is different, the hash changes for the same action" (the reason canonicalize is needed).

## The perp asset index is different on testnet and mainnet.

`a` / `asset` in `action` is the perp's asset index. **The order is different on mainnet and testnet.**

- mainnet: `meta.universe` is `0:BTC, 1:ETH, 2:ATOM, 3:MATIC, 4:DYDX, 5:SOL`
- **testnet: `0:SOL, 1:APT, 2:ATOM, 3:BTC, 4:ETH, 5:MATIC`** → BTC is `3`, ETH is `4`

With SDK's `SymbolConverter` ( `@nktkas/hyperliquid/utils`) and `meta` ( `@nktkas/hyperliquid/api/info`)
The value actually confirmed:

```
isTestnet=true  → BTC assetId=3, szDecimals=5 / ETH assetId=4, szDecimals=4
isTestnet=false → BTC assetId=0, szDecimals=5 / ETH assetId=1, szDecimals=4
```

This fixture is set to `network: "testnet"` so **it uses the testnet value (BTC=3, ETH=4) directly**.
The asset index needs to be resolved from the order of **`meta.universe`** rather than a fixed value.
(The generation script also holds the solution as a constant to make it a definitive output).
If you set BTC=0 / ETH=1 based on the mainnet standard, it will become the wrong index on the testnet.

## Attention on the Rust side: the 2 fixtures with user-signed are on a separate path from L1

`approve_agent.json` and `usd_send.json` are **user-signed EIP-712 actions**, not L1 actions.
(The SDK passes `executeUserSignedAction`). Therefore:

- Since these two do not have a **connectionId**, `connection_id_hex` is `null`.
  Even if you calculate the action hash of phantom agent, that value will not be used at all in signature (if you put it in, it will cause misleading induction).
- These two cannot be verified by the digest of `createL1ActionHash` / phantom agent. Verification requires the following
  EIP-712 typed data is required (`esm/signing/_userSigned.js:64-78` and
  `esm/api/exchange/_methods/approveAgent.js:53-72` / `usdSend.js:46-65`）:

  | an item | `approve_agent.json` | `usd_send.json` |
  | --- | --- | --- |
  | domain.name | `"HyperliquidSignTransaction"` | Same as left |
  | domain.version | `"1"` | Same as left |
  | domain.chainId | `421614` (convert `action.signatureChainId` = `0x66eee` to base 10) | Same as left |
  | domain.verifyingContract | `0x0000000000000000000000000000000000000000` | Same as left |
  | primaryType | `"HyperliquidTransaction:ApproveAgent"` | `"HyperliquidTransaction:UsdSend"` |
  | Field order | `hyperliquidChain(string), agentAddress(address), agentName(string), nonce(uint64)` | `hyperliquidChain(string), destination(string), amount(string), time(uint64)` |

  The struct hash is computed in **the field order of the table above** (i.e., the order of type definitions in SDK). This differs from the serialized `action` key order, which begins with `type`. `destination` is not `address` but `string`,
  `amount` is also treated as a `string`.
- The reconfigured digest of the “validation” section of this README is also the value computed with this typed data for these two items.

`crates/hl-sign/tests/fixtures.rs` currently treats all fixtures as L1
(`connection_id_hex` is required and re-signed with `sign_action`), this two cases require branching.

## Unconfirmed / Notes

- `digest_hex` / `msgpack_hex` / `payload_hex` cannot be obtained from the public API of the SDK, so in all fixtures
  `null`. The correctness of the value is calculated by the Rust side itself from `action` + `nonce`, and the `signature_hex` of
  Verify with the matching recovery address (or matching the reconfigured digest in README).
- The `r ‖ s ‖ v` sequence in `signature_hex` is a definition of this tool and is not a specification of the SDK / HL API.
  HL API sends `{r, s, v}` objects.
- The re-configuration digest of README is computed by viem and is not returned by the SDK.
  (Although the signature path of the SDK is also delegated to the wallet = viem, it is consistent, but the return value of the SDK's public API is
  There is no change.)
- The `signatureChainId` for `approveAgent` / `usdSend` is explicitly provided as `"0x66eee"`.
  This value is not the default of the SDK (the default is the wallet's chainId), but the SDK's own sample and
  It is adjusted to the testnet value of Hyperliquid. On the mainnet it becomes `"0xa4b1"`.
- `agentName` has a schema constraint of 16 characters or less (`esm/api/exchange/_methods/approveAgent.js:19-26`).
  This fixture is `"pp-test-agent"` (13 characters).
- cloid must be `0x` + 32 hex (34 characters) (the `Cloid` in `esm/api/_schemas.js`).
- The grouping of `order` will always appear in `action` because the default value `"na"` is provided when the grouping is omitted.
- This tool does not send any requests to the HL API (only one additional check of the asset index was performed).
- Since the `minimumReleaseAge` is defined in the default `pnpm` settings, when adding dependencies, it will be from the release of the specified version.
  A certain amount of time must have passed. The current fixed version is resolved.
