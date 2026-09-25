import { Principal } from '@icp-sdk/core/principal'
import { describe, expect, it } from 'vitest'
import {
  connectWallet,
  hexToBytes,
  signPersonalBytes,
  signTypedData,
  withdrawalTypedData,
} from '../../src/client/wallet'

describe('EIP-1193 wallet', () => {
  it('requests an account and signs exact typed data', async () => {
    const calls: Array<{ method: string; params?: unknown[] }> = []
    const provider = {
      request: async (args: { method: string; params?: unknown[] }) => {
        calls.push(args)
        if (args.method === 'eth_requestAccounts') return [`0x${'11'.repeat(20)}`]
        return `0x${'22'.repeat(65)}`
      },
    }
    const address = await connectWallet(provider)
    const data = JSON.stringify({ domain: {}, types: {}, primaryType: 'X', message: {} })
    expect(await signTypedData(address, data, provider)).toEqual(hexToBytes(`0x${'22'.repeat(65)}`))
    expect(calls).toEqual([
      { method: 'eth_requestAccounts' },
      { method: 'eth_signTypedData_v4', params: [address, data] },
    ])
  })

  it('builds a five-minute local withdrawal payload with exact integer strings', () => {
    const json = JSON.parse(
      withdrawalTypedData({
        address: `0x${'11'.repeat(20)}`,
        amount: 1n,
        nonce: 1000n,
        expiresAt: 301000n,
        canister: Principal.fromText('aaaaa-aa'),
      }),
    )
    expect(json.primaryType).toBe('PrivatePerpWithdrawal')
    expect(json.message).toMatchObject({
      amount: '1',
      nonce: '1000',
      expiresAt: '301000',
      network: 'local',
      asset: 'usdc',
      destination: `0x${'11'.repeat(20)}`,
    })
  })

  it('signs the exact 32-byte zero-fee consent digest', async () => {
    const calls: Array<{ method: string; params?: unknown[] }> = []
    const provider = {
      request: async (args: { method: string; params?: unknown[] }) => {
        calls.push(args)
        return `0x${'33'.repeat(65)}`
      },
    }
    const address = `0x${'11'.repeat(20)}`
    const digest = hexToBytes(`0x${'22'.repeat(32)}`)
    expect(await signPersonalBytes(address, digest, provider)).toEqual(
      hexToBytes(`0x${'33'.repeat(65)}`),
    )
    expect(calls).toEqual([{ method: 'personal_sign', params: [`0x${'22'.repeat(32)}`, address] }])
  })
})
