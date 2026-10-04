import { beforeEach, describe, expect, it, vi } from 'vitest'
import { LocalGateway } from '../../src/client/gateway'
import { createClients, type CanisterClients } from '../../src/client/ic'
import { EnvelopeClient } from '../../src/client/envelope'
import { connectWallet, signTypedData } from '../../src/client/wallet'

vi.mock('../../src/client/ic', () => ({ createClients: vi.fn() }))
vi.mock('../../src/client/wallet', async (importOriginal) => ({
  ...(await importOriginal<typeof import('../../src/client/wallet')>()),
  connectWallet: vi.fn(),
  signTypedData: vi.fn(),
}))

const address = `0x${'11'.repeat(20)}`

function fixture(results: Array<{ Ok: object } | { Err: object }>) {
  let issued = 0
  const issue_challenge = vi.fn(async () => {
    issued++
    return {
      Ok: {
        challenge_id: Uint8Array.of(issued),
        typed_data: new TextEncoder().encode('{}'),
      },
    }
  })
  const open_session = vi.fn(async (_request: { challenge_id: Uint8Array }) => results.shift())
  vi.mocked(createClients).mockResolvedValue({
    config: { stage: 'local' },
    principal: {} as CanisterClients['principal'],
    vault: { issue_challenge, open_session },
  } as unknown as CanisterClients)
  return { gateway: new LocalGateway(), issue_challenge, open_session }
}

describe('login journal contention', () => {
  beforeEach(() => {
    vi.resetAllMocks()
    vi.stubGlobal('location', { origin: 'https://local.test' })
    vi.mocked(connectWallet).mockResolvedValue(address)
    vi.mocked(signTypedData).mockResolvedValue(new Uint8Array(65))
    vi.spyOn(EnvelopeClient, 'create').mockResolvedValue({} as EnvelopeClient)
  })

  it('stops on a writer conflict without requesting another signature', async () => {
    const session = { session_id: Uint8Array.of(9) }
    const { gateway, issue_challenge, open_session } = fixture([
      { Err: { JournalWriterBusy: null } },
      { Ok: session },
    ])
    await expect(gateway.login()).rejects.toMatchObject({ code: 'JournalWriterBusy' })
    expect(issue_challenge).toHaveBeenCalledTimes(1)
    expect(signTypedData).toHaveBeenCalledTimes(1)
    expect(open_session.mock.calls.map(([request]) => [...request.challenge_id])).toEqual([[1]])
    expect(connectWallet).toHaveBeenCalledTimes(1)
  })

  it('does not request another signature when the journal is stopped', async () => {
    const { gateway, issue_challenge } = fixture([{ Err: { PolicyUnavailable: null } }])
    await expect(gateway.login()).rejects.toThrow('PolicyUnavailable')
    expect(issue_challenge).toHaveBeenCalledTimes(1)
    expect(signTypedData).toHaveBeenCalledTimes(1)
  })
})
