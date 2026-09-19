#!/usr/bin/env node
/**
 * Hyperliquid 署名テストベクタ（fixture）生成スクリプト。
 *
 * 公式相当の TypeScript SDK `@nktkas/hyperliquid` を使い、Rust 実装 (`crates/hl-sign`)
 * の digest / 署名検証に使う fixture を `crates/hl-sign/tests/fixtures/` へ書き出す。
 *
 * 方針:
 * - 署名・ハッシュ・action の正規化はすべて SDK の公開APIに実行させる。
 * - 中間値 (msgpack バイト列 / EIP-712 digest / 署名対象 payload) が SDK の公開APIから
 *   取得できない場合は null を書き、README に理由を記録する（捏造しない）。
 * - nonce は固定値。Date.now() は使わない。出力は決定的。
 *
 * 実行: pnpm generate
 */

import { mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { fileURLToPath } from "node:url";

import {
  ApproveAgentTypes,
  UsdSendTypes,
  approveAgent,
  cancel,
  cancelByCloid,
  order,
  updateLeverage,
  usdSend,
} from "@nktkas/hyperliquid/api/exchange";
import {
  createL1ActionHash,
  getWalletAddress,
  signL1Action,
  signUserSignedAction,
} from "@nktkas/hyperliquid/signing";
import { hashTypedData, recoverAddress } from "viem";
import { privateKeyToAccount } from "viem/accounts";

// ============================================================
// 定数
// ============================================================

const SDK_NAME = "@nktkas/hyperliquid";
const SDK_VERSION = JSON.parse(
  readFileSync(new URL("./node_modules/@nktkas/hyperliquid/package.json", import.meta.url), "utf8"),
).version;

const OUT_DIR = fileURLToPath(new URL("../../crates/hl-sign/tests/fixtures/", import.meta.url));

/** fixture の network。transport.isTestnet = true、phantom agent の source = "b"。 */
const NETWORK = "testnet";
const IS_TESTNET = true;

/**
 * User-signed EIP-712 の domain.chainId に入る値（action.signatureChainId）。
 * 0x66eee = 421614 = Arbitrum Sepolia。SDK 自身のサンプルもこの値を使っている
 * (node_modules/@nktkas/hyperliquid/esm/signing/_userSigned.js:25)。
 */
const SIGNATURE_CHAIN_ID = "0x66eee";

const ZERO_ADDRESS = "0x0000000000000000000000000000000000000000";

/**
 * テスト専用の公開鍵（Hardhat / Anvil の既定アカウント。#0 は使わない）。
 * 実資金の鍵ではない。README にも明記する。
 */
const KEY_MASTER = "0x59c6995e998f97a5a0044966f0945389dc9e86dae88c7a8412f4603b6b78690d"; // anvil #1
const KEY_AGENT = "0x5de4111afa1a4b94908f83103eb1f1706367c2e68ca870fc3fb9a804cdab365a"; // anvil #2

const ADDRESS_AGENT = "0x3c44cdddb6a900fa2b585dd299e03d12fa4293bc"; // anvil #2
const ADDRESS_DESTINATION = "0x90f79bf6eb2c4f870365e785982e1f101e93b906"; // anvil #3
const ADDRESS_VAULT = "0x15d34aaf54267db7d7c367839aaf71a00a2c6a65"; // anvil #4

/**
 * perp の asset index。testnet の meta.universe は mainnet と並びが異なる
 * （testnet: 0:SOL, 1:APT, 2:ATOM, 3:BTC, 4:ETH / mainnet: 0:BTC, 1:ETH）。
 * 本fixtureは network="testnet" なので testnet の値をそのまま使う。
 * 検証方法は README を参照。
 */
const ASSET = { BTC: 3, ETH: 4 };

const CLOID = "0x0102030405060708090a0b0c0d0e0f10";

// ============================================================
// 失敗時に原因を残すための assert
// ============================================================

function assert(condition, message) {
  if (!condition) throw new Error(`assertion failed: ${message}`);
}

function assertEqual(actual, expected, label) {
  assert(
    actual === expected,
    `${label}: expected ${JSON.stringify(expected)}, got ${JSON.stringify(actual)}`,
  );
}

// ============================================================
// ケース定義
// ============================================================

/** テスト用 transport。SDK が transport へ渡した payload をそのまま捕獲する。 */
function makeCapturingTransport() {
  const state = { payload: null, endpoint: null };
  const transport = {
    isTestnet: IS_TESTNET,
    async request(endpoint, payload) {
      if (endpoint !== "exchange") {
        throw new Error(`unexpected endpoint: ${endpoint}`);
      }
      assert(state.payload === null, "transport.request called twice for one case");
      state.endpoint = endpoint;
      state.payload = payload;
      // assertSuccessResponse を通る形（error 形状ではない）のダミー応答。
      return { status: "ok", response: { type: "default" } };
    },
  };
  return { transport, state };
}

const CASES = [
  {
    file: "order_limit_btc.json",
    kind: "l1",
    signer: "master",
    nonce: 1758000000000,
    decimals: [
      ["orders.0.p", "95000"],
      ["orders.0.s", "0.01"],
    ],
    exec: (config) =>
      order(config, {
        orders: [
          { a: ASSET.BTC, b: true, p: "95000", s: "0.01", r: false, t: { limit: { tif: "Gtc" } } },
        ],
        grouping: "na",
      }),
  },
  {
    file: "order_market_ioc_eth.json",
    kind: "l1",
    signer: "master",
    nonce: 1758000001000,
    decimals: [
      ["orders.0.p", "3210.5"],
      ["orders.0.s", "1.25"],
    ],
    exec: (config) =>
      order(config, {
        orders: [
          { a: ASSET.ETH, b: true, p: "3210.5", s: "1.25", r: false, t: { limit: { tif: "Ioc" } } },
        ],
        grouping: "na",
      }),
  },
  {
    file: "cancel.json",
    kind: "l1",
    signer: "master",
    nonce: 1758000002000,
    decimals: [],
    exec: (config) => cancel(config, { cancels: [{ a: ASSET.BTC, o: 123456789 }] }),
  },
  {
    // msgpack の整数符号化の境界用。2^32 以上の整数は SDK の adjust() が BigInt へ広げ、
    // エンコーダは最小表現ではなく 8 バイト big-endian の uint64 (0xcf) で書く。
    file: "cancel_large_oid.json",
    kind: "l1",
    signer: "master",
    nonce: 1758000009000,
    decimals: [],
    exec: (config) => cancel(config, { cancels: [{ a: ASSET.BTC, o: 4294967297 }] }),
  },
  {
    file: "cancel_by_cloid.json",
    kind: "l1",
    signer: "master",
    nonce: 1758000003000,
    decimals: [],
    exec: (config) => cancelByCloid(config, { cancels: [{ asset: ASSET.ETH, cloid: CLOID }] }),
  },
  {
    file: "update_leverage.json",
    kind: "l1",
    signer: "master",
    nonce: 1758000004000,
    decimals: [],
    exec: (config) => updateLeverage(config, { asset: ASSET.BTC, isCross: true, leverage: 10 }),
  },
  {
    file: "order_limit_vault_expires.json",
    kind: "l1",
    signer: "master",
    nonce: 1758000005000,
    vaultAddress: ADDRESS_VAULT,
    expiresAfter: 1758000600000,
    decimals: [
      ["orders.0.p", "95000.5"],
      ["orders.0.s", "0.25"],
    ],
    exec: (config) =>
      order(
        config,
        {
          orders: [
            {
              a: ASSET.BTC,
              b: false,
              p: "95000.5",
              s: "0.25",
              r: false,
              t: { limit: { tif: "Gtc" } },
            },
          ],
          grouping: "na",
        },
        { vaultAddress: ADDRESS_VAULT, expiresAfter: 1758000600000 },
      ),
  },
  {
    // expiresAfter のみ（vaultAddress なし）の連結確認用:
    // msgpack(action) || nonce(8B) || 0x00 || 0x00 || expiresAfter(8B BE)
    file: "order_expires_only.json",
    kind: "l1",
    signer: "master",
    nonce: 1758000010000,
    expiresAfter: 1758000700000,
    decimals: [
      ["orders.0.p", "95002"],
      ["orders.0.s", "0.03"],
    ],
    exec: (config) =>
      order(
        config,
        {
          orders: [
            {
              a: ASSET.BTC,
              b: true,
              p: "95002",
              s: "0.03",
              r: false,
              t: { limit: { tif: "Gtc" } },
            },
          ],
          grouping: "na",
        },
        { expiresAfter: 1758000700000 },
      ),
  },
  {
    file: "approve_agent.json",
    kind: "user",
    signer: "master",
    nonce: 1758000006000,
    types: ApproveAgentTypes,
    nonceField: "nonce",
    decimals: [],
    exec: (config) =>
      approveAgent(config, { agentAddress: ADDRESS_AGENT, agentName: "pp-test-agent" }),
  },
  {
    file: "usd_send.json",
    kind: "user",
    signer: "master",
    nonce: 1758000007000,
    types: UsdSendTypes,
    nonceField: "time",
    decimals: [["amount", "12.5"]],
    exec: (config) => usdSend(config, { destination: ADDRESS_DESTINATION, amount: "12.5" }),
  },
  {
    file: "order_limit_agent.json",
    kind: "l1",
    signer: "agent",
    nonce: 1758000008000,
    decimals: [
      ["orders.0.p", "95001"],
      ["orders.0.s", "0.02"],
    ],
    exec: (config) =>
      order(config, {
        orders: [
          { a: ASSET.BTC, b: true, p: "95001", s: "0.02", r: false, t: { limit: { tif: "Gtc" } } },
        ],
        grouping: "na",
      }),
  },
];

// ============================================================
// 検証（自分の再構成。fixture の値ではない）
// ============================================================

const EXCHANGE_DOMAIN = {
  name: "Exchange",
  version: "1",
  chainId: 1337,
  verifyingContract: ZERO_ADDRESS,
};

const AGENT_TYPES = {
  Agent: [
    { name: "source", type: "string" },
    { name: "connectionId", type: "bytes32" },
  ],
};

/** signL1Action / createL1ActionHash が使う phantom agent の typed data を再構成する。 */
function l1TypedData(connectionId) {
  return {
    domain: EXCHANGE_DOMAIN,
    types: AGENT_TYPES,
    primaryType: "Agent",
    message: { source: IS_TESTNET ? "b" : "a", connectionId },
  };
}

/** signUserSignedAction が使う typed data を再構成する。types は SDK の公開定数。 */
function userTypedData(action, types) {
  return {
    domain: {
      name: "HyperliquidSignTransaction",
      version: "1",
      chainId: Number.parseInt(action.signatureChainId, 16),
      verifyingContract: ZERO_ADDRESS,
    },
    types,
    primaryType: Object.keys(types)[0],
    message: action,
  };
}

function getPath(object, path) {
  return path.split(".").reduce((value, key) => value?.[key], object);
}

function toSignatureHex(signature) {
  return `0x${signature.r.slice(2)}${signature.s.slice(2)}${signature.v
    .toString(16)
    .padStart(2, "0")}`;
}

// ============================================================
// 生成
// ============================================================

async function generateCase(caseDef) {
  const key = caseDef.signer === "agent" ? KEY_AGENT : KEY_MASTER;
  const wallet = privateKeyToAccount(key);
  const { transport, state } = makeCapturingTransport();
  const config = {
    transport,
    wallet,
    nonceManager: () => caseDef.nonce,
    signatureChainId: SIGNATURE_CHAIN_ID,
  };

  await caseDef.exec(config);

  const payload = state.payload;
  assert(payload !== null, `${caseDef.file}: transport was not called`);
  const action = payload.action;
  const signature = payload.signature;
  const nonce = payload.nonce;

  const vaultAddress = payload.vaultAddress ?? caseDef.vaultAddress ?? null;
  const expiresAfter = payload.expiresAfter ?? caseDef.expiresAfter ?? null;

  // --- SDK が出した値の検証 --------------------------------------------
  assertEqual(nonce, caseDef.nonce, `${caseDef.file}: payload.nonce`);
  assertEqual(vaultAddress, caseDef.vaultAddress ?? null, `${caseDef.file}: payload.vaultAddress`);
  assertEqual(expiresAfter, caseDef.expiresAfter ?? null, `${caseDef.file}: payload.expiresAfter`);

  // 渡した十進文字列が丸められていないこと（SDK は先頭/末尾の 0 だけを落とす）。
  for (const [path, expected] of caseDef.decimals) {
    assertEqual(getPath(action, path), expected, `${caseDef.file}: action.${path}`);
  }

  if (caseDef.kind === "l1") {
    assertEqual(action.type === undefined, false, `${caseDef.file}: action.type`);
  } else {
    assertEqual(action.signatureChainId, SIGNATURE_CHAIN_ID, `${caseDef.file}: signatureChainId`);
    assertEqual(action.hyperliquidChain, "Testnet", `${caseDef.file}: hyperliquidChain`);
    assertEqual(
      action[caseDef.nonceField],
      caseDef.nonce,
      `${caseDef.file}: action.${caseDef.nonceField}`,
    );
  }

  // --- connectionId（phantom agent）------------------------------------
  const connectionIdHex =
    caseDef.kind === "l1"
      ? createL1ActionHash({
          action,
          nonce,
          vaultAddress: vaultAddress ?? undefined,
          expiresAfter: expiresAfter ?? undefined,
        })
      : null;

  // --- 同じ入力で SDK の公開署名関数をもう一度呼び、署名が一致することを確認 ---
  const resign =
    caseDef.kind === "l1"
      ? await signL1Action({
          wallet,
          action,
          nonce,
          isTestnet: IS_TESTNET,
          vaultAddress: vaultAddress ?? undefined,
          expiresAfter: expiresAfter ?? undefined,
        })
      : await signUserSignedAction({ wallet, action, types: caseDef.types });
  assertEqual(
    toSignatureHex(resign),
    toSignatureHex(signature),
    `${caseDef.file}: SDK re-sign produced a different signature`,
  );

  // --- EIP-712 digest を再構成し、復元アドレスが署名者と一致することを確認 --
  const typedData =
    caseDef.kind === "l1"
      ? l1TypedData(connectionIdHex)
      : userTypedData(action, caseDef.types);
  const digestHex = hashTypedData(typedData);
  const signatureHex = toSignatureHex(signature);
  const recovered = await recoverAddress({ hash: digestHex, signature: signatureHex });
  const address = wallet.address.toLowerCase();
  const sdkAddress = await getWalletAddress(wallet);

  assertEqual(sdkAddress, address, `${caseDef.file}: SDK getWalletAddress`);
  assertEqual(recovered.toLowerCase(), address, `${caseDef.file}: recovered address`);

  const fixture = {
    name: caseDef.file.replace(/\.json$/, ""),
    sdk: { name: SDK_NAME, version: SDK_VERSION },
    network: NETWORK,
    private_key_hex: key,
    address,
    is_agent: caseDef.signer === "agent",
    vault_address: vaultAddress,
    expires_after: expiresAfter,
    nonce,
    action,
    // SDK の公開APIからは取得できない中間値。README「取得できなかった値」を参照。
    msgpack_hex: null,
    payload_hex: null,
    // L1 action のみ: phantom agent の connectionId = createL1ActionHash(...) の戻り値。
    connection_id_hex: connectionIdHex,
    digest_hex: null,
    signature_hex: signatureHex,
    signature: { r: signature.r, s: signature.s, v: signature.v },
  };

  return { fixture, digestHex, recovered: recovered.toLowerCase() };
}

async function generateAll() {
  const results = [];
  for (const caseDef of CASES) {
    results.push({ caseDef, ...(await generateCase(caseDef)) });
  }
  return results;
}

function serialize(fixture) {
  return `${JSON.stringify(fixture, null, 2)}\n`;
}

// ============================================================
// createL1ActionHash の入力依存性チェック（SDK 公開APIのみ使用）
// ============================================================

function hashSensitivity(baseAction) {
  const baseNonce = CASES[0].nonce;
  const reordered = {
    orders: baseAction.orders,
    grouping: baseAction.grouping,
    type: baseAction.type,
  };
  const rows = [
    { label: "base", args: { action: baseAction, nonce: baseNonce } },
    { label: "nonce+1", args: { action: baseAction, nonce: baseNonce + 1 } },
    {
      label: "vaultAddress",
      args: { action: baseAction, nonce: baseNonce, vaultAddress: ADDRESS_VAULT },
    },
    {
      label: "expiresAfter",
      args: { action: baseAction, nonce: baseNonce, expiresAfter: 1758000600000 },
    },
    {
      label: "vault+expires",
      args: {
        action: baseAction,
        nonce: baseNonce,
        vaultAddress: ADDRESS_VAULT,
        expiresAfter: 1758000600000,
      },
    },
    { label: "reordered keys", args: { action: reordered, nonce: baseNonce } },
  ];
  return rows.map(({ label, args }) => ({
    label,
    nonce: args.nonce,
    vault: args.vaultAddress ?? null,
    expires: args.expiresAfter ?? null,
    hash: createL1ActionHash(args),
  }));
}

// ============================================================
// main
// ============================================================

const first = await generateAll();
const second = await generateAll();

for (let i = 0; i < first.length; i += 1) {
  assertEqual(
    serialize(second[i].fixture),
    serialize(first[i].fixture),
    `determinism: ${first[i].caseDef.file}`,
  );
}

mkdirSync(OUT_DIR, { recursive: true });
for (const { caseDef, fixture } of first) {
  writeFileSync(join(OUT_DIR, caseDef.file), serialize(fixture));
}

console.log(`sdk: ${SDK_NAME}@${SDK_VERSION}`);
console.log(`network: ${NETWORK} (isTestnet=${IS_TESTNET}, signatureChainId=${SIGNATURE_CHAIN_ID})`);
console.log(`out: ${OUT_DIR}`);
console.log("");
const header = [
  "fixture".padEnd(30),
  "kind".padEnd(5),
  "nonce".padEnd(14),
  "connection_id".padEnd(15),
  "digest".padEnd(7),
  "msgpack",
];
console.log(header.join(" "));
for (const { caseDef, fixture } of first) {
  console.log(
    [
      caseDef.file.padEnd(30),
      caseDef.kind.padEnd(5),
      String(fixture.nonce).padEnd(14),
      (fixture.connection_id_hex === null ? "null" : "yes").padEnd(15),
      (fixture.digest_hex === null ? "null" : "yes").padEnd(7),
      fixture.msgpack_hex === null ? "null" : "yes",
    ].join(" "),
  );
}
console.log("");
console.log("verification (reconstructed outside the SDK public API, not stored in fixtures):");
for (const { caseDef, fixture, digestHex, recovered } of first) {
  console.log(
    `  ${caseDef.file.padEnd(30)} digest=${digestHex} recovered=${recovered} == address: ${recovered === fixture.address}`,
  );
}
console.log("");
console.log("createL1ActionHash input sensitivity (public SDK API, base = order_limit_btc):");
for (const row of hashSensitivity(first[0].fixture.action)) {
  console.log(
    `  ${row.label.padEnd(15)} nonce=${String(row.nonce).padEnd(14)} vault=${row.vault ?? "null"} expires=${row.expires ?? "null"} -> ${row.hash}`,
  );
}
console.log("");
console.log(`${first.length} fixtures written; two in-process runs produced identical output.`);
