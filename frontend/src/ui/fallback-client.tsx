import { useMemo, useState } from 'react'
import { LocalGateway, type LiveData } from '../client/gateway'
import { bytesToHex } from '../client/wallet'
import { variantName } from '../client/result'

const toMicros = (value: string) => {
  if (!/^\d+(\.\d{1,6})?$/.test(value)) throw new Error('USDCは小数6桁以内で入力してください')
  const [whole, fraction = ''] = value.split('.')
  return BigInt(whole) * 1_000_000n + BigInt(fraction.padEnd(6, '0'))
}

export function FallbackClient() {
  const gateway = useMemo(() => new LocalGateway(), [])
  const [address, setAddress] = useState<string>()
  const [data, setData] = useState<LiveData>()
  const [amount, setAmount] = useState('1')
  const [busy, setBusy] = useState(false)
  const [message, setMessage] = useState('通常UI停止時だけ使用してください。')
  const run = async (action: () => Promise<unknown>) => {
    setBusy(true)
    try {
      await action()
      if (gateway.session) setData(await gateway.refresh())
      setMessage('操作を受け付けました。取引所側の確定状態を確認してください。')
    } catch (error) {
      setMessage(error instanceof Error ? error.message : String(error))
    } finally {
      setBusy(false)
    }
  }
  return (
    <main className="fallback-client">
      <div className="eyebrow">MINIMUM RECOVERY CLIENT / LOCAL</div>
      <h1>取消・出金クライアント</h1>
      <p>
        これはCanister停止を回避する仕組みではありません。取引口座は公開され、入出金の額と時刻から関連を推測される場合があります。
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
          MetaMaskで認証
        </button>
      ) : (
        <>
          <dl className="details">
            <dt>EOA</dt>
            <dd>
              {address.slice(0, 10)}…{address.slice(-6)}
            </dd>
            <dt>観測時刻</dt>
            <dd>
              {data?.snapshot
                ? new Date(Number(data.snapshot.observed_at)).toLocaleString()
                : '未観測'}
            </dd>
            <dt>鮮度</dt>
            <dd>{data?.snapshot ? `${data.snapshot.data_age_ms} ms` : '—'}</dd>
          </dl>
          <section className="panel table-panel">
            <div className="panel-title">
              <h2>未終端注文</h2>
              <button
                className="danger"
                disabled={busy}
                onClick={() =>
                  window.confirm('保護用SL/TPを含む未終端注文をすべて取り消しますか？') &&
                  void run(() => gateway.cancelAll())
                }
              >
                Cancel All
              </button>
            </div>
            <table>
              <thead>
                <tr>
                  <th>ID</th>
                  <th>市場</th>
                  <th>状態</th>
                  <th>操作</th>
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
                          取消要求
                        </button>
                      </td>
                    </tr>
                  ))}
              </tbody>
            </table>
          </section>
          <section className="panel">
            <h2>本人EOA宛出金</h2>
            <label>
              金額 (USDC)
              <input
                aria-label="fallback出金額"
                value={amount}
                onChange={(event) => setAmount(event.target.value)}
              />
            </label>
            <button
              disabled={busy}
              onClick={() => void run(() => gateway.withdraw(toMicros(amount)))}
            >
              MetaMask署名で出金
            </button>
          </section>
          <div className="fallback-actions">
            <button disabled={busy} onClick={() => void run(async () => undefined)}>
              再読込
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
              ログアウト
            </button>
          </div>
        </>
      )}
    </main>
  )
}
