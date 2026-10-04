import { useMemo, useState } from 'react'
import { LocalGateway, type LiveData } from '../client/gateway'
import { bytesToHex } from '../client/wallet'
import { variantName } from '../client/result'

const toMicros = (value: string) => {
  if (!/^\d+(\.\d{1,6})?$/.test(value))
    throw new Error('Enter USDC with no more than six decimal places')
  const [whole, fraction = ''] = value.split('.')
  return BigInt(whole) * 1_000_000n + BigInt(fraction.padEnd(6, '0'))
}

export function FallbackClient() {
  const gateway = useMemo(() => new LocalGateway(), [])
  const [address, setAddress] = useState<string>()
  const [data, setData] = useState<LiveData>()
  const [amount, setAmount] = useState('1')
  const [busy, setBusy] = useState(false)
  const [message, setMessage] = useState('Use only when the standard UI is unavailable.')
  const run = async (action: () => Promise<unknown>) => {
    setBusy(true)
    try {
      await action()
      if (gateway.session) setData(await gateway.refresh())
      setMessage('Action accepted. Check the confirmed state on the exchange.')
    } catch (error) {
      setMessage(error instanceof Error ? error.message : String(error))
    } finally {
      setBusy(false)
    }
  }
  return (
    <main className="fallback-client">
      <div className="eyebrow">
        MINIMUM RECOVERY CLIENT /{' '}
        {import.meta.env.VITE_APP_STAGE === 'testnet' ? 'HL TESTNET' : 'LOCAL'}
      </div>
      <h1>Cancellation and withdrawal client</h1>
      <p>
        This cannot bypass a stopped canister. Trading accounts are public; deposit and withdrawal
        amounts and timing may reveal links.
      </p>
      <output className="warning-banner fallback-message">{message}</output>
      {!address ? (
        <button
          className="primary"
          disabled={busy}
          onClick={() =>
            void run(async () => {
              const session = await gateway.login()
              setAddress(session.address)
              await gateway.fundingInstructions()
            })
          }
        >
          Authenticate with MetaMask
        </button>
      ) : (
        <>
          <dl className="details">
            <dt>EOA</dt>
            <dd>
              {address.slice(0, 10)}…{address.slice(-6)}
            </dd>
            <dt>Observed at</dt>
            <dd>
              {data?.snapshot
                ? new Date(Number(data.snapshot.observed_at)).toLocaleString('en-US')
                : 'Not observed'}
            </dd>
            <dt>Data age</dt>
            <dd>{data?.snapshot ? `${data.snapshot.data_age_ms} ms` : '—'}</dd>
          </dl>
          <section className="panel table-panel">
            <div className="panel-title">
              <h2>Non-terminal orders</h2>
              <button
                className="danger"
                disabled={busy}
                onClick={() =>
                  window.confirm(
                    'Cancel all non-terminal orders, including protective SL/TP orders?',
                  ) && void run(() => gateway.cancelAll())
                }
              >
                Cancel All
              </button>
            </div>
            <table>
              <thead>
                <tr>
                  <th>ID</th>
                  <th>Market</th>
                  <th>State</th>
                  <th>Action</th>
                </tr>
              </thead>
              <tbody>
                {data?.orders?.items
                  .filter((order) =>
                    ['Open', 'PartiallyFilled', 'Unknown', 'Pending'].includes(
                      variantName(order.state),
                    ),
                  )
                  .map((order) => (
                    <tr key={bytesToHex(order.order_id)}>
                      <td>{bytesToHex(order.order_id).slice(0, 10)}</td>
                      <td>{order.market}</td>
                      <td>{variantName(order.state)}</td>
                      <td>
                        <button
                          disabled={busy}
                          onClick={() => void run(() => gateway.cancel(order.order_id))}
                        >
                          Request cancellation
                        </button>
                      </td>
                    </tr>
                  ))}
              </tbody>
            </table>
          </section>
          <section className="panel">
            <h2>Withdraw to your own wallet</h2>
            <label>
              Amount (USDC)
              <input
                aria-label="Fallback withdrawal amount"
                value={amount}
                onChange={(event) => setAmount(event.target.value)}
              />
            </label>
            <button
              disabled={busy}
              onClick={() => void run(() => gateway.withdraw(toMicros(amount)))}
            >
              Withdraw with MetaMask
            </button>
          </section>
          <div className="fallback-actions">
            <button disabled={busy} onClick={() => void run(async () => undefined)}>
              Refresh
            </button>
            <button
              className="danger"
              disabled={busy}
              onClick={() =>
                void run(async () => {
                  await gateway.logout()
                  setAddress(undefined)
                  setData(undefined)
                })
              }
            >
              Log out
            </button>
          </div>
        </>
      )}
    </main>
  )
}
