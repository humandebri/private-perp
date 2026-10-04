import { IDL } from '@icp-sdk/core/candid'
import { describe, expect, it } from 'vitest'
import { idlFactory } from '../../src/client/candid/funds_vault.did.js'
import { vaultPrivateCodec } from '../../src/client/candid-codec'
import { CanisterError } from '../../src/client/result'

describe('private response errors', () => {
  it('decodes journal contention from the generated canister contract', () => {
    const service = idlFactory({ IDL })
    const method = service._fields.find(([name]) => name === 'get_fund_status')
    expect(method).toBeDefined()
    const encoded = IDL.encode([method![1].retTypes[0]!], [{ Err: { JournalWriterBusy: null } }])
    expect(() => vaultPrivateCodec.fund(new Uint8Array(encoded))).toThrow(CanisterError)
    expect(() => vaultPrivateCodec.fund(new Uint8Array(encoded))).toThrow('JournalWriterBusy:')
  })
})
