import { useEffect, useState } from 'react'
import { QueryClient, QueryClientProvider, useQuery, useQueryClient } from '@tanstack/react-query'
import {
  createColumnHelper,
  flexRender,
  getCoreRowModel,
  useReactTable,
} from '@tanstack/react-table'
import type { DemoOrder, OrderInput, Scenario } from '../domain/demo'
import {
  labels,
  parseUsdc,
  STALE_AFTER_MS,
  STALE_NOTICE,
  STALE_SAMPLE_INTERVAL_MS,
} from '../domain/demo'
import {
  cancelAllDemo,
  cancelDemo,
  logoutDemo,
  refreshDemo,
  setScenario,
  setStale,
  startDemo,
  submitDemo,
  transferDemo,
  useDemo,
} from '../client/demo-session'
import { Chart } from './chart'

/** USDC amounts keep the full 1e-6 unit so a 0.000001 move never renders as 0. */
const money = (n: number) =>
  n.toLocaleString('en-US', { minimumFractionDigits: 6, maximumFractionDigits: 6 })
/** Prices are quoted with 2 decimals, like the demo ticket. */
const quote = (n: number) =>
  n.toLocaleString('en-US', { minimumFractionDigits: 2, maximumFractionDigits: 2 })
const helper = createColumnHelper<DemoOrder>()
const columns = [
  helper.accessor('market', { header: '銘柄' }),
  helper.accessor('side', {
    header: '方向',
    cell: (info) => (
      <span className={info.getValue() === 'buy' ? 'positive' : 'negative'}>
        {info.getValue() === 'buy' ? 'Long' : 'Short'}
      </span>
    ),
  }),
  helper.accessor('kind', { header: '種別' }),
  helper.accessor('quantity', { header: '数量' }),
  helper.accessor('price', { header: '価格', cell: (info) => quote(Number(info.getValue())) }),
  helper.accessor('filled', { header: '約定数量' }),
  helper.accessor('status', {
    header: '状態',
    cell: (info) => (
      <span className={info.getValue() === 'unknown' ? 'warning' : ''}>
        {labels[info.getValue()]}
      </span>
    ),
  }),
  helper.display({
    id: 'actions',
    header: '操作',
    cell: ({ row }) => (
      <button
        className="text-button"
        disabled={!['open', 'partial', 'queued'].includes(row.original.status)}
        onClick={() => cancelDemo(row.original.id)}
      >
        取消
      </button>
    ),
  }),
]

export function DemoApp({ screen }: { screen: 'trade' | 'funds' | 'history' }) {
  const [queryClient] = useState(
    () =>
      new QueryClient({
        defaultOptions: { queries: { retry: false, refetchOnWindowFocus: false } },
      }),
  )
  return (
    <QueryClientProvider client={queryClient}>
      <Workspace screen={screen} />
    </QueryClientProvider>
  )
}

function Workspace({ screen }: { screen: 'trade' | 'funds' | 'history' }) {
  const state = useDemo()
  const queryClient = useQueryClient()
  const [simulateStale, toggleStale] = useState(false)
  const [tick, setTick] = useState(0)
  const [now, setNow] = useState(() => Date.now())
  useEffect(() => {
    const timer = setInterval(() => {
      setTick((v) => v + 1)
      setNow(Date.now())
      if (!simulateStale) refreshDemo()
    }, STALE_SAMPLE_INTERVAL_MS)
    return () => clearInterval(timer)
  }, [simulateStale])
  // Reads only generated public demo prices; never fetches account data.
  const market = useQuery({
    queryKey: ['synthetic-market', tick],
    queryFn: () => ({ btc: 64582.4 + Math.sin(tick) * 8, eth: 3428.15 + Math.sin(tick) }),
    gcTime: 0,
  })
  const stale = state.active && (simulateStale || now - state.observedAt > STALE_AFTER_MS)
  return (
    <>
      <section className="workspace-bar" data-testid="account-panel">
        <span className="eyebrow">WORKSPACE / {screen.toUpperCase()}</span>
        <div className="session-controls">
          {state.active ? (
            <>
              <span className="session-label">合成口座 DEMO-001</span>
              <button
                className="secondary"
                onClick={() => {
                  logoutDemo()
                  queryClient.clear()
                  toggleStale(false)
                }}
              >
                デモ終了
              </button>
            </>
          ) : (
            <button className="primary" onClick={startDemo}>
              デモセッション開始
            </button>
          )}
        </div>
      </section>
      <div className="demo-strip">
        合成データ専用 · ウォレット接続・署名・送金なし <span>実注文は実行できません</span>
      </div>
      {screen === 'trade' ? (
        <Trade btc={market.data?.btc ?? 64582.4} eth={market.data?.eth ?? 3428.15} stale={stale} />
      ) : screen === 'funds' ? (
        <Funds />
      ) : (
        <History />
      )}
      <section className="simulation">
        <div>
          <strong>検証パネル</strong>
          <span> 外部通信なしで障害表示を確認</span>
        </div>
        <label>
          応答シナリオ
          <select
            aria-label="応答シナリオ"
            value={state.scenario}
            onChange={(event) => setScenario(event.target.value as Scenario)}
          >
            <option value="open">正常受理</option>
            <option value="partial">部分約定</option>
            <option value="unknown">結果不明</option>
            <option value="rejected">HL拒否</option>
            <option value="cancel-race">取消中に全約定</option>
          </select>
        </label>
        <label className="checkbox">
          <input
            type="checkbox"
            checked={simulateStale}
            onChange={(event) => {
              toggleStale(event.target.checked)
              setStale(event.target.checked)
            }}
          />
          口座データ遅延
        </label>
        <span className={stale ? 'warning' : 'muted'}>
          {state.active
            ? stale
              ? STALE_NOTICE
              : `更新 ${new Date(state.observedAt).toLocaleTimeString('ja-JP')}`
            : '未接続'}
        </span>
      </section>
    </>
  )
}

function Trade({ btc, eth, stale }: { btc: number; eth: number; stale: boolean }) {
  const state = useDemo()
  const [market, setMarket] = useState<'BTC' | 'ETH'>('BTC')
  const [tab, setTab] = useState<'orders' | 'positions'>('orders')
  const price = market === 'BTC' ? btc : eth
  return (
    <main className="terminal">
      <section className="market-header">
        <div className="market-symbol">
          <span className="coin">{market === 'BTC' ? '₿' : 'Ξ'}</span>
          <select
            aria-label="取引銘柄"
            value={market}
            onChange={(event) => setMarket(event.target.value as 'BTC' | 'ETH')}
          >
            <option value="BTC">BTC / USDC</option>
            <option value="ETH">ETH / USDC</option>
          </select>
          <small>PERPETUAL</small>
        </div>
        <div className="price-big positive">{quote(price)}</div>
        <MarketStat label="24h変動（合成）" value="+2.41%" positive />
        <MarketStat label="24h出来高（合成）" value="$842.6M" />
        <MarketStat label="Funding（合成）" value="0.0100%" />
        <MarketStat label="建玉残高（合成）" value="$1.24B" />
      </section>
      <div className="trade-grid">
        <section className="panel chart-panel">
          <div className="panel-heading">
            <h2>{market} 価格チャート</h2>
            <span>15m · Candles · DEMO</span>
          </div>
          <Chart market={market} />
        </section>
        <section className="panel book">
          <div className="panel-heading">
            <h2>オーダーブック</h2>
            <span>合成</span>
          </div>
          <div className="book-labels">
            <span>価格 (USDC)</span>
            <span>数量 ({market})</span>
          </div>
          {Array.from({ length: 7 }, (_, i) => (
            <BookRow
              key={`ask-${i}`}
              price={price + (7 - i) * (market === 'BTC' ? 4 : 0.4)}
              index={i}
              sell
            />
          ))}
          <div className="mid-price positive">
            {quote(price)} <span>↑</span>
          </div>
          {Array.from({ length: 7 }, (_, i) => (
            <BookRow
              key={`bid-${i}`}
              price={price - (i + 1) * (market === 'BTC' ? 4 : 0.4)}
              index={6 - i}
            />
          ))}
        </section>
        <section className="panel ticket">
          <OrderForm
            key={`${market}-${state.active}`}
            market={market}
            price={price}
            stale={stale}
          />
        </section>
      </div>
      <section className="panel orders">
        <div className="tabs">
          <button className={tab === 'orders' ? 'selected' : ''} onClick={() => setTab('orders')}>
            注文一覧 <span>{state.orders.length}</span>
          </button>
          <button
            className={tab === 'positions' ? 'selected' : ''}
            onClick={() => setTab('positions')}
          >
            建玉プレビュー
          </button>
        </div>
        {tab === 'orders' ? (
          <Orders />
        ) : (
          <div className="empty">
            <strong>建玉・SL/TPは次の接続段階</strong>
            <p>
              注文シミュレーションは実ポジションや損益を生成しません。HL照合APIができるまで、決済・SL/TPは操作できません。
            </p>
            <button disabled>SL / TP 設定（未接続）</button>
          </div>
        )}
      </section>
    </main>
  )
}
function MarketStat({
  label,
  value,
  positive = false,
}: {
  label: string
  value: string
  positive?: boolean
}) {
  return (
    <div className="market-stat">
      <small>{label}</small>
      <span className={positive ? 'positive' : ''}>{value}</span>
    </div>
  )
}
function BookRow({ price, index, sell = false }: { price: number; index: number; sell?: boolean }) {
  return (
    <div className={`book-row ${sell ? 'ask' : 'bid'}`}>
      <div className="depth" style={{ width: `${22 + index * 10}%` }} />
      <span className={sell ? 'negative' : 'positive'}>{quote(price)}</span>
      <span>{(0.042 + index * 0.183).toFixed(3)}</span>
    </div>
  )
}

function OrderForm({
  market,
  price,
  stale,
}: {
  market: 'BTC' | 'ETH'
  price: number
  stale: boolean
}) {
  const state = useDemo()
  const [side, setSide] = useState<'buy' | 'sell'>('buy')
  const [kind, setKind] = useState<'Market' | 'Limit'>('Limit')
  const [quantity, setQuantity] = useState('0.01')
  const [limit, setLimit] = useState(price.toFixed(2))
  const [requestId, setRequestId] = useState<string | null>(null)
  const [message, setMessage] = useState('')
  const submitted = state.orders.find((order) => order.id === requestId)
  const frozen = !!submitted
  const input: OrderInput = {
    market,
    side,
    kind,
    quantity,
    price: kind === 'Market' ? price.toFixed(2) : limit,
  }
  function submit() {
    try {
      const id = requestId ?? crypto.randomUUID()
      setRequestId(id)
      submitDemo(input, id)
      setMessage('')
    } catch (error) {
      setMessage(error instanceof Error ? error.message : '受付を確認できません。')
    }
  }
  return (
    <>
      <div className="panel-heading">
        <h2>注文</h2>
        <span>分離証拠金 · 3×</span>
      </div>
      <div className="segmented">
        <button
          disabled={frozen}
          className={side === 'buy' ? 'long active' : ''}
          onClick={() => setSide('buy')}
        >
          買い / Long
        </button>
        <button
          disabled={frozen}
          className={side === 'sell' ? 'short active' : ''}
          onClick={() => setSide('sell')}
        >
          売り / Short
        </button>
      </div>
      <div className="order-kind">
        <button
          disabled={frozen}
          className={kind === 'Limit' ? 'selected' : ''}
          onClick={() => setKind('Limit')}
        >
          指値
        </button>
        <button
          disabled={frozen}
          className={kind === 'Market' ? 'selected' : ''}
          onClick={() => setKind('Market')}
        >
          成行
        </button>
      </div>
      <label>
        価格 <span>USDC</span>
        <input
          aria-label="注文価格"
          inputMode="decimal"
          disabled={frozen || kind === 'Market'}
          value={kind === 'Market' ? price.toFixed(2) : limit}
          onChange={(event) => setLimit(event.target.value)}
        />
      </label>
      <label>
        数量 <span>{market}</span>
        <input
          aria-label="注文数量"
          inputMode="decimal"
          disabled={frozen}
          value={quantity}
          onChange={(event) => setQuantity(event.target.value)}
        />
      </label>
      <dl className="summary">
        <div>
          <dt>注文額（参考）</dt>
          <dd>{money(Number(input.price) * Number(quantity) || 0)} USDC</dd>
        </div>
        <div>
          <dt>許容スリッページ</dt>
          <dd>0.5%</dd>
        </div>
        <div>
          <dt>実際の執行</dt>
          <dd>なし · DEMO</dd>
        </div>
      </dl>
      <button
        className={side === 'buy' ? 'primary full' : 'danger full'}
        disabled={!state.active || stale || frozen}
        onClick={submit}
      >
        {!state.active
          ? 'デモセッションを開始してください'
          : stale
            ? '口座データ遅延 · 注文停止'
            : frozen
              ? 'この要求は受付済みです'
              : 'デモ注文を送信'}
      </button>
      {message && (
        <p role="alert" className="warning">
          {message}
        </p>
      )}
      {submitted && (
        <output className="order-result">
          {labels[submitted.status]}
          {!['queued', 'unknown'].includes(submitted.status) && (
            <button
              className="text-button"
              onClick={() => {
                setRequestId(null)
                setMessage('')
              }}
            >
              新しい注文を入力
            </button>
          )}
        </output>
      )}
      <p className="fineprint">
        署名・実注文なし。板・価格は合成です。結果不明の要求は再送せず、状態を保持します。
      </p>
    </>
  )
}

function Orders() {
  const state = useDemo()
  const [confirm, setConfirm] = useState(false)
  // React Compiler is not enabled; Table v8's mutable instance is consumed in this component only.
  // oxlint-disable-next-line react/incompatible-library
  const table = useReactTable({ data: state.orders, columns, getCoreRowModel: getCoreRowModel() })
  if (!state.orders.length)
    return (
      <div className="empty">
        <span className="empty-icon">≋</span>
        <strong>注文はまだありません</strong>
        <p>デモセッションを開始して、注文と照合状態を確認できます。</p>
      </div>
    )
  return (
    <>
      <div className="table-actions">
        <button onClick={() => setConfirm(true)}>全注文の取消を確認</button>
        {confirm && (
          <div role="alert">
            <span>保護用SL/TPも取り消す操作です。建玉は決済しません。</span>
            <button
              onClick={() => {
                cancelAllDemo()
                setConfirm(false)
              }}
            >
              全取消を実行（模擬）
            </button>
            <button onClick={() => setConfirm(false)}>戻る</button>
          </div>
        )}
      </div>
      <div className="table-scroll">
        <table>
          <thead>
            {table.getHeaderGroups().map((group) => (
              <tr key={group.id}>
                {group.headers.map((header) => (
                  <th key={header.id}>
                    {flexRender(header.column.columnDef.header, header.getContext())}
                  </th>
                ))}
              </tr>
            ))}
          </thead>
          <tbody>
            {table.getRowModel().rows.map((row) => (
              <tr key={row.id}>
                {row.getVisibleCells().map((cell) => (
                  <td key={cell.id}>{flexRender(cell.column.columnDef.cell, cell.getContext())}</td>
                ))}
              </tr>
            ))}
          </tbody>
        </table>
      </div>
    </>
  )
}

function Funds() {
  const state = useDemo()
  const [amount, setAmount] = useState('100')
  const [kind, setKind] = useState<'deposit' | 'allocate' | 'recover' | 'withdraw'>('allocate')
  const [confirmation, setConfirmation] = useState<{
    id: string
    amount: number
    kind: typeof kind
  } | null>(null)
  const [message, setMessage] = useState('')
  const names = {
    deposit: '合成USDCを追加',
    allocate: '取引口座へ配分',
    recover: '保管残高へ回収',
    withdraw: '本人宛出金を模擬',
  }
  function prepare() {
    try {
      setConfirmation({ id: crypto.randomUUID(), amount: parseUsdc(amount), kind })
      setMessage('')
    } catch (error) {
      setMessage(error instanceof Error ? error.message : '金額を確認してください。')
    }
  }
  function execute() {
    // Nothing to sign off when the confirmed amount rounds to a zero display.
    if (!confirmation || !Number.isSafeInteger(confirmation.amount) || confirmation.amount <= 0) {
      setConfirmation(null)
      setMessage('金額を確認してください。')
      return
    }
    try {
      transferDemo(confirmation.id, confirmation.kind, confirmation.amount)
      setConfirmation(null)
      setMessage('合成台帳を更新しました。実送金はありません。')
    } catch (error) {
      setMessage(error instanceof Error ? error.message : '処理できませんでした。')
      setConfirmation(null)
    }
  }
  return (
    <main className="page">
      <div className="page-title">
        <div className="eyebrow">CUSTODY / ALLOCATION</div>
        <h1>資金を、見渡す。</h1>
        <p>保管・取引・移動中の資金を分けて確認します。</p>
      </div>
      <div className="balance-grid" data-testid="account-balances">
        <Balance name="保管残高" value={state.reserve / 1e6} detail="取引口座へ未配分" />
        <Balance
          name="取引口座の合成残高"
          value={state.trading / 1e6}
          detail="PnL・証拠金拘束は未接続"
        />
        <Balance name="移動中" value={0} detail="デモでは即時処理" />
        <Balance
          name="出金可能額（模擬）"
          value={state.reserve / 1e6}
          detail="保管残高のみ・実額ではありません"
        />
      </div>
      <div className="fund-grid">
        <section className="panel fund-form">
          <h2>資金移動シミュレーション</h2>
          <label>
            操作
            <select
              value={kind}
              disabled={!!confirmation}
              onChange={(event) => setKind(event.target.value as typeof kind)}
            >
              {Object.entries(names).map(([key, value]) => (
                <option value={key} key={key}>
                  {value}
                </option>
              ))}
            </select>
          </label>
          <label>
            金額（USDC）
            <input
              aria-label="資金移動額"
              inputMode="decimal"
              value={amount}
              disabled={!!confirmation}
              onChange={(event) => setAmount(event.target.value)}
            />
          </label>
          <button className="primary" disabled={!state.active || !!confirmation} onClick={prepare}>
            内容を確認
          </button>
          {confirmation && (
            <dialog open className="confirm" aria-label="資金移動の確認">
              <h3>{names[confirmation.kind]}</h3>
              <p>{money(confirmation.amount / 1e6)} USDC</p>
              <p>
                宛先：
                {confirmation.kind === 'withdraw'
                  ? '本人HL口座（模擬・アドレス未登録）'
                  : '合成台帳内'}
              </p>
              <p>実際のウォレット署名・送金は行いません。</p>
              <button className="primary" onClick={execute}>
                模擬実行
              </button>
              <button className="secondary" onClick={() => setConfirmation(null)}>
                戻る
              </button>
            </dialog>
          )}
          {message && <output>{message}</output>}
        </section>
        <aside className="panel fund-explainer">
          <div className="eyebrow">KNOW THE BOUNDARIES</div>
          <h2>資金の経路</h2>
          <ol>
            <li>本人のHL口座</li>
            <li>共通保管口座 / funds_vault</li>
            <li>ユーザー別HL取引口座</li>
            <li>回収後、本人のHL口座へ</li>
          </ol>
          <p>
            Canisterがmaster鍵を管理する設計です。停止時の単独出金や、EOA紛失時の救済は保証しません。
          </p>
          <p className="warning">共通保管を経由するだけでは、金額・時刻の相関は隠れません。</p>
        </aside>
      </div>
    </main>
  )
}
function Balance({ name, value, detail }: { name: string; value: number; detail: string }) {
  return (
    <article className="panel balance">
      <h2>{name}</h2>
      <div>
        {money(value)} <small>USDC</small>
      </div>
      <p>{detail}</p>
    </article>
  )
}
function History() {
  const state = useDemo()
  return (
    <main className="page">
      <div className="page-title">
        <div className="eyebrow">ACTIVITY / SESSION ONLY</div>
        <h1>履歴を確認する。</h1>
        <p>このブラウザのデモセッション内だけに保持します。再読込・終了で消去されます。</p>
      </div>
      <section className="panel">
        <div className="panel-heading">
          <h2>注文・約定状態</h2>
          <span>合成データ</span>
        </div>
        <Orders />
      </section>
      <section className="panel history-funds">
        <div className="panel-heading">
          <h2>資金履歴</h2>
        </div>
        {state.events.length ? (
          <div className="table-scroll">
            <table>
              <thead>
                <tr>
                  <th>時刻</th>
                  <th>操作</th>
                  <th>金額 (USDC)</th>
                  <th>状態</th>
                </tr>
              </thead>
              <tbody>
                {state.events.map((event) => (
                  <tr key={event.id}>
                    <td>{event.at}</td>
                    <td>{event.kind}</td>
                    <td>{money(event.amount / 1e6)}</td>
                    <td>模擬完了 · 実送金なし</td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        ) : (
          <div className="empty">資金操作の履歴はありません。</div>
        )}
      </section>
    </main>
  )
}
