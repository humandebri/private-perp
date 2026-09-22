// HPKE封筒（RFC 9180: X25519 / HKDF-SHA256 / ChaCha20-Poly1305）。
//
// canister側（`crates/hpke-envelope`）と同じ`info`・`aad`を使い、要求は
// `enc || ciphertext`、応答は同じ形で返る。`aad`のエンコードはRust実装と
// バイト単位で一致させる（`envelope.test.ts`が固定値で検査する）。

import { Chacha20Poly1305 } from '@hpke/chacha20poly1305'
import { CipherSuite, DhkemX25519HkdfSha256, HkdfSha256 } from '@hpke/core'

/** 用途分離ラベル（`crates/hpke-envelope`の`ENVELOPE_INFO`と同一）。 */
export const ENVELOPE_INFO = new TextEncoder().encode('private-perp/envelope/v1')

const suite = new CipherSuite({
  kem: new DhkemX25519HkdfSha256(),
  kdf: new HkdfSha256(),
  aead: new Chacha20Poly1305(),
})

/** TypedArrayのview範囲だけを独立したArrayBufferへする。 */
function asArrayBuffer(bytes: Uint8Array): ArrayBuffer {
  return bytes.buffer.slice(bytes.byteOffset, bytes.byteOffset + bytes.byteLength) as ArrayBuffer
}

/** `request_id`（32バイト。単回使用。再送はcanisterが拒否する）。 */
export function newRequestId(): Uint8Array {
  return crypto.getRandomValues(new Uint8Array(32))
}

/** 部分を`(len as u32 BE) || bytes`で連結し、末尾に期限（u64 BE）を足す。 */
export function envelopeAad(
  network: string,
  canister: Uint8Array,
  method: string,
  caller: Uint8Array,
  requestId: Uint8Array,
  expiresAt: bigint,
): Uint8Array {
  const parts = [
    new TextEncoder().encode(network),
    canister,
    new TextEncoder().encode(method),
    caller,
    requestId,
  ]
  const length = parts.reduce((total, part) => total + 4 + part.length, 0) + 8
  const aad = new Uint8Array(length)
  const view = new DataView(aad.buffer)
  let offset = 0
  for (const part of parts) {
    view.setUint32(offset, part.length, false)
    offset += 4
    aad.set(part, offset)
    offset += part.length
  }
  view.setBigUint64(offset, expiresAt, false)
  return aad
}

/** 画面側の封筒クライアント（クライアント鍵はセッション中だけ保持する）。 */
export class EnvelopeClient {
  private constructor(
    private readonly privateKey: CryptoKey,
    /** 応答の復号に使う公開鍵（`client_public_key`として送る）。 */
    readonly publicKey: Uint8Array,
  ) {}

  /** クライアント鍵対を生成する（保存しない）。 */
  static async create(): Promise<EnvelopeClient> {
    const pair = await suite.kem.generateKeyPair()
    const publicKey = new Uint8Array(await suite.kem.serializePublicKey(pair.publicKey))
    return new EnvelopeClient(pair.privateKey, publicKey)
  }

  /** 決定論的なクライアント鍵（試験用）。 */
  static async fromSeed(seed: Uint8Array): Promise<EnvelopeClient> {
    const pair = await suite.kem.deriveKeyPair(seed)
    const publicKey = new Uint8Array(await suite.kem.serializePublicKey(pair.publicKey))
    return new EnvelopeClient(pair.privateKey, publicKey)
  }

  /** 平文をサーバ公開鍵へ封をする（`enc || ciphertext`）。 */
  async seal(
    serverPublicKey: Uint8Array,
    aad: Uint8Array,
    plaintext: Uint8Array,
  ): Promise<Uint8Array> {
    const recipient = await suite.kem.deserializePublicKey(serverPublicKey)
    const sender = await suite.createSenderContext({
      recipientPublicKey: recipient,
      info: asArrayBuffer(ENVELOPE_INFO),
    })
    const ciphertext = new Uint8Array(
      await sender.seal(asArrayBuffer(plaintext), asArrayBuffer(aad)),
    )
    const enc = new Uint8Array(sender.enc)
    const envelope = new Uint8Array(enc.length + ciphertext.length)
    envelope.set(enc, 0)
    envelope.set(ciphertext, enc.length)
    return envelope
  }

  /** 応答の封筒（`enc || ciphertext`）を開ける。 */
  async open(aad: Uint8Array, envelope: Uint8Array): Promise<Uint8Array> {
    if (envelope.length <= 32) throw new Error('封筒が短すぎます')
    const enc = envelope.slice(0, 32)
    const ciphertext = envelope.slice(32)
    const recipient = await suite.createRecipientContext({
      recipientKey: this.privateKey,
      enc: asArrayBuffer(enc),
      info: asArrayBuffer(ENVELOPE_INFO),
    })
    return new Uint8Array(await recipient.open(asArrayBuffer(ciphertext), asArrayBuffer(aad)))
  }
}
