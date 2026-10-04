import type { Principal } from '@icp-sdk/core/principal'

export type Eip1193Provider = {
  request(args: { method: string; params?: unknown[] }): Promise<unknown>
}

declare global {
  interface Window {
    ethereum?: Eip1193Provider
  }
}

export function hexToBytes(value: string, expected?: number): Uint8Array {
  const hex = value.startsWith('0x') ? value.slice(2) : value
  if (!/^[0-9a-f]*$/i.test(hex) || hex.length % 2 !== 0)
    throw new Error('Invalid hexadecimal value')
  const bytes = Uint8Array.from(hex.match(/.{2}/g)?.map((pair) => Number.parseInt(pair, 16)) ?? [])
  if (expected !== undefined && bytes.length !== expected)
    throw new Error(`${expected} bytes required`)
  return bytes
}

export function bytesToHex(value: Uint8Array | number[]): string {
  return `0x${[...value].map((byte) => byte.toString(16).padStart(2, '0')).join('')}`
}

export async function connectWallet(provider = window.ethereum): Promise<string> {
  if (!provider) throw new Error('MetaMask was not found')
  const accounts = await provider.request({ method: 'eth_requestAccounts' })
  const address = Array.isArray(accounts) ? accounts[0] : undefined
  if (typeof address !== 'string') throw new Error('Could not retrieve the wallet account')
  return bytesToHex(hexToBytes(address, 20)).toLowerCase()
}

export async function signTypedData(
  address: string,
  typedData: Uint8Array | string,
  provider = window.ethereum,
): Promise<Uint8Array> {
  if (!provider) throw new Error('MetaMask was not found')
  const json = typeof typedData === 'string' ? typedData : new TextDecoder().decode(typedData)
  JSON.parse(json)
  const signature = await provider.request({
    method: 'eth_signTypedData_v4',
    params: [address, json],
  })
  if (typeof signature !== 'string') throw new Error('Could not retrieve the signature')
  return hexToBytes(signature, 65)
}

export async function signPersonalBytes(
  address: string,
  message: Uint8Array,
  provider = window.ethereum,
): Promise<Uint8Array> {
  if (!provider) throw new Error('MetaMask was not found')
  const signature = await provider.request({
    method: 'personal_sign',
    params: [bytesToHex(message), address],
  })
  if (typeof signature !== 'string') throw new Error('Could not retrieve the signature')
  return hexToBytes(signature, 65)
}

export function withdrawalTypedData(args: {
  address: string
  amount: bigint
  nonce: bigint
  expiresAt: bigint
  canister: Principal
  network?: 'local' | 'testnet'
}): string {
  return JSON.stringify({
    domain: {
      name: 'private-perp',
      version: '1',
      chainId: 1,
      verifyingContract: '0x0000000000000000000000000000000000000000',
    },
    primaryType: 'PrivatePerpWithdrawal',
    types: {
      EIP712Domain: [
        { name: 'name', type: 'string' },
        { name: 'version', type: 'string' },
        { name: 'chainId', type: 'uint256' },
        { name: 'verifyingContract', type: 'address' },
      ],
      PrivatePerpWithdrawal: [
        { name: 'eoa', type: 'address' },
        { name: 'amount', type: 'uint64' },
        { name: 'asset', type: 'string' },
        { name: 'destination', type: 'string' },
        { name: 'network', type: 'string' },
        { name: 'nonce', type: 'uint64' },
        { name: 'expiresAt', type: 'uint64' },
        { name: 'canister', type: 'bytes' },
      ],
    },
    message: {
      eoa: args.address,
      amount: args.amount.toString(),
      asset: 'usdc',
      destination: args.address,
      network: args.network ?? 'local',
      nonce: args.nonce.toString(),
      expiresAt: args.expiresAt.toString(),
      canister: bytesToHex(args.canister.toUint8Array()),
    },
  })
}
