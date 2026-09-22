import { useState, type ReactNode } from 'react'
import { bytesToHex } from '../client/wallet'
import { variantName } from '../client/result'
import { useLocalSession } from './local-session'

const micros = (value: bigint | undefined) =>
  value === undefined ? '—' : `${value / 1_000_000n}.${String(value % 1_000_000n).padStart(6, '0')}`
const amountMicros = (value: string) => {
  if (!/^\d+(\.\d{1,6})?$/.test(value)) throw new Error('USDCは小数6桁以内で入力してください')
  const [whole, fraction = ''] = value.split('.')
  const result = BigInt(whole) * 1_000_000n + BigInt(fraction.padEnd(6, '0'))
  if (result <= 0n) throw new Error('金額は0より大きくしてください')
  return result
}

function Workspace({ children }: { children: ReactNode }) {
  const session = useLocalSession()
  return (
    <main className="local-workspace">
      <section className="workspace-bar">
        <div>
          <div className="eyebrow">LOCAL CANISTER / REAL CONNECTION</div>
          <strong>
            {session.address
              ? `${session.address.slice(0, 10)}…${session.address.slice(-6)}`
              : '未接続'}
          </strong>
        </div>
        <div className="session-controls">
          {session.address && (
            <button disabled={session.busy} onClick={() => void session.refresh()}>
              再読込
            </button>
          )}
          <button
            className={session.address ? 'danger' : 'primary'}
            disabled={session.busy}
            onClick={() => void (session.address ? session.logout() : session.login())}
          >
            {session.address ? 'ログアウト' : 'MetaMaskで接続'}
          </button>
        </div>
      </section>
      <div className="local-strip">
        <b>LOCAL MOCK</b>
        <span>実Canister・模擬venue・実資金なし</span>
      </div>
      {session.error && (
        <div role="alert" className="error-banner">
          {session.error}
        </div>
      )}
      {session.data?.issues.length ? (
        <div role="alert" className="warning-banner">
          一部データを取得できませんでした：
          {session.data.issues.map((issue) => `${issue.source} (${issue.message})`).join(' / ')}
        </div>
      ) : null}
      {!session.address ? (
        <section className="empty-state">
          <h1>ローカルセッションを開始</h1>
          <p>短命IC identityとHPKE鍵はこのタブのメモリだけに保持されます。</p>
        </section>
      ) : (
        children
      )}
    </main>
  )
}

function StatusCards() {
  const { data } = useLocalSession()
  return (
    <div className="status-grid" data-testid="account-balances">
      <article>
        <span>RESERVE</span>
        <strong>{micros(data?.funds.reserve_unallocated)}</strong>
        <small>USDC</small>
      </article>
      <article>
        <span>TRADING EQUITY</span>
        <strong>{micros(data?.funds.trading_equity)}</strong>
        <small>USDC</small>
      </article>
      <article>
        <span>WITHDRAWABLE</span>
        <strong>{micros(data?.funds.withdrawable)}</strong>
        <small>USDC</small>
      </article>
      <article>
        <span>FRESHNESS</span>
        <strong>{data?.snapshot ? `${data.snapshot.data_age_ms} ms` : '未観測'}</strong>
        <small>
          {data?.snapshot
            ? new Date(Number(data.snapshot.observed_at)).toLocaleTimeString()
            : 'refresh required'}
        </small>
      </article>
    </div>
  )
}

export function FundsApp() {
  const { gateway, run: execute, busy, data } = useLocalSession()
  const [amount, setAmount] = useState('100')
  const action = (kind: string) => async () => {
    const value = amountMicros(amount)
    if (kind === 'seed') await gateway.seedDeposit(amount)
    if (kind === 'allocate') await gateway.allocate(value)
    if (kind === 'recover') await gateway.recover(value)
    if (kind === 'withdraw') await gateway.withdraw(value)
  }
  return (
    <Workspace>
      <div data-testid="account-panel" className="content-shell">
        <StatusCards />
        <div className="two-column">
          <section className="panel action-panel">
            <h1>資金操作</h1>
            <form>
              <label>
                金額 (USDC)
                <input
                  aria-label="金額"
                  value={amount}
                  onChange={(event) => setAmount(event.target.value)}
                />
              </label>
              <div className="action-grid">
                <button type="button" disabled={busy} onClick={() => void execute(action('seed'))}>
                  LOCAL MOCK 入金seed
                </button>
                <button
                  type="button"
                  disabled={busy}
                  onClick={() => void execute(action('allocate'))}
                >
                  取引口座へ配分
                </button>
                <button
                  type="button"
                  disabled={busy}
                  onClick={() => void execute(action('recover'))}
                >
                  reserveへ回収
                </button>
                <button
                  type="button"
                  disabled={busy}
                  onClick={() => void execute(action('withdraw'))}
                >
                  MetaMask署名で出金
                </button>
              </div>
            </form>
          </section>
          <section className="panel">
            <h2>資金状態</h2>
            <dl className="details">
              <dt>in transit</dt>
              <dd>{micros(data?.funds.in_transit)}</dd>
              <dt>withdrawal reserve</dt>
              <dd>{micros(data?.funds.reserved_for_withdrawal)}</dd>
              <dt>unknown</dt>
              <dd>{data?.funds.unknowns.length ?? 0}</dd>
              <dt>revision</dt>
              <dd>{data?.funds.revision.toString() ?? '—'}</dd>
            </dl>
          </section>
        </div>
      </div>
    </Workspace>
  )
}

export function TradeApp() {
  const { gateway, run: execute, busy, data } = useLocalSession()
  const [market, setMarket] = useState('BTC'),
    [side, setSide] = useState<'buy' | 'sell'>('buy')
  const [kind, setKind] = useState<'market' | 'limit'>('market'),
    [quantity, setQuantity] = useState('0.001'),
    [price, setPrice] = useState('60000')
  const activeAgent =
    data?.agent?.current[0] && variantName(data.agent.current[0].state) === 'Active'
  const fresh = data?.snapshot && data.snapshot.data_age_ms <= 10_000n
  const canOrder = Boolean(activeAgent && fresh && data?.snapshot?.account_id.length)
  return (
    <Workspace>
      <div data-testid="account-panel" className="content-shell">
        <StatusCards />
        <div className="two-column">
          <section className="panel action-panel">
            <h1>注文</h1>
            <div className="form-row">
              <label>
                銘柄
                <select value={market} onChange={(e) => setMarket(e.target.value)}>
                  <option>BTC</option>
                  <option>ETH</option>
                </select>
              </label>
              <label>
                売買
                <select value={side} onChange={(e) => setSide(e.target.value as 'buy' | 'sell')}>
                  <option value="buy">Buy</option>
                  <option value="sell">Sell</option>
                </select>
              </label>
              <label>
                種別
                <select
                  value={kind}
                  onChange={(e) => setKind(e.target.value as 'market' | 'limit')}
                >
                  <option value="market">Market IOC</option>
                  <option value="limit">Limit GTC</option>
                </select>
              </label>
            </div>
            <label>
              数量
              <input value={quantity} onChange={(e) => setQuantity(e.target.value)} />
            </label>
            <label>
              上限 / 指値
              <input value={price} onChange={(e) => setPrice(e.target.value)} />
            </label>
            <button
              className="primary"
              disabled={busy || !canOrder}
              onClick={() =>
                void execute(() => gateway.submitOrder({ market, side, kind, quantity, price }))
              }
            >
              {canOrder ? '注文を受付' : 'Agent・口座観測を確認してください'}
            </button>
          </section>
          <section className="panel">
            <h2>Agent</h2>
            <p>{activeAgent ? 'Active' : '未承認'}</p>
            <button
              disabled={busy || activeAgent}
              onClick={() => void execute(() => gateway.approveAgent())}
            >
              Agentを生成・承認
            </button>
            <p className="muted">preflightと送信状態は下表へそのまま表示します。</p>
          </section>
        </div>
        <section className="panel table-panel">
          <h2>注文状態</h2>
          <table>
            <thead>
              <tr>
                <th>市場</th>
                <th>種別</th>
                <th>状態</th>
                <th>preflight</th>
                <th>dispatch</th>
                <th>数量</th>
                <th>操作</th>
              </tr>
            </thead>
            <tbody>
              {data?.orders?.items.map((order) => (
                <tr key={bytesToHex(order.order_id)}>
                  <td>{order.market}</td>
                  <td>{order.kind}</td>
                  <td>{variantName(order.state)}</td>
                  <td>{variantName(order.preflight_state)}</td>
                  <td>{variantName(order.dispatch_state)}</td>
                  <td>
                    {order.filled_quantity} / {order.quantity}
                  </td>
                  <td>
                    <button
                      disabled={
                        busy || !['Open', 'PartiallyFilled'].includes(variantName(order.state))
                      }
                      onClick={() => void execute(() => gateway.cancel(order.order_id))}
                    >
                      取消
                    </button>
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
          {!data?.orders?.items.length && <p>注文はまだありません</p>}
        </section>
      </div>
    </Workspace>
  )
}

export function HistoryApp() {
  const { data } = useLocalSession()
  return (
    <Workspace>
      <div data-testid="account-panel" className="content-shell">
        <StatusCards />
        <section className="panel table-panel">
          <h1>資金履歴</h1>
          <table>
            <thead>
              <tr>
                <th>時刻</th>
                <th>操作</th>
                <th>状態</th>
                <th>金額</th>
              </tr>
            </thead>
            <tbody>
              {data?.fundEvents.items.map((event) => (
                <tr key={bytesToHex(event.event_id)}>
                  <td>{new Date(Number(event.at)).toLocaleString()}</td>
                  <td>{variantName(event.kind)}</td>
                  <td>{variantName(event.state)}</td>
                  <td>{micros(event.amount)}</td>
                </tr>
              ))}
            </tbody>
          </table>
        </section>
        <section className="panel table-panel">
          <h2>約定</h2>
          <table>
            <thead>
              <tr>
                <th>時刻</th>
                <th>市場</th>
                <th>価格</th>
                <th>数量</th>
                <th>手数料</th>
              </tr>
            </thead>
            <tbody>
              {data?.fills?.items.map((fill, index) => (
                <tr key={`${fill.at}-${index}`}>
                  <td>{new Date(Number(fill.at)).toLocaleString()}</td>
                  <td>{fill.market}</td>
                  <td>{fill.price}</td>
                  <td>{fill.quantity}</td>
                  <td>{micros(fill.fee)}</td>
                </tr>
              ))}
            </tbody>
          </table>
        </section>
      </div>
    </Workspace>
  )
}
