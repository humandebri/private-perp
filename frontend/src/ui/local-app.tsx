import { useState, type ReactNode } from 'react'
import { bytesToHex } from '../client/wallet'
import { variantName } from '../client/result'
import type { Fill, OrderSummary, Position } from '../client/candid-codec'
import type { FundEvent } from '../client/candid/funds_vault.did.js'
import { useLocalSession } from './local-session'
import { PriceChart, useMarket } from './market'

const micros = (value: bigint | undefined) =>
  value === undefined ? '—' : `${value / 1_000_000n}.${String(value % 1_000_000n).padStart(6, '0')}`
const signedMicros = (value: bigint | undefined) =>
  value === undefined ? '—' : `${value < 0n ? '-' : ''}${micros(value < 0n ? -value : value)}`
const amountMicros = (value: string) => {
  if (!/^\d+(\.\d{1,6})?$/.test(value)) throw new Error('USDCは小数6桁以内で入力してください')
  const [whole, fraction = ''] = value.split('.')
  const result = BigInt(whole) * 1_000_000n + BigInt(fraction.padEnd(6, '0'))
  if (result <= 0n) throw new Error('金額は0より大きくしてください')
  return result
}

function Workspace({
  children,
  publicContent = false,
}: {
  children: ReactNode
  publicContent?: boolean
}) {
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
            disabled={session.busy && !session.address}
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
      {session.busy && (
        <output className="operation-status">
          処理中です。署名を求められた場合はMetaMaskを確認してください。
        </output>
      )}
      {session.error && (
        <div role="alert" className="error-banner">
          {session.error}
        </div>
      )}
      {session.refreshError && (
        <div role="alert" className="warning-banner">
          更新失敗：{session.refreshError}。古い口座情報での新規注文は停止しています。
        </div>
      )}
      {session.data?.issues.length ? (
        <div role="alert" className="warning-banner">
          一部データを取得できませんでした：
          {session.data.issues.map((issue) => `${issue.source} (${issue.message})`).join(' / ')}
        </div>
      ) : null}
      {!session.address && !publicContent ? (
        <section className="empty-state">
          <h1>ローカルセッションを開始</h1>
          <p>上の「MetaMaskで接続」から始めてください。実資金は使用しません。</p>
          <p>接続情報はこのタブだけに保持され、再読込・ログアウトで破棄されます。</p>
        </section>
      ) : (
        children
      )}
    </main>
  )
}

function StatusCards() {
  const { data, age, fresh } = useLocalSession()
  return (
    <div className="status-grid" data-testid="account-balances">
      <article>
        <span>RESERVE</span>
        <strong>{micros(data?.funds.reserve_unallocated)}</strong>
        <small>USDC</small>
      </article>
      {data?.snapshot && (
        <>
          <article>
            <span>VENUE MARGIN</span>
            <strong>{micros(data.snapshot.margin_used)}</strong>
            <small>USDC observed</small>
          </article>
          <article>
            <span>UNREALIZED PNL</span>
            <strong>{signedMicros(data.snapshot.unrealized_pnl)}</strong>
            <small>risk reserved {micros(data.snapshot.open_order_risk_reserved)}</small>
          </article>
        </>
      )}
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
        <strong>
          {data?.snapshot
            ? `${Number.isFinite(age) ? Math.floor(age) : '—'} ms${fresh ? '' : ' / 更新待ち'}`
            : '未観測'}
        </strong>
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
            <p>
              模擬入金 → 取引口座へ配分 → 取引の順に進めます。出金前にはreserveへ回収してください。
            </p>
            <form>
              <label>
                金額 (USDC)
                <input
                  aria-label="金額"
                  inputMode="decimal"
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
            <h2>Agent</h2>
            <dl className="details">
              <dt>state</dt>
              <dd>
                {data?.agent?.current[0] ? variantName(data.agent.current[0].state) : '未承認'}
              </dd>
              <dt>generation</dt>
              <dd>{data?.agent?.current[0]?.generation.toString() ?? '—'}</dd>
              <dt>expires</dt>
              <dd>
                {data?.agent?.current[0]?.expires_at[0]
                  ? new Date(Number(data.agent.current[0].expires_at[0])).toLocaleString()
                  : '期限なし'}
              </dd>
              <dt>revocation</dt>
              <dd>{data?.agent?.revocation_pending ? '要求中' : 'なし'}</dd>
            </dl>
          </section>
        </div>
      </div>
    </Workspace>
  )
}

export function TradeApp() {
  const {
    gateway,
    run: execute,
    busy,
    data,
    fresh,
    orderBlockReason: disableReason,
    orders: tracked,
    submit: submitOrder,
  } = useLocalSession()
  const publicMarket = useMarket()
  const [market, setMarket] = useState('BTC'),
    [side, setSide] = useState<'buy' | 'sell'>('buy')
  const [kind, setKind] = useState<'market' | 'limit'>('market'),
    [quantity, setQuantity] = useState('0.001'),
    [price, setPrice] = useState('60000')
  const [leverage, setLeverage] = useState('3')
  const [slippage, setSlippage] = useState('50')
  const activeAgent =
    data?.agent?.current[0] && variantName(data.agent.current[0].state) === 'Active'
  const canOrder = !disableReason
  const known = new Set(data?.orders?.items.map((order) => bytesToHex(order.order_id)) ?? [])
  const orders = [
    ...(data?.orders?.items ?? []),
    ...tracked.flatMap((item) =>
      item.order && !known.has(bytesToHex(item.order.order_id)) ? [item.order] : [],
    ),
  ].sort((a, b) => Number(b.created_at - a.created_at))
  const optimistic = tracked.filter((item) => !item.order && !known.has(item.orderId ?? ''))
  const submit = () =>
    void submitOrder({
      market,
      side,
      kind,
      quantity,
      price,
      leverage: Number(leverage),
      slippageBps: Number(slippage),
    })
  const book = publicMarket.book[market] ?? [[], []]
  const trades = publicMarket.trades[market] ?? []
  const inputError =
    !/^\d+(\.\d+)?$/.test(quantity) || Number(quantity) <= 0 || !Number.isFinite(Number(quantity))
      ? '数量は0より大きい数値で入力してください。'
      : !/^\d+(\.\d+)?$/.test(price) || Number(price) <= 0 || !Number.isFinite(Number(price))
        ? '価格は0より大きい数値で入力してください。'
        : !/^[1-5]$/.test(leverage)
          ? 'レバレッジは1〜5の整数で入力してください。'
          : kind === 'market' &&
              (!/^\d+$/.test(slippage) || !Number.isSafeInteger(Number(slippage)))
            ? 'スリッページは0以上の整数で入力してください。'
            : undefined
  return (
    <Workspace publicContent>
      <div data-testid="account-panel" className="content-shell">
        <section className="market-head">
          <div>
            <span className="eyebrow">{market} PERPETUAL</span>
            <strong>{publicMarket.mids[market] ?? '—'}</strong>
          </div>
          <span className={`state-chip ${publicMarket.connection}`}>
            市況 ·{' '}
            {publicMarket.connection === 'live'
              ? '接続中'
              : publicMarket.connection === 'reconnecting'
                ? '再接続中'
                : '未接続'}
          </span>
          {data && (
            <span className={`state-chip ${fresh ? 'live' : 'warning'}`}>
              ACCOUNT · {fresh ? 'fresh' : 'stale'}
            </span>
          )}
        </section>
        {data && <StatusCards />}
        <nav className="trade-shortcuts" aria-label="取引画面内の移動">
          <a href="#order-ticket">注文入力</a>
          <a href="#market-chart">チャート</a>
          <a href="#order-status">注文状態</a>
        </nav>
        <div className="trading-grid">
          <section className="panel chart-panel" id="market-chart">
            <div className="panel-title">
              <h2>{market} / USDC</h2>
              <span>1分足 · 模擬データ</span>
            </div>
            {publicMarket.candles[market]?.length ? (
              <PriceChart candles={publicMarket.candles[market]} />
            ) : (
              <div className="chart-empty">
                <strong>チャートデータを待っています</strong>
                <p>表示されない場合は、ローカルmockの起動と接続設定を確認してください。</p>
              </div>
            )}
          </section>
          <section className="panel market-tape">
            <h2>板</h2>
            <div className="book-labels">
              <span>価格 (USDC)</span>
              <span>数量 ({market})</span>
            </div>
            {!book[0].length && !book[1].length && <p>板データを待っています。</p>}
            <div className="book-side asks">
              {book[1].map((level) => (
                <div key={level.px}>
                  <span>{level.px}</span>
                  <span>{level.sz}</span>
                </div>
              ))}
            </div>
            <div className="mid-price">{publicMarket.mids[market] ?? '—'}</div>
            <div className="book-side bids">
              {book[0].map((level) => (
                <div key={level.px}>
                  <span>{level.px}</span>
                  <span>{level.sz}</span>
                </div>
              ))}
            </div>
            <h2>公開約定</h2>
            {!trades.length && <p>約定データはまだありません。</p>}
            <div className="trade-tape">
              {trades.map((trade) => (
                <div key={trade.tid} className={trade.side === 'B' ? 'positive' : 'negative'}>
                  <span>{trade.px}</span>
                  <span>{trade.sz}</span>
                </div>
              ))}
            </div>
          </section>
          <section className="panel action-panel order-ticket" id="order-ticket">
            <h1>注文</h1>
            <div className="form-row">
              <label>
                銘柄
                <select
                  value={market}
                  onChange={(e) => {
                    setMarket(e.target.value)
                    setPrice('')
                  }}
                >
                  <option>BTC</option>
                  <option>ETH</option>
                </select>
              </label>
              <label>
                売買
                <select value={side} onChange={(e) => setSide(e.target.value as 'buy' | 'sell')}>
                  <option value="buy">買い / Buy</option>
                  <option value="sell">売り / Sell</option>
                </select>
              </label>
              <label>
                種別
                <select
                  value={kind}
                  onChange={(e) => setKind(e.target.value as 'market' | 'limit')}
                >
                  <option value="market">成行 (IOC)</option>
                  <option value="limit">指値 (GTC)</option>
                </select>
              </label>
            </div>
            <label>
              数量 ({market})
              <input
                aria-label="数量"
                inputMode="decimal"
                value={quantity}
                onChange={(e) => setQuantity(e.target.value)}
              />
            </label>
            <label>
              {kind === 'limit' ? '指値' : side === 'buy' ? '買い価格の上限' : '売り価格の下限'}{' '}
              (USDC)
              <input
                aria-label="注文価格"
                inputMode="decimal"
                value={price}
                onChange={(e) => setPrice(e.target.value)}
              />
            </label>
            <button
              className="reference-price"
              disabled={!publicMarket.mids[market] || publicMarket.connection !== 'live'}
              onClick={() => setPrice(publicMarket.mids[market] ?? '')}
            >
              現在の参考価格を入力
            </button>
            <div className="form-row two">
              <label>
                レバレッジ
                <input
                  aria-label="レバレッジ"
                  type="number"
                  min="1"
                  max="5"
                  value={leverage}
                  onChange={(e) => setLeverage(e.target.value)}
                />
              </label>
              <label>
                スリッページ (bps)
                <input
                  aria-label="スリッページ"
                  inputMode="numeric"
                  disabled={kind === 'limit'}
                  value={slippage}
                  onChange={(e) => setSlippage(e.target.value)}
                />
              </label>
            </div>
            <p className="ticket-help">
              {kind === 'market'
                ? '指定した価格とスリッページの範囲で即時約定を試み、未約定分は取消します。50 bps = 0.5%。'
                : '指定価格で注文を残します。約定するか、取り消すまで有効です。'}
            </p>
            <button
              className={`primary ${side === 'sell' ? 'sell-submit' : ''}`}
              aria-describedby="order-guidance"
              disabled={busy || !canOrder || Boolean(inputError)}
              onClick={submit}
            >
              注文を受付
            </button>
            <div id="order-guidance" className="order-guidance" aria-live="polite">
              {busy
                ? '処理中です。二重送信せずお待ちください。'
                : (disableReason ??
                  inputError ??
                  `${market}を${side === 'buy' ? '買い' : '売り'}で送信します。受付後の約定結果は注文状態で確認できます。`)}
            </div>
            {!data && (
              <p className="ticket-note">
                公開市況は閲覧できます。注文にはMetaMask接続が必要です。
              </p>
            )}
            {data && (
              <p className="agent-line">
                Agent <b>{activeAgent ? 'Active' : '未承認'}</b>
              </p>
            )}
            {data && !activeAgent && (
              <button disabled={busy} onClick={() => void execute(() => gateway.approveAgent())}>
                Agentを生成・承認
              </button>
            )}
            <p className="muted">
              Marketはスリッページ上限付きIOC。倍率は安全性の推奨ではありません。
            </p>
          </section>
        </div>
        {data?.snapshot?.positions.length ? (
          <section className="panel table-panel">
            <div className="panel-title">
              <h2>建玉</h2>
              <button
                className="danger"
                disabled={busy}
                onClick={() =>
                  window.confirm('すべての建玉をreduce-onlyで決済します。続行しますか？') &&
                  void execute(() => gateway.closeAll())
                }
              >
                全決済
              </button>
            </div>
            <table>
              <thead>
                <tr>
                  <th>市場</th>
                  <th>数量</th>
                  <th>Entry</th>
                  <th>PnL</th>
                  <th>清算価格</th>
                  <th>倍率</th>
                  <th>保護 / 決済</th>
                </tr>
              </thead>
              <tbody>
                {data.snapshot.positions.map((position) => (
                  <PositionRow
                    key={position.market}
                    position={position}
                    busy={busy}
                    run={execute}
                  />
                ))}
              </tbody>
            </table>
          </section>
        ) : null}
        <section className="panel table-panel" id="order-status">
          <div className="panel-title">
            <h2>注文状態</h2>
            <button
              className="danger"
              disabled={
                busy ||
                !data?.orders?.items.some((order) =>
                  ['Open', 'PartiallyFilled'].includes(variantName(order.state)),
                )
              }
              onClick={() =>
                window.confirm(
                  '未終端注文をすべて取り消します。保護用SL/TPも対象です。続行しますか？',
                ) && void execute(() => gateway.cancelAll())
              }
            >
              Cancel All
            </button>
          </div>
          <table>
            <thead>
              <tr>
                <th>市場</th>
                <th>種別</th>
                <th>状態</th>
                <th>preflight</th>
                <th>dispatch</th>
                <th>約定 / 注文数量</th>
                <th>操作</th>
              </tr>
            </thead>
            <tbody>
              {optimistic.map((order) => (
                <tr key={bytesToHex(order.id)} className="pending-row">
                  <td>{order.market}</td>
                  <td>—</td>
                  <td>
                    {
                      {
                        sending: '送信中',
                        accepted: '受付済み・照合待ち',
                        rejected: '確定拒否',
                        unknown: '応答不明・再送禁止',
                      }[order.state]
                    }
                    {order.message && <small>{order.message}</small>}
                  </td>
                  <td>—</td>
                  <td>—</td>
                  <td>—</td>
                  <td>{order.state === 'rejected' ? '入力・エラー内容を確認' : '照合待ち'}</td>
                </tr>
              ))}
              {orders.map((order) => (
                <tr key={bytesToHex(order.order_id)}>
                  <td>{order.market}</td>
                  <td>
                    {order.trigger[0]
                      ? `positionTpsl/${variantName(order.trigger[0].kind)}`
                      : order.kind}
                  </td>
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
          {!data?.orders?.items.length && !optimistic.length && (
            <p className="table-empty">
              {data
                ? '注文はまだありません。上の注文入力から始めてください。'
                : '接続後に、ご自身の注文と約定状態を表示します。'}
            </p>
          )}
        </section>
      </div>
    </Workspace>
  )
}

function PositionRow({
  position,
  busy,
  run,
}: {
  position: Position
  busy: boolean
  run: (action: () => Promise<unknown>) => Promise<void>
}) {
  const { gateway, generation, store } = useLocalSession()
  const [stopLoss, setStopLoss] = useState(position.stop_loss[0] ?? '')
  const [takeProfit, setTakeProfit] = useState(position.take_profit[0] ?? '')
  const protect = async () => {
    if (stopLoss) await gateway.protectPosition(position, 'stopLoss', stopLoss)
    if (!store.isCurrent(generation)) return
    if (takeProfit) await gateway.protectPosition(position, 'takeProfit', takeProfit)
  }
  return (
    <tr>
      <td>{position.market}</td>
      <td>{position.size}</td>
      <td>{position.entry_price}</td>
      <td className={position.unrealized_pnl >= 0n ? 'positive' : 'negative'}>
        {signedMicros(position.unrealized_pnl)}
      </td>
      <td>{position.liquidation_price[0] ?? '—'}</td>
      <td>
        {position.leverage}x {position.margin_mode}
      </td>
      <td>
        <div className="position-actions">
          <input
            aria-label={`${position.market} Stop Loss`}
            placeholder="SL"
            value={stopLoss}
            onChange={(event) => setStopLoss(event.target.value)}
          />
          <input
            aria-label={`${position.market} Take Profit`}
            placeholder="TP"
            value={takeProfit}
            onChange={(event) => setTakeProfit(event.target.value)}
          />
          <button disabled={busy || (!stopLoss && !takeProfit)} onClick={() => void run(protect)}>
            SL/TP設定
          </button>
          {[2500, 5000, 10000].map((ratio) => (
            <button
              key={ratio}
              disabled={busy}
              onClick={() => void run(() => gateway.closePosition(position, ratio))}
            >
              {ratio / 100}%決済
            </button>
          ))}
        </div>
      </td>
    </tr>
  )
}

export function HistoryApp() {
  const { generation } = useLocalSession()
  return <SessionHistory key={generation} />
}

function SessionHistory() {
  const { data, gateway, run, busy, generation, store } = useLocalSession()
  const [moreOrders, setMoreOrders] = useState<OrderSummary[]>([])
  const [moreFills, setMoreFills] = useState<Fill[]>([])
  const [moreFunds, setMoreFunds] = useState<FundEvent[]>([])
  const [orderCursor, setOrderCursor] = useState<Uint8Array | null>()
  const [fillCursor, setFillCursor] = useState<Uint8Array | null>()
  const [fundCursor, setFundCursor] = useState<Uint8Array | number[] | null>()
  const orders = [
    ...new Map(
      [...moreOrders, ...(data?.orders?.items ?? [])].map((order) => [
        bytesToHex(order.order_id),
        order,
      ]),
    ).values(),
  ].sort((a, b) => Number(b.created_at - a.created_at))
  const fills = [...(data?.fills?.items ?? []), ...moreFills]
  const funds = [
    ...new Map(
      [...moreFunds, ...(data?.fundEvents.items ?? [])].map((event) => [
        bytesToHex(event.event_id),
        event,
      ]),
    ).values(),
  ].sort((a, b) => Number(b.at - a.at))
  const nextOrder = orderCursor === undefined ? data?.orders?.next_cursor[0] : orderCursor
  const nextFill = fillCursor === undefined ? data?.fills?.next_cursor[0] : fillCursor
  const nextFund = fundCursor === undefined ? data?.fundEvents.next_cursor[0] : fundCursor
  return (
    <Workspace>
      <div data-testid="account-panel" className="content-shell">
        <StatusCards />
        {data?.funds.unknowns.length ||
        orders.some((order) => variantName(order.state) === 'Unknown') ? (
          <section className="panel unknown-panel">
            <h2>未解決の結果不明</h2>
            <p>
              外部結果を照合中です。要求を再送せず、取消またはreduce-only操作だけを使用してください。
            </p>
          </section>
        ) : null}
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
              {funds.map((event) => (
                <tr key={bytesToHex(event.event_id)}>
                  <td>{new Date(Number(event.at)).toLocaleString()}</td>
                  <td>{variantName(event.kind)}</td>
                  <td>{variantName(event.state)}</td>
                  <td>{micros(event.amount)}</td>
                </tr>
              ))}
            </tbody>
          </table>
          {nextFund && (
            <button
              disabled={busy}
              onClick={() =>
                void run(async () => {
                  const page = await gateway.listFundEvents(nextFund)
                  if (!store.isCurrent(generation)) return
                  setMoreFunds((current) => [...current, ...page.items])
                  setFundCursor(page.next_cursor[0] ?? null)
                })
              }
            >
              資金履歴をさらに表示
            </button>
          )}
        </section>
        <section className="panel table-panel">
          <h2>注文履歴</h2>
          <table>
            <thead>
              <tr>
                <th>更新</th>
                <th>市場</th>
                <th>種別</th>
                <th>状態</th>
                <th>数量</th>
                <th>参照ID</th>
              </tr>
            </thead>
            <tbody>
              {orders.map((order) => (
                <tr key={bytesToHex(order.order_id)}>
                  <td>{new Date(Number(order.updated_at)).toLocaleString()}</td>
                  <td>{order.market}</td>
                  <td>{order.kind}</td>
                  <td>{variantName(order.state)}</td>
                  <td>
                    {order.filled_quantity} / {order.quantity}
                  </td>
                  <td>{bytesToHex(order.order_id).slice(0, 12)}</td>
                </tr>
              ))}
            </tbody>
          </table>
          {nextOrder && (
            <button
              disabled={busy}
              onClick={() =>
                void run(async () => {
                  const page = await gateway.listOrders(nextOrder)
                  if (!store.isCurrent(generation)) return
                  setMoreOrders((current) => [...current, ...page.items])
                  setOrderCursor(page.next_cursor[0] ?? null)
                })
              }
            >
              注文履歴をさらに表示
            </button>
          )}
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
              {fills.map((fill, index) => (
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
          {nextFill && (
            <button
              disabled={busy}
              onClick={() =>
                void run(async () => {
                  const page = await gateway.listFills(nextFill)
                  if (!store.isCurrent(generation)) return
                  setMoreFills((current) => [...current, ...page.items])
                  setFillCursor(page.next_cursor[0] ?? null)
                })
              }
            >
              約定履歴をさらに表示
            </button>
          )}
        </section>
      </div>
    </Workspace>
  )
}
