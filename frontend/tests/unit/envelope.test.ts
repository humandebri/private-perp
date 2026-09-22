import { describe, expect, it } from 'vitest'
import { EnvelopeClient, envelopeAad } from '../../src/client/envelope'

describe('HPKE envelope', () => {
  it('round-trips enc || ciphertext with the contract AAD', async () => {
    const sender = await EnvelopeClient.fromSeed(new Uint8Array(32).fill(1))
    const recipient = await EnvelopeClient.fromSeed(new Uint8Array(32).fill(2))
    const aad = envelopeAad(
      'testnet',
      new Uint8Array([1, 2, 3]),
      'submit_order',
      new Uint8Array([4, 5]),
      new Uint8Array(32).fill(6),
      1234n,
    )
    const plaintext = new TextEncoder().encode('private order')

    const envelope = await sender.seal(recipient.publicKey, aad, plaintext)

    expect(envelope.length).toBeGreaterThan(32)
    await expect(recipient.open(aad, envelope)).resolves.toEqual(plaintext)
    await expect(recipient.open(new Uint8Array(aad.length), envelope)).rejects.toThrow()
  })
})
