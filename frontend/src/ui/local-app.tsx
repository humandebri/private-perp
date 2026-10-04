import { useEffect, useMemo, useRef, useState, type ReactNode } from 'react'
import { Link } from '@tanstack/react-router'
import { bytesToHex } from '../client/wallet'
import { variantName } from '../client/result'
import type { Fill, OrderSummary, Position } from '../client/candid-codec'
import type { FundEvent } from '../client/candid/funds_vault.did.js'
import { useLocalSession } from './local-session'
import { PriceChart, useMarket } from './market'
import { aggregateCandles } from '../client/chart-data'
import { availableMargin, decimalText, quoteOrder } from '../client/order-ticket'

const micros = (value: bigint | undefined) =>
  value === undefined ? '—' : `${value / 1_000_000n}.${String(value % 1_000_000n).padStart(6, '0')}`
const signedMicros = (value: bigint | undefined) =>
  value === undefined ? '—' : `${value < 0n ? '-' : ''}${micros(value < 0n ? -value : value)}`
const amountMicros = (value: string) => {
  if (!/^\d+(\.\d{1,6})?$/.test(value))
    throw new Error('Enter USDC with no more than six decimal places')
  const [whole, fraction = ''] = value.split('.')
  const result = BigInt(whole) * 1_000_000n + BigInt(fraction.padEnd(6, '0'))
  if (result <= 0n) throw new Error('Amount must be greater than zero')
  return result
}

function Workspace({
  children,
  publicContent = false,
  compact = false,
}: {
  children: ReactNode
  publicContent?: boolean
  compact?: boolean
}) {
  const session = useLocalSession()
  const data = session.data
  const agentExpired = Boolean(
    data?.agent?.current[0]?.expires_at[0] &&
    data.agent.current[0].expires_at[0] <= BigInt(session.wallNow),
  )
  const journalLocked = Boolean(data?.vaultJournal?.[0] || data?.coreJournal?.[0])
  const stopReasons = [
    journalLocked &&
      (data?.vaultJournal?.[1] || data?.coreJournal?.[1]
        ? 'Restored records are awaiting reconciliation'
        : 'Waiting for dispatch journal verification'),
    !data?.eligibility?.eligible && 'Eligibility is missing or expired',
    (data?.vaultCycles?.new_risk_stopped || data?.coreCycles?.new_risk_stopped) &&
      'Check the cycles balance and configuration',
    agentExpired && 'Agent approval has expired',
  ].filter((reason): reason is string => Boolean(reason))
  return (
    <main className={`local-workspace ${compact ? 'compact-workspace' : ''}`}>
      <section className="workspace-bar">
        <div>
          <div className="eyebrow">
            {import.meta.env.VITE_APP_STAGE === 'testnet'
              ? 'PUBLIC CANISTER / HL TESTNET'
              : 'LOCAL CANISTER / REAL CONNECTION'}
          </div>
          <strong>
            {session.address
              ? `${session.address.slice(0, 10)}…${session.address.slice(-6)}`
              : 'Disconnected'}
          </strong>
        </div>
        <div className="session-controls">
          {session.address && (
            <button disabled={session.busy} onClick={() => void session.refresh()}>
              Refresh
            </button>
          )}
          <button
            className={session.address ? 'danger' : 'primary'}
            disabled={session.busy && !session.address}
            onClick={() => void (session.address ? session.logout() : session.login())}
          >
            {session.address ? 'Log out' : 'Connect MetaMask'}
          </button>
        </div>
      </section>
      <div className="local-strip">
        <b>{import.meta.env.VITE_APP_STAGE === 'testnet' ? 'HL TESTNET' : 'LOCAL MOCK'}</b>
        <span>
          {import.meta.env.VITE_APP_STAGE === 'testnet'
            ? 'Public canister · Hyperliquid testnet · Test USDC'
            : 'Real canister · Mock venue · No real funds'}
        </span>
      </div>
      {session.busy && (
        <output className="operation-status">
          Processing. Check MetaMask if a signature is requested.
        </output>
      )}
      {session.error && (
        <div role="alert" className="error-banner">
          {session.error}
        </div>
      )}
      {session.refreshError && (
        <div role="alert" className="warning-banner">
          Refresh failed: {session.refreshError}. New orders are blocked while account data is
          stale.
        </div>
      )}
      {session.data?.issues.length ? (
        <div role="alert" className="warning-banner">
          Some data could not be loaded:
          {session.data.issues.map((issue) => `${issue.source} (${issue.message})`).join(' / ')}
        </div>
      ) : null}
      {data && stopReasons.length > 0 && (
        <details className="warning-banner account-notice">
          <summary>New actions paused · {stopReasons.join(' / ')}</summary>
          <p>
            {journalLocked
              ? 'You can view balances and history. Dispatch resumes after journal reconciliation.'
              : 'Cancellation, reduce-only closes, recovery and withdrawals remain available.'}
          </p>
        </details>
      )}
      {!session.address && !publicContent ? (
        <section className="empty-state">
          <h1>
            {import.meta.env.VITE_APP_STAGE === 'testnet'
              ? 'Start a testnet session'
              : 'Start a local session'}
          </h1>
          <p>
            Start with “Connect MetaMask” above.
            {import.meta.env.VITE_APP_STAGE === 'testnet'
              ? 'Use test USDC only.'
              : 'No real funds are used.'}
          </p>
          <p>Session data stays in this tab and is discarded on reload or logout.</p>
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
        <span>Custody balance</span>
        <strong>{micros(data?.funds.reserve_unallocated)}</strong>
        <small>USDC</small>
      </article>
      <article>
        <span>Trading balance</span>
        <strong>{micros(data?.funds.trading_equity)}</strong>
        <small>USDC</small>
      </article>
      <article>
        <span>Available to withdraw from custody</span>
        <strong>{micros(data?.funds.withdrawable)}</strong>
        <small>USDC</small>
      </article>
    </div>
  )
}

function TradeOverview() {
  const { data } = useLocalSession()
  if (!data) return null
  return (
    <section
      className="trade-overview"
      aria-label="Trading account overview"
      data-testid="trade-overview"
    >
      <div>
        <span>Trading equity</span>
        <strong>{micros(data.funds.trading_equity)} USDC</strong>
      </div>
      <div>
        <span>Custody available to withdraw</span>
        <strong>{micros(data.funds.withdrawable)} USDC</strong>
      </div>
      <div>
        <span>Positions</span>
        <strong>{data.snapshot?.positions.length ?? '—'}</strong>
      </div>
      <Link to="/funds">Manage funds</Link>
    </section>
  )
}

export function FundsApp() {
  const {
    gateway,
    store,
    fundingInstructions,
    run: execute,
    busy,
    data,
    wallNow,
    age,
    fresh,
  } = useLocalSession()
  const isTestnet = import.meta.env.VITE_APP_STAGE === 'testnet'
  const [amount, setAmount] = useState('100')
  const [eligibilityClaims, setEligibilityClaims] = useState('')
  const [eligibilitySignature, setEligibilitySignature] = useState('')
  const newFundsStopped =
    !data?.eligibility?.eligible ||
    !data?.vaultCycles ||
    data.vaultCycles.new_risk_stopped ||
    !data.vaultJournal ||
    data.vaultJournal[0]
  const action = (kind: string) => async () => {
    if (kind === 'seed' && isTestnet) {
      await store.loadFundingInstructions()
      return
    }
    const value = amountMicros(amount)
    if (kind === 'seed') await gateway.depositToReserve(amount, value)
    if (kind === 'allocate') await gateway.allocateFromReserve(value)
    if (kind === 'recover') await gateway.recover(value)
    if (kind === 'withdraw') await gateway.withdraw(value)
  }
  return (
    <Workspace>
      <div data-testid="account-panel" className="content-shell">
        <StatusCards />
        <div className="two-column">
          <section className="panel action-panel">
            <h1>Funding</h1>
            <details className="eligibility-panel" open={!data?.eligibility?.eligible}>
              <summary>
                Eligibility: {data?.eligibility?.eligible ? 'Registered' : 'Missing or expired'}
              </summary>
              <button
                type="button"
                disabled={busy}
                onClick={() =>
                  void execute(async () => {
                    setEligibilityClaims(bytesToHex(await gateway.eligibilitySigningClaims()))
                  })
                }
              >
                Get signing claims
              </button>
              {eligibilityClaims && (
                <label>
                  Candid claims for the issuer (never enter a private key)
                  <textarea readOnly value={eligibilityClaims} rows={3} />
                </label>
              )}
              <label>
                Signature returned by the issuer
                <input
                  value={eligibilitySignature}
                  onChange={(event) => setEligibilitySignature(event.target.value)}
                  placeholder="0x…"
                />
              </label>
              <button
                type="button"
                disabled={busy || !eligibilityClaims || !eligibilitySignature}
                onClick={() =>
                  void execute(async () => {
                    await gateway.registerEligibility(eligibilityClaims, eligibilitySignature)
                  })
                }
              >
                Register eligibility
              </button>
            </details>
            <p>
              Deposits can stay in custody. When you want to trade, use “Allocate to trading” to
              move only the amount you need.
            </p>
            <p>
              Send only from your connected HL account. A shared reserve does not yet prevent amount
              and timing correlation.
            </p>
            <form>
              <label>
                Amount (USDC)
                <input
                  aria-label="Amount"
                  inputMode="decimal"
                  value={amount}
                  onChange={(event) => setAmount(event.target.value)}
                />
              </label>
              <div className="action-grid">
                <button
                  type="button"
                  disabled={busy || newFundsStopped}
                  onClick={() => void execute(action('seed'))}
                >
                  {isTestnet ? 'Show HL testnet deposit address' : 'Deposit to LOCAL MOCK custody'}
                </button>
                <button
                  type="button"
                  disabled={busy || newFundsStopped}
                  onClick={() => void execute(action('allocate'))}
                >
                  Allocate to trading
                </button>
                <button
                  type="button"
                  disabled={busy}
                  onClick={() => void execute(action('withdraw'))}
                >
                  Withdraw with MetaMask
                </button>
              </div>
              {isTestnet && fundingInstructions && (
                <p>
                  Send USDC from your connected HL testnet account{' '}
                  <strong>{bytesToHex(fundingInstructions.source_hl_account_address)}</strong> to
                  the reserve account{' '}
                  <strong>{bytesToHex(fundingInstructions.hl_account_address)}</strong>. After
                  sending, refresh balances to confirm the deposit.
                </p>
              )}
              <details>
                <summary>Adjust balances</summary>
                <p>
                  Move trading funds back to custody. Withdrawals automatically recover only the
                  shortfall.
                </p>
                <div className="action-grid">
                  <button
                    type="button"
                    disabled={busy}
                    onClick={() => void execute(action('recover'))}
                  >
                    Return to custody
                  </button>
                </div>
              </details>
            </form>
          </section>
          <details
            className="panel account-details"
            open={Boolean(
              data?.funds.unknowns.length ||
              data?.funds.recovery_fence.length ||
              data?.vaultCycles?.new_risk_stopped ||
              data?.coreCycles?.new_risk_stopped ||
              data?.vaultJournal?.[0] ||
              data?.coreJournal?.[0] ||
              (data?.agent?.current[0]?.expires_at[0] &&
                data.agent.current[0].expires_at[0] <= BigInt(wallNow)),
            )}
          >
            <summary>Funding and operations details</summary>
            <h2>Funding status</h2>
            <dl className="details">
              <dt>Margin used</dt>
              <dd>{micros(data?.snapshot?.margin_used)} USDC</dd>
              <dt>Unrealized PnL</dt>
              <dd>{signedMicros(data?.snapshot?.unrealized_pnl)} USDC</dd>
              <dt>Order risk reservation</dt>
              <dd>{micros(data?.snapshot?.open_order_risk_reserved)} USDC</dd>
              <dt>Account data</dt>
              <dd>
                {data?.snapshot
                  ? `${Number.isFinite(age) ? Math.floor(age) : '—'} ms${fresh ? '' : ' / Refresh pending'}`
                  : 'Not observed'}
              </dd>
              <dt>in transit</dt>
              <dd>{micros(data?.funds.in_transit)}</dd>
              <dt>withdrawal reserve</dt>
              <dd>{micros(data?.funds.reserved_for_withdrawal)}</dd>
              <dt>unknown</dt>
              <dd>{data?.funds.unknowns.length ?? 0}</dd>
              <dt>Dispatch journal / vault</dt>
              <dd>
                {!data?.vaultJournal
                  ? 'Loading'
                  : data.vaultJournal[0]
                    ? 'Awaiting reconciliation'
                    : 'Healthy'}
              </dd>
              <dt>Dispatch journal / core</dt>
              <dd>
                {!data?.coreJournal
                  ? 'Loading'
                  : data.coreJournal[0]
                    ? 'Awaiting reconciliation'
                    : 'Healthy'}
              </dd>
              <dt>Recovery fence</dt>
              <dd>
                {data?.funds.recovery_fence.length
                  ? variantName(data.funds.recovery_fence[0]) === 'Preparing'
                    ? 'Preparing recovery'
                    : 'Awaiting reconciliation'
                  : 'None'}
              </dd>
              <dt>revision</dt>
              <dd>{data?.funds.revision.toString() ?? '—'}</dd>
              <dt>Eligibility</dt>
              <dd>
                {data?.eligibility?.eligible
                  ? `Valid (terms ${data.eligibility.terms_version})`
                  : 'Missing or expired'}
              </dd>
              <dt>cycles / vault</dt>
              <dd>
                {data?.vaultCycles?.estimated_days[0]?.toString() ?? 'Unknown'} days ·{' '}
                {data?.vaultCycles?.new_risk_stopped ? 'New actions paused' : 'Ready'}
              </dd>
              <dt>cycles / core</dt>
              <dd>
                {data?.coreCycles?.estimated_days[0]?.toString() ?? 'Unknown'} days ·{' '}
                {data?.coreCycles?.new_risk_stopped ? 'New actions paused' : 'Ready'}
              </dd>
            </dl>
            <h2>Agent</h2>
            <dl className="details">
              <dt>state</dt>
              <dd>
                {data?.agent?.current[0]
                  ? variantName(data.agent.current[0].state)
                  : 'Not approved'}
              </dd>
              <dt>generation</dt>
              <dd>{data?.agent?.current[0]?.generation.toString() ?? '—'}</dd>
              <dt>expires</dt>
              <dd>
                {data?.agent?.current[0]?.expires_at[0]
                  ? new Date(Number(data.agent.current[0].expires_at[0])).toLocaleString('en-US')
                  : 'No expiry'}
              </dd>
              <dt>revocation</dt>
              <dd>{data?.agent?.revocation_pending ? 'Pending' : 'None'}</dd>
            </dl>
          </details>
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
    wallNow,
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
    [amount, setAmount] = useState('10'),
    [price, setPrice] = useState('')
  const [leverage, setLeverage] = useState('3')
  const [slippage, setSlippage] = useState('50')
  const [unit, setUnit] = useState<'margin' | 'coin'>('margin')
  const [interval, setInterval] = useState(5)
  const [isMobile, setIsMobile] = useState(false)
  const orderDialog = useRef<HTMLDialogElement>(null)
  useEffect(() => {
    const query = window.matchMedia('(max-width: 760px)')
    const update = () => {
      if (query.matches) orderDialog.current?.close()
      setIsMobile(query.matches)
    }
    update()
    query.addEventListener('change', update)
    return () => query.removeEventListener('change', update)
  }, [])
  const candles = useMemo(
    () => aggregateCandles(publicMarket.candles[market] ?? [], interval),
    [publicMarket.candles, market, interval],
  )
  const position = data?.snapshot?.positions.find((item) => item.market === market)
  const chartLevels = useMemo(
    () =>
      position
        ? [
            { title: 'Entry', price: Number(position.entry_price), color: '#dce8e9' },
            ...position.stop_loss.map((value) => ({
              title: 'SL',
              price: Number(value),
              color: '#ef8d99',
            })),
            ...position.take_profit.map((value) => ({
              title: 'TP',
              price: Number(value),
              color: '#8ce3c5',
            })),
          ]
        : [],
    [position],
  )
  const available =
    data?.snapshot && fresh
      ? availableMargin(
          data.snapshot.equity,
          data.snapshot.margin_used,
          data.snapshot.open_order_risk_reserved,
        )
      : undefined
  const reference = publicMarket.mids[market] ?? ''
  const asset = publicMarket.assets?.[market]
  const context = publicMarket.assetContext?.[market]
  const change =
    context && Number(context.prevDayPx) > 0
      ? (Number(context.markPx) / Number(context.prevDayPx) - 1) * 100
      : undefined
  const quote = (now: number) => {
    if (
      publicMarket.connection !== 'live' ||
      !publicMarket.midsObservedAt ||
      now - publicMarket.midsObservedAt > 10_000
    )
      throw new Error('Waiting for fresh market data.')
    if (!asset) throw new Error('Waiting for market precision. Reload if this persists.')
    if (Number(leverage) > asset.maxLeverage)
      throw new Error('Leverage exceeds this market’s limit.')
    return quoteOrder({
      amount,
      unit,
      reference,
      limit: price,
      kind,
      side,
      leverage: Number(leverage),
      slippageBps: Number(slippage),
      sizeDecimals: asset.sizeDecimals,
    })
  }
  let preview: ReturnType<typeof quote> | undefined
  let inputError: string | undefined
  try {
    preview = quote(wallNow)
    if (available !== undefined && preview.margin > available)
      inputError = 'Insufficient available margin. Add funds or lower the amount.'
  } catch (error) {
    inputError = error instanceof Error ? error.message : String(error)
  }

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
  const submit = () => {
    try {
      const current = quote(Date.now())
      if (available === undefined || current.margin > available || !canOrder) return
      void submitOrder({
        market,
        side,
        kind,
        quantity: current.quantity,
        price: current.price,
        leverage: Number(leverage),
        slippageBps: Number(slippage),
      })
    } catch {
      /* Invalid or stale quotes remain blocked by the ticket. */
    }
  }
  const book = publicMarket.book[market] ?? [[], []]
  const trades = publicMarket.trades[market] ?? []
  return (
    <Workspace publicContent compact>
      <div data-testid="account-panel" className="content-shell">
        <section className="market-head">
          <div>
            <label className="market-selector">
              Market
              <select
                aria-label="Symbol"
                value={market}
                onChange={(event) => {
                  setMarket(event.target.value)
                  setPrice('')
                  if (unit === 'coin') setAmount('')
                }}
              >
                <option>BTC</option>
                <option>ETH</option>
              </select>
            </label>
            <strong>
              {Number(reference) > 0
                ? Number(reference).toLocaleString('en-US', { maximumFractionDigits: 2 })
                : '—'}{' '}
              <small>USDC · Mid</small>
            </strong>
          </div>
          {publicMarket.connection !== 'live' && (
            <span className={`state-chip ${publicMarket.connection}`}>
              Market data ·{' '}
              {publicMarket.connection === 'reconnecting' ? 'Reconnecting' : 'Disconnected'}
            </span>
          )}
          {data && !fresh && (
            <span className="state-chip warning">Account data refresh pending</span>
          )}
          {data &&
            ![data.btcMarket, data.ethMarket].find((item) => item?.market === market)
              ?.eligible_for_new_risk && (
              <span className="state-chip warning">
                Market monitoring ·{' '}
                {[data.btcMarket, data.ethMarket].find((item) => item?.market === market)
                  ?.reason_code[0] ?? 'Awaiting observation'}
              </span>
            )}
        </section>
        <div className="market-metrics">
          <span>
            24h change{' '}
            <b className={(change ?? 0) >= 0 ? 'positive' : 'negative'}>
              {change !== undefined && Number.isFinite(change)
                ? `${change >= 0 ? '+' : ''}${change.toFixed(2)}%`
                : '—'}
            </b>
          </span>
          <span>
            Funding / hour{' '}
            <b>
              {context && Number.isFinite(Number(context.funding))
                ? `${(Number(context.funding) * 100).toFixed(4)}%`
                : '—'}
            </b>
          </span>
          <span>
            24h volume{' '}
            <b>
              {context && Number.isFinite(Number(context.dayNtlVlm))
                ? `$${Number(context.dayNtlVlm).toLocaleString('en-US', { maximumFractionDigits: 0 })}`
                : '—'}
            </b>
          </span>
        </div>
        <TradeOverview />
        <nav className="trade-shortcuts" aria-label="Trading navigation">
          <button className="mobile-order-link" onClick={() => orderDialog.current?.showModal()}>
            Trade
          </button>
          <a href="#market-chart">Chart</a>
          <a href="#positions">Positions</a>
          <a href="#order-status">Order status</a>
        </nav>
        <nav className="mobile-exit-actions" aria-label="Exit actions">
          <button
            disabled={
              busy ||
              !data?.orders?.items.some((order) =>
                ['Open', 'PartiallyFilled'].includes(variantName(order.state)),
              )
            }
            onClick={() =>
              window.confirm(
                'Cancel all non-terminal orders, including protective SL/TP orders. Continue?',
              ) && void execute(() => gateway.cancelAll())
            }
          >
            Cancel all
          </button>
          <button
            disabled={busy || !data?.snapshot?.positions.length}
            onClick={() =>
              window.confirm('Close all positions with reduce-only orders. Continue?') &&
              void execute(() => gateway.closeAll())
            }
          >
            Close all
          </button>
          <Link to="/funds">Recover / withdraw</Link>
        </nav>
        <div className="trading-grid">
          <section className="panel chart-panel" id="market-chart">
            <div className="panel-title">
              <h2>{market} / USDC</h2>
              <span>
                {import.meta.env.VITE_APP_STAGE === 'testnet'
                  ? 'HL testnet · UTC'
                  : 'Local mock · UTC'}
              </span>
            </div>
            <fieldset className="chart-intervals" aria-label="Chart interval">
              {[1, 5, 15, 60].map((minutes) => (
                <button
                  key={minutes}
                  aria-pressed={interval === minutes}
                  onClick={() => setInterval(minutes)}
                >
                  {minutes === 60 ? '1h' : `${minutes}m`}
                </button>
              ))}
              <span>Last 24h · Volume</span>
            </fieldset>
            {candles.length ? (
              <>
                <PriceChart key={`${market}-${interval}`} candles={candles} levels={chartLevels} />
                <p className="muted">
                  {candles.length} candles · Latest candle:{' '}
                  {new Date(candles.at(-1)!.t).toLocaleString('en-US', {
                    timeZone: 'UTC',
                    hour12: false,
                  })}{' '}
                  UTC
                  {' · '}Close: {candles.at(-1)!.c} USDC
                </p>
              </>
            ) : (
              <div className="chart-empty">
                <strong>Waiting for chart data</strong>
                <p>If data does not appear, check the market feed connection and configuration.</p>
              </div>
            )}
            {publicMarket.candleHistory?.[market] === 'error' && (
              <output>
                Could not load candle history. Reload the page to refresh. Any displayed candles are
                from the live feed or previously loaded history.
              </output>
            )}
          </section>
          <dialog ref={orderDialog} open={!isMobile} className="order-dialog">
            <section className="panel action-panel order-ticket" id="order-ticket">
              <div className="panel-title">
                <h1>Trade {market}</h1>
                <button
                  className="mobile-ticket-close"
                  onClick={() => orderDialog.current?.close()}
                  aria-label="Close order panel"
                >
                  Close
                </button>
              </div>
              <fieldset className="direction-switch" aria-label="Order direction">
                <button
                  aria-pressed={side === 'buy'}
                  className="long"
                  onClick={() => setSide('buy')}
                >
                  Long
                </button>
                <button
                  aria-pressed={side === 'sell'}
                  className="short"
                  onClick={() => setSide('sell')}
                >
                  Short
                </button>
              </fieldset>
              <fieldset className="order-kind" aria-label="Order type">
                {(['market', 'limit'] as const).map((value) => (
                  <button key={value} aria-pressed={kind === value} onClick={() => setKind(value)}>
                    {value === 'market' ? 'Market' : 'Limit'}
                  </button>
                ))}
              </fieldset>
              <div className="ticket-balance">
                <span>Available margin</span>
                <strong>{available === undefined ? '—' : `${decimalText(available)} USDC`}</strong>
              </div>
              <Link
                className="funding-link"
                to="/funds"
                onClick={() => orderDialog.current?.close()}
              >
                {data?.funds.reserve_unallocated ? 'Allocate held funds' : 'Add funds'}
              </Link>
              <div className="form-row two">
                <label>
                  Amount unit
                  <select
                    aria-label="Amount unit"
                    value={unit}
                    onChange={(event) => {
                      setUnit(event.target.value as 'margin' | 'coin')
                      setAmount('')
                    }}
                  >
                    <option value="margin">Margin · USDC</option>
                    <option value="coin">Quantity · {market}</option>
                  </select>
                </label>
                <label>
                  Leverage
                  <select
                    aria-label="Leverage"
                    value={leverage}
                    onChange={(event) => setLeverage(event.target.value)}
                  >
                    {[1, 2, 3, 4, 5].map((value) => (
                      <option key={value} value={value}>
                        {value}x
                      </option>
                    ))}
                  </select>
                </label>
              </div>
              <label>
                {unit === 'margin' ? 'Margin amount (USDC)' : `Quantity (${market})`}
                <input
                  aria-label="Order amount"
                  inputMode="decimal"
                  value={amount}
                  onChange={(event) => setAmount(event.target.value)}
                  placeholder={unit === 'margin' ? '0.00' : '0.00000'}
                />
              </label>
              {unit === 'margin' && (
                <fieldset className="amount-presets" aria-label="Use available margin">
                  {[25, 50, 75, 99].map((percent) => (
                    <button
                      key={percent}
                      disabled={available === undefined || available === 0n}
                      onClick={() => setAmount(decimalText((available! * BigInt(percent)) / 100n))}
                    >
                      {percent === 99 ? 'Max · 99%' : `${percent}%`}
                    </button>
                  ))}
                </fieldset>
              )}
              {kind === 'limit' && (
                <label>
                  Limit price (USDC)
                  <input
                    aria-label="Order price"
                    inputMode="decimal"
                    value={price}
                    onChange={(event) => setPrice(event.target.value)}
                  />
                  <button
                    className="reference-price"
                    disabled={!reference}
                    onClick={() => setPrice(reference)}
                  >
                    Use reference price
                  </button>
                </label>
              )}
              <dl className="order-estimate">
                <dt>Position value (estimate)</dt>
                <dd>{preview ? `${decimalText(preview.notional)} USDC` : '—'}</dd>
                <dt>Quantity</dt>
                <dd>{preview ? `${preview.quantity} ${market}` : '—'}</dd>
                <dt>Margin (estimate)</dt>
                <dd>{preview ? `${decimalText(preview.margin)} USDC` : '—'}</dd>
              </dl>
              <details className="advanced-order">
                <summary>Advanced settings</summary>
                <label>
                  Slippage (bps)
                  <input
                    aria-label="Slippage"
                    inputMode="numeric"
                    disabled={kind === 'limit'}
                    value={slippage}
                    onChange={(event) => setSlippage(event.target.value)}
                  />
                </label>
                <p className="ticket-help">
                  {kind === 'market'
                    ? `Immediate-or-cancel. ${slippage} bps maximum slippage. ${side === 'buy' ? 'Maximum buy' : 'Minimum sell'} price: ${preview?.price ?? '—'} USDC.`
                    : 'Good-til-cancel. The order rests at your limit price until filled or canceled.'}
                </p>
              </details>
              <button
                className={`primary ${side === 'sell' ? 'sell-submit' : ''}`}
                aria-describedby="order-guidance"
                disabled={busy || !canOrder || available === undefined || Boolean(inputError)}
                onClick={submit}
              >
                {busy ? 'Processing…' : `Open ${side === 'buy' ? 'long' : 'short'}`}
              </button>
              <div id="order-guidance" className="order-guidance" aria-live="polite">
                {busy
                  ? 'Processing. Wait without submitting again.'
                  : (disableReason ??
                    inputError ??
                    'Review the estimates before opening your position.')}
              </div>
              <p className="ticket-help">
                Estimates exclude fees and funding. Reservations are deducted from available margin.
                Max leaves 1% unallocated.
              </p>
              {data && !activeAgent && (
                <button disabled={busy} onClick={() => void execute(() => gateway.approveAgent())}>
                  Approve trading
                </button>
              )}
              {position && (
                <p className="ticket-help">
                  Current position: {position.size} {market}. An opposite order can reduce or
                  reverse it.
                </p>
              )}
            </section>
          </dialog>
          <details className="panel market-tape">
            <summary>View order book and public trades</summary>
            <h2>Order book</h2>
            <div className="book-labels">
              <span>Price (USDC)</span>
              <span>Quantity ({market})</span>
            </div>
            {!book[0].length && !book[1].length && <p>Waiting for order book data.</p>}
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
            <h2>Public trades</h2>
            {!trades.length && <p>No trade data yet.</p>}
            <div className="trade-tape">
              {trades.map((trade) => (
                <div key={trade.tid} className={trade.side === 'B' ? 'positive' : 'negative'}>
                  <span>{trade.px}</span>
                  <span>{trade.sz}</span>
                </div>
              ))}
            </div>
          </details>
        </div>
        {data && (
          <section className="panel table-panel" id="positions">
            <div className="panel-title">
              <h2>Positions</h2>
              <button
                className="danger"
                disabled={busy || !data.snapshot?.positions.length}
                onClick={() =>
                  window.confirm('Close all positions with reduce-only orders. Continue?') &&
                  void execute(() => gateway.closeAll())
                }
              >
                Close all
              </button>
            </div>
            {data.snapshot?.positions.length ? (
              <table className="responsive-table">
                <thead>
                  <tr>
                    <th>Market</th>
                    <th>Quantity</th>
                    <th>Entry</th>
                    <th>PnL</th>
                    <th>Liquidation price</th>
                    <th>Leverage</th>
                    <th>Protection / close</th>
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
            ) : (
              <p className="table-empty">No positions. Filled positions will appear here.</p>
            )}
          </section>
        )}
        <div className="mobile-trade-bar">
          <span>
            {market} · {reference || '—'}
          </span>
          <button className="primary" onClick={() => orderDialog.current?.showModal()}>
            Trade {market}
          </button>
        </div>
        <section className="panel table-panel" id="order-status">
          <div className="panel-title">
            <h2>Order status</h2>
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
                  'Cancel all non-terminal orders, including protective SL/TP orders. Continue?',
                ) && void execute(() => gateway.cancelAll())
              }
            >
              Cancel all
            </button>
          </div>
          <table className="responsive-table">
            <thead>
              <tr>
                <th>Market</th>
                <th>Type</th>
                <th>State</th>
                <th>Filled / ordered quantity</th>
                <th>Action</th>
              </tr>
            </thead>
            <tbody>
              {optimistic.map((order) => (
                <tr key={bytesToHex(order.id)} className="pending-row">
                  <td data-label="Market">{order.market}</td>
                  <td data-label="Type">—</td>
                  <td data-label="State">
                    {
                      {
                        sending: 'Sending',
                        accepted: 'Accepted; awaiting reconciliation',
                        rejected: 'Rejected',
                        unknown: 'Unknown response; do not resubmit',
                      }[order.state]
                    }
                    {order.message && <small>{order.message}</small>}
                  </td>
                  <td data-label="Filled / ordered quantity">—</td>
                  <td data-label="Action">
                    {order.state === 'rejected'
                      ? 'Check inputs and error details'
                      : 'Awaiting reconciliation'}
                  </td>
                </tr>
              ))}
              {orders.map((order) => (
                <tr key={bytesToHex(order.order_id)}>
                  <td data-label="Market">{order.market}</td>
                  <td data-label="Type">
                    {order.trigger[0]
                      ? `positionTpsl/${variantName(order.trigger[0].kind)}`
                      : order.kind}
                  </td>
                  <td data-label="State">
                    {variantName(order.state)}
                    <details className="order-processing-details">
                      <summary>Processing details</summary>
                      <span>Preflight: {variantName(order.preflight_state)}</span>
                      <span>Dispatch: {variantName(order.dispatch_state)}</span>
                    </details>
                  </td>
                  <td data-label="Filled / ordered quantity">
                    {order.filled_quantity} / {order.quantity}
                  </td>
                  <td data-label="Action">
                    <button
                      disabled={
                        busy || !['Open', 'PartiallyFilled'].includes(variantName(order.state))
                      }
                      onClick={() => void execute(() => gateway.cancel(order.order_id))}
                    >
                      Cancel
                    </button>
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
          {!data?.orders?.items.length && !optimistic.length && (
            <p className="table-empty">
              {data
                ? 'No orders yet. Start with the order ticket above.'
                : 'Connect to view your orders and fills.'}
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
      <td data-label="Market">
        <strong>{position.market}</strong>
        <span className={position.size.startsWith('-') ? 'negative' : 'positive'}>
          {' '}
          · {position.size.startsWith('-') ? 'Short' : 'Long'}
        </span>
      </td>
      <td data-label="Quantity">{position.size}</td>
      <td data-label="Entry">{position.entry_price}</td>
      <td data-label="PnL" className={position.unrealized_pnl >= 0n ? 'positive' : 'negative'}>
        {signedMicros(position.unrealized_pnl)}
      </td>
      <td data-label="Liquidation price">{position.liquidation_price[0] ?? '—'}</td>
      <td data-label="Leverage">
        {position.leverage}x {position.margin_mode}
      </td>
      <td data-label="Protection / close">
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
            Set SL/TP
          </button>
          {[2500, 5000, 10000].map((ratio) => (
            <button
              key={ratio}
              disabled={busy}
              onClick={() => void run(() => gateway.closePosition(position, ratio))}
            >
              Close {ratio / 100}%
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
        {data?.funds.recovery_fence.length ? (
          <section className="panel unknown-panel">
            <h2>
              {variantName(data.funds.recovery_fence[0]) === 'Preparing'
                ? 'Preparing recovery'
                : 'Reconciling recovery'}
            </h2>
            <p>
              New orders are paused for this account. Cancellation and reduce-only closes remain
              available.
            </p>
          </section>
        ) : null}
        {data?.funds.unknowns.length ||
        orders.some((order) => variantName(order.state) === 'Unknown') ? (
          <section className="panel unknown-panel">
            <h2>Unresolved outcomes</h2>
            <p>
              External results are being reconciled. Do not resubmit; use only cancellation or
              reduce-only actions.
            </p>
          </section>
        ) : null}
        <section className="panel table-panel">
          <h1>Funding history</h1>
          <table>
            <thead>
              <tr>
                <th>Time</th>
                <th>Action</th>
                <th>State</th>
                <th>Amount</th>
              </tr>
            </thead>
            <tbody>
              {funds.map((event) => (
                <tr key={bytesToHex(event.event_id)}>
                  <td>{new Date(Number(event.at)).toLocaleString('en-US')}</td>
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
              Load more fund events
            </button>
          )}
        </section>
        <section className="panel table-panel">
          <h2>Order history</h2>
          <table>
            <thead>
              <tr>
                <th>Updated</th>
                <th>Market</th>
                <th>Type</th>
                <th>State</th>
                <th>Quantity</th>
                <th>Reference ID</th>
              </tr>
            </thead>
            <tbody>
              {orders.map((order) => (
                <tr key={bytesToHex(order.order_id)}>
                  <td>{new Date(Number(order.updated_at)).toLocaleString('en-US')}</td>
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
              Load more orders
            </button>
          )}
        </section>
        <section className="panel table-panel">
          <h2>Fills</h2>
          <table>
            <thead>
              <tr>
                <th>Time</th>
                <th>Market</th>
                <th>Price</th>
                <th>Quantity</th>
                <th>Fee</th>
              </tr>
            </thead>
            <tbody>
              {fills.map((fill, index) => (
                <tr key={`${fill.at}-${index}`}>
                  <td>{new Date(Number(fill.at)).toLocaleString('en-US')}</td>
                  <td>{fill.market}</td>
                  <td>{fill.price}</td>
                  <td>{fill.quantity}</td>
                  <td>{signedMicros(fill.fee)}</td>
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
              Load more fills
            </button>
          )}
        </section>
      </div>
    </Workspace>
  )
}
