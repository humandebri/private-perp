import { createHash } from 'node:crypto'
import { execFileSync } from 'node:child_process'
import { fileURLToPath } from 'node:url'
import { createServer } from 'node:http'
import { createRequire } from 'node:module'

// mockはfrontendの開発依存として管理し、リポジトリ直下に別のJS workspaceを増やさない。
const require = createRequire(new URL('../../frontend/package.json', import.meta.url))
const { WebSocketServer } = require('ws')

const host = process.env.MOCK_HL_HOST ?? '127.0.0.1'
const port = Number(process.env.MOCK_HL_PORT ?? '8080')
const adminOrigins = new Set(
  (process.env.MOCK_HL_ADMIN_ORIGINS ??
    'http://127.0.0.1:4173,http://127.0.0.1:5173')
    .split(',')
    .map((value) => value.trim())
    .filter(Boolean),
)
if (!['127.0.0.1', '::1', 'localhost'].includes(host)) {
  throw new Error('mock-hl refuses non-loopback hosts')
}

const marketState = {
  BTC: { mid: 60000, decimals: 5 },
  ETH: { mid: 3000, decimals: 5 },
}
const state = {
  nextOid: 1,
  nextTid: 1,
  deposits: new Map(),
  balances: new Map(),
  orders: new Map(),
  fills: [],
  positions: new Map(),
  scenario: { nextOrder: 'normal', infoUnavailable: false },
}
class HttpError extends Error {
  constructor(status, message) {
    super(message)
    this.status = status
  }
}
export const isLoopbackAddress = (address) =>
  address === '127.0.0.1' || address === '::1' || address === '::ffff:127.0.0.1'
export const validateAdminAccess = (remoteAddress, origin, allowedOrigins = adminOrigins) => {
  if (!isLoopbackAddress(remoteAddress)) throw new HttpError(403, 'admin requires loopback')
  if (origin && origin !== 'null' && allowedOrigins.has(origin)) return origin
  if (origin === undefined) return undefined
  throw new HttpError(403, 'admin origin is not allowed')
}
const json = (response, status, value, allowedOrigin) => {
  const headers = {
    'content-type': 'application/json',
    'access-control-allow-methods': 'POST, OPTIONS',
    'access-control-allow-headers': 'content-type',
    'cache-control': 'no-store',
  }
  if (allowedOrigin) {
    headers['access-control-allow-origin'] = allowedOrigin
    headers.vary = 'Origin'
  }
  response.writeHead(status, headers)
  response.end(status === 204 ? undefined : JSON.stringify(value))
}
const readJson = async (request) => {
  const chunks = []
  for await (const chunk of request) chunks.push(chunk)
  return JSON.parse(Buffer.concat(chunks).toString('utf8'))
}
const hash = (value) => `0x${createHash('sha256').update(value).digest('hex')}`
const normalizeAddress = (value) => {
  const address = String(value ?? '').toLowerCase()
  if (!/^0x[0-9a-f]{40}$/.test(address)) throw new Error('invalid address')
  return address
}
export const reset = () => {
  state.nextOid = 1
  state.nextTid = 1
  state.deposits.clear()
  state.balances.clear()
  state.orders.clear()
  state.fills.length = 0
  state.positions.clear()
  state.scenario = { nextOrder: 'normal', infoUnavailable: false }
}

const marketForAsset = (asset) => (Number(asset) === 2 ? 'BTC' : Number(asset) === 1 ? 'ETH' : 'UNKNOWN')
const positionRows = () =>
  [...state.positions.entries()]
    .filter(([, position]) => Math.abs(position.size) > 1e-12)
    .map(([coin, position]) => ({
      position: {
        coin,
        szi: String(position.size),
        entryPx: String(position.entry),
        liquidationPx: String(position.entry * (position.size > 0 ? 0.8 : 1.2)),
        unrealizedPnl: '0',
        leverage: { type: 'cross', value: 3 },
        marginMode: 'cross',
      },
    }))

const applyFill = (market, order, size) => {
  const signed = (order.b ? 1 : -1) * Number(size)
  const current = state.positions.get(market) ?? { size: 0, entry: Number(order.p) }
  let nextSize = current.size + signed
  if (order.r) {
    if (current.size === 0 || Math.sign(current.size) === Math.sign(signed)) nextSize = current.size
    else if (Math.abs(signed) >= Math.abs(current.size)) nextSize = 0
  }
  nextSize = Number(nextSize.toFixed(8))
  if (nextSize === 0) state.positions.delete(market)
  else state.positions.set(market, { size: nextSize, entry: current.size === 0 ? Number(order.p) : current.entry })
}

export function exchange(body) {
  const action = body.action ?? {}
  if (['updateLeverage', 'approveAgent'].includes(action.type)) {
    return { status: 'ok', response: { type: 'default' } }
  }
  if (action.type === 'usdSend') {
    const destination = normalizeAddress(action.destination)
    const sender = normalizeAddress(execFileSync(fileURLToPath(new URL('../../target/debug/recover-usd-send', import.meta.url)), [], { input: JSON.stringify(body), encoding: 'utf8', timeout: 5000 }).trim())
    const key = hash(`usdSend:${body.nonce}:${destination}:${action.amount}`)
    const entries = state.deposits.get(destination) ?? []
    if (!entries.some((entry) => entry.hash === key)) {
      entries.push({
        hash: key,
        time: Number(action.time),
        delta: { type: 'internalTransfer', user: sender, destination, usdc: String(action.amount), fee: '0' },
      })
      state.deposits.set(destination, entries)
      const outgoing = state.deposits.get(sender) ?? []
      outgoing.push(entries.at(-1))
      state.deposits.set(sender, outgoing)
      state.balances.set(sender, (state.balances.get(sender) ?? 0) - Number(action.amount))
      state.balances.set(destination, (state.balances.get(destination) ?? 0) + Number(action.amount))
    }
    return { status: 'ok', response: { type: 'default' } }
  }
  if (action.type === 'order') {
    const order = action.orders?.[0] ?? {}
    const oid = state.nextOid++
    const market = marketForAsset(order.a)
    const isMarket = order.t?.limit?.tif === 'Ioc'
    const scenario = state.scenario.nextOrder
    state.scenario.nextOrder = 'normal'
    if (scenario === 'reject') return { status: 'err', response: 'LOCAL MOCK rejected next order' }
    if (scenario === 'unknown') throw new HttpError(503, 'LOCAL MOCK unknown next order result')
    const partial = isMarket && scenario === 'partial'
    const status = isMarket && !partial ? 'filled' : 'open'
    const record = { oid, status, market, order }
    state.orders.set(oid, record)
    if (isMarket) {
      const filledSize = partial ? String(Number(order.s ?? 0) / 2) : String(order.s ?? '0')
      applyFill(market, order, filledSize)
      state.fills.push({
        tid: state.nextTid++, oid, coin: market, px: String(order.p ?? '0'),
        sz: filledSize, fee: "0", time: Number(body.nonce ?? Date.now()),
      })
    }
    return {
      status: 'ok', response: { type: 'order', data: { statuses: [
        isMarket && !partial
          ? { filled: { oid, totalSz: String(order.s ?? '0'), avgPx: String(order.p ?? '0') } }
          : { resting: { oid } },
      ] } },
    }
  }
  if (action.type === 'cancel' || action.type === 'cancelByCloid') {
    for (const cancellation of action.cancels ?? action.cancelsByCloid ?? []) {
      const oid = Number(cancellation.o)
      if (state.orders.has(oid)) state.orders.get(oid).status = 'canceled'
    }
    return { status: 'ok', response: { type: 'cancel', data: { statuses: ['success'] } } }
  }
  return { status: 'err', response: `unsupported action: ${String(action.type)}` }
}

export function info(body) {
  if (state.scenario.infoUnavailable) throw new HttpError(503, 'LOCAL MOCK info unavailable')
  if (body.type === 'meta') return { universe: [
    { name: 'SOL', szDecimals: 0, maxLeverage: 10 },
    { name: 'ETH', szDecimals: 5, maxLeverage: 50 },
    { name: 'BTC', szDecimals: 5, maxLeverage: 50 },
  ] }
  if (body.type === 'metaAndAssetCtxs') return [
    { universe: [
      { name: 'SOL', szDecimals: 0, maxLeverage: 10 },
      { name: 'ETH', szDecimals: 5, maxLeverage: 50 },
      { name: 'BTC', szDecimals: 5, maxLeverage: 50 },
    ] },
    [
      { dayNtlVlm: '10000000' },
      { dayNtlVlm: '100000000' },
      { dayNtlVlm: '500000000' },
    ],
  ]
  if (body.type === 'l2Book' && marketState[body.coin]) {
    const mid = marketState[body.coin].mid
    return { coin: body.coin, time: Date.now(), levels: [
      [{ px: String(mid - 1), sz: '1.25', n: 2 }],
      [{ px: String(mid + 1), sz: '1.10', n: 2 }],
    ] }
  }
  if (body.type === 'candleSnapshot' && marketState[body.req?.coin] && body.req.interval === '1m') {
    const { coin, startTime, endTime } = body.req
    const mid = marketState[coin].mid
    const candles = []
    for (let t = Math.floor(startTime / 60_000) * 60_000; t <= endTime; t += 60_000)
      candles.push({ s: coin, i: '1m', t, T: t + 59_999, o: String(mid - 10), h: String(mid + 20), l: String(mid - 20), c: String(mid), v: '12.5', n: 10 })
    return candles.slice(-5000)
  }
  if (body.type === 'userNonFundingLedgerUpdates') {
    return (state.deposits.get(normalizeAddress(body.user)) ?? [])
      .filter((entry) => entry.time >= Number(body.startTime ?? 0) && entry.time <= Number(body.endTime ?? Number.MAX_SAFE_INTEGER))
      .sort((a, b) => a.time - b.time).slice(0, 500)
  }
  if (body.type === 'openOrders') {
    return [...state.orders.values()].filter((order) => order.status === 'open').map(({ oid }) => ({ oid }))
  }
  if (body.type === 'userFills') return state.fills
  if (body.type === 'userFillsByTime') return state.fills.filter(fill => fill.time >= body.startTime && fill.time <= body.endTime).sort((a, b) => a.time - b.time).slice(0, 2000)
  if (body.type === 'clearinghouseState') {
    const margin = [...state.positions.entries()].reduce(
      (total, [coin, position]) => total + Math.abs(position.size) * marketState[coin].mid / 3,
      0,
    )
    return {
      time: Date.now(),
      marginSummary: { accountValue: String(state.balances.get(normalizeAddress(body.user ?? "0x0000000000000000000000000000000000000000")) ?? 0), totalMarginUsed: margin.toFixed(6), totalNtlPos: (margin * 3).toFixed(6) },
      assetPositions: positionRows(),
    }
  }
  if (body.type === 'orderStatus') {
    const order = typeof body.oid === 'string' && body.oid.startsWith('0x')
      ? [...state.orders.values()].find((v) => v.order.c === body.oid)
      : state.orders.get(Number(body.oid))
    return order ? { status: 'order', order: { status: order.status, order: { oid: order.oid, cloid: order.order.c } } } : { status: 'unknownOid' }
  }
  return { error: `unsupported info query: ${String(body.type)}` }
}

export function seedDeposit(body) {
  const address = normalizeAddress(body.address)
  if (!/^\d+(\.\d{1,6})?$/.test(String(body.amount)) || Number(body.amount) <= 0) {
    throw new Error('invalid amount')
  }
  const id = String(body.id ?? `${address}:${body.amount}`)
  const entries = state.deposits.get(address) ?? []
  const event = {
    hash: hash(`deposit:${id}`),
    time: Number(body.time ?? Date.now()),
    delta: body.sender
      ? { type: 'internalTransfer', user: normalizeAddress(body.sender), destination: address, usdc: String(body.amount), fee: '0' }
      : { type: 'deposit', usdc: String(body.amount) },
  }
  if (!entries.some((entry) => entry.hash === event.hash)) {
    entries.push(event)
    state.balances.set(address, (state.balances.get(address) ?? 0) + Number(body.amount))
  }
  state.deposits.set(address, entries)
  return { ok: true, event, mode: 'LOCAL MOCK' }
}

export function setScenario(body) {
  const nextOrder = body.nextOrder ?? state.scenario.nextOrder
  if (!['normal', 'partial', 'reject', 'unknown'].includes(nextOrder)) {
    throw new Error('invalid nextOrder scenario')
  }
  state.scenario = { nextOrder, infoUnavailable: Boolean(body.infoUnavailable) }
  return { ok: true, mode: 'LOCAL MOCK', scenario: state.scenario }
}

const marketMessage = (subscription) => {
  const coin = subscription.coin && marketState[subscription.coin] ? subscription.coin : 'BTC'
  const { mid } = marketState[coin]
  const now = Date.now()
  if (subscription.type === 'allMids')
    return { channel: 'allMids', data: { mids: { BTC: '60000', ETH: '3000' } } }
  if (subscription.type === 'l2Book')
    return { channel: 'l2Book', data: { coin, isSnapshot: true, levels: [
      [{ px: String(mid - 1), sz: '1.25', n: 2 }],
      [{ px: String(mid + 1), sz: '1.10', n: 2 }],
    ] } }
  if (subscription.type === 'trades')
    return { channel: 'trades', data: [{ coin, px: String(mid), sz: '0.01', side: 'B', time: now, tid: now }] }
  if (subscription.type === 'candle')
    return { channel: 'candle', data: { s: coin, i: subscription.interval ?? '1m', t: Math.floor(now / 60_000) * 60_000, T: Math.floor(now / 60_000) * 60_000 + 59_999, o: String(mid - 10), h: String(mid + 20), l: String(mid - 20), c: String(mid), v: '12.5', n: 10 } }
  if (subscription.type === 'bbo')
    return { channel: 'bbo', data: { coin, time: now, bbo: [{ px: String(mid - 1), sz: '1.25' }, { px: String(mid + 1), sz: '1.10' }] } }
  if (subscription.type === 'activeAssetCtx')
    return { channel: 'activeAssetCtx', data: { coin, ctx: { markPx: String(mid), oraclePx: String(mid), funding: '0.00001', openInterest: '100' } } }
  return null
}

const sockets = new WebSocketServer({ noServer: true })
sockets.on('connection', (socket) => {
  const subscriptions = new Map()
  const updates = setInterval(() => {
    if (socket.readyState !== 1) return
    for (const subscription of subscriptions.values()) {
      if (!['allMids', 'candle'].includes(subscription.type)) continue
      const payload = marketMessage(subscription)
      if (payload) socket.send(JSON.stringify(payload))
    }
  }, 1000)
  socket.on('close', () => clearInterval(updates))
  socket.on('message', (raw) => {
    let message
    try { message = JSON.parse(raw.toString()) } catch { return }
    if (message.method === 'ping') return socket.send(JSON.stringify({ channel: 'pong' }))
    if (message.method === 'subscribe') {
      subscriptions.set(JSON.stringify(message.subscription), message.subscription ?? {})
      const payload = marketMessage(message.subscription ?? {})
      if (payload) socket.send(JSON.stringify(payload))
      socket.send(JSON.stringify({ channel: 'subscriptionResponse', data: message }))
    }
    if (message.method === 'unsubscribe') subscriptions.delete(JSON.stringify(message.subscription))
  })
})

export const server = createServer(async (request, response) => {
  let allowedOrigin
  try {
    const isAdmin = ['/admin/reset', '/admin/deposits', '/admin/scenarios'].includes(request.url)
    allowedOrigin = isAdmin || request.url === '/info'
      ? validateAdminAccess(request.socket.remoteAddress, request.headers.origin)
      : undefined
    if (request.method === 'OPTIONS') return json(response, 204, null, allowedOrigin)
    if (request.method !== 'POST')
      return json(response, 405, { error: 'POST required' }, allowedOrigin)
    const body = await readJson(request)
    if (request.url === '/exchange') return json(response, 200, exchange(body))
    if (request.url === '/info') return json(response, 200, info(body), allowedOrigin)
    if (request.url === '/admin/reset') {
      reset()
      return json(response, 200, { ok: true, mode: 'LOCAL MOCK' }, allowedOrigin)
    }
    if (request.url === '/admin/deposits') {
      const seeded = seedDeposit(body)
      return json(response, 200, seeded, allowedOrigin)
    }
    if (request.url === '/admin/scenarios')
      return json(response, 200, setScenario(body), allowedOrigin)
    return json(response, 404, { error: 'not found' })
  } catch (error) {
    const status = error instanceof HttpError ? error.status : 400
    return json(
      response,
      status,
      { error: error instanceof Error ? error.message : String(error) },
      allowedOrigin,
    )
  }
})

server.on('upgrade', (request, socket, head) => {
  if (request.url !== '/ws' || !isLoopbackAddress(request.socket.remoteAddress)) {
    socket.destroy()
    return
  }
  sockets.handleUpgrade(request, socket, head, (client) => sockets.emit('connection', client, request))
})

if (process.argv[1]?.endsWith('/tools/mock-hl/server.mjs') && process.env.MOCK_HL_IMPORT_ONLY !== '1') {
  server.listen(port, host, () => console.log(`LOCAL MOCK HL listening on http://${host}:${port}`))
}
