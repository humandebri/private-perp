import { createHash } from 'node:crypto'
import { createServer } from 'node:http'

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

const state = { nextOid: 1, nextTid: 1, deposits: new Map(), orders: new Map(), fills: [] }
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
  state.orders.clear()
  state.fills.length = 0
}

export function exchange(body) {
  const action = body.action ?? {}
  if (['updateLeverage', 'approveAgent'].includes(action.type)) {
    return { status: 'ok', response: { type: 'default' } }
  }
  if (action.type === 'usdSend') {
    const destination = normalizeAddress(action.destination)
    const key = hash(`usdSend:${body.nonce}:${destination}:${action.amount}`)
    const entries = state.deposits.get(destination) ?? []
    if (!entries.some((entry) => entry.hash === key)) {
      entries.push({
        hash: key,
        time: Number(action.time),
        delta: { type: 'deposit', usdc: String(action.amount) },
      })
      state.deposits.set(destination, entries)
    }
    return { status: 'ok', response: { type: 'default' } }
  }
  if (action.type === 'order') {
    const order = action.orders?.[0] ?? {}
    const oid = state.nextOid++
    const market = Number(order.a) === 2 ? 'BTC' : Number(order.a) === 1 ? 'ETH' : 'UNKNOWN'
    const isMarket = order.t?.limit?.tif === 'Ioc'
    const record = { oid, status: isMarket ? 'filled' : 'open', market, order }
    state.orders.set(oid, record)
    if (isMarket) {
      state.fills.push({
        tid: state.nextTid++, oid, coin: market, px: String(order.p ?? '0'),
        sz: String(order.s ?? '0'), fee: 0, time: Number(body.nonce ?? Date.now()),
      })
    }
    return {
      status: 'ok', response: { type: 'order', data: { statuses: [
        isMarket ? { filled: { oid, totalSz: String(order.s ?? '0'), avgPx: String(order.p ?? '0') } } : { resting: { oid } },
      ] } },
    }
  }
  if (action.type === 'cancel' || action.type === 'cancelByCloid') {
    for (const cancellation of action.cancels ?? action.cancelsByCloid ?? []) {
      const oid = Number(cancellation.o)
      if (state.orders.has(oid)) state.orders.get(oid).status = 'canceled'
    }
    return { status: 'ok', response: { type: 'cancel', data: { statuses: [{ success: true }] } } }
  }
  return { status: 'err', response: `unsupported action: ${String(action.type)}` }
}

export function info(body) {
  if (body.type === 'userNonFundingLedgerUpdates') {
    return state.deposits.get(normalizeAddress(body.user)) ?? []
  }
  if (body.type === 'userFills') return state.fills
  if (body.type === 'clearinghouseState') return { assetPositions: [] }
  if (body.type === 'orderStatus') {
    const order = state.orders.get(Number(body.oid))
    return { status: order?.status ?? 'unknown', order: { oid: Number(body.oid) } }
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
    delta: { type: 'deposit', usdc: String(body.amount) },
  }
  if (!entries.some((entry) => entry.hash === event.hash)) entries.push(event)
  state.deposits.set(address, entries)
  return { ok: true, event, mode: 'LOCAL MOCK' }
}

export const server = createServer(async (request, response) => {
  let allowedOrigin
  try {
    const isAdmin = request.url === '/admin/reset' || request.url === '/admin/deposits'
    allowedOrigin = isAdmin
      ? validateAdminAccess(request.socket.remoteAddress, request.headers.origin)
      : undefined
    if (request.method === 'OPTIONS') return json(response, 204, null, allowedOrigin)
    if (request.method !== 'POST')
      return json(response, 405, { error: 'POST required' }, allowedOrigin)
    const body = await readJson(request)
    if (request.url === '/exchange') return json(response, 200, exchange(body))
    if (request.url === '/info') return json(response, 200, info(body))
    if (request.url === '/admin/reset') {
      reset()
      return json(response, 200, { ok: true, mode: 'LOCAL MOCK' }, allowedOrigin)
    }
    if (request.url === '/admin/deposits') {
      const seeded = seedDeposit(body)
      return json(response, 200, seeded, allowedOrigin)
    }
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

if (process.argv[1]?.endsWith('/tools/mock-hl/server.mjs') && process.env.MOCK_HL_IMPORT_ONLY !== '1') {
  server.listen(port, host, () => console.log(`LOCAL MOCK HL listening on http://${host}:${port}`))
}
