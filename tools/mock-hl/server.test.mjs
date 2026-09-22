import assert from 'node:assert/strict'
import { test } from 'node:test'
import { createRequire } from 'node:module'

process.env.MOCK_HL_IMPORT_ONLY = '1'
const require = createRequire(new URL('../../frontend/package.json', import.meta.url))
const WebSocket = require('ws')
const { exchange, info, isLoopbackAddress, reset, seedDeposit, server, setScenario, validateAdminAccess } =
  await import('./server.mjs')

test('deposit seed is idempotent and queryable', () => {
  const address = `0x${'11'.repeat(20)}`
  reset()
  seedDeposit({ address, amount: '12.000001', id: 'same' })
  seedDeposit({ address, amount: '12.000001', id: 'same' })
  const entries = info({ type: 'userNonFundingLedgerUpdates', user: address })
  assert.equal(entries.length, 1)
  assert.equal(entries[0].delta.usdc, '12.000001')
})

test('market fills, limit rests, and cancellation succeeds', () => {
  reset()
  const market = exchange({ nonce: 1, action: { type: 'order', orders: [{ a: 2, p: '60000', s: '0.01', t: { limit: { tif: 'Ioc' } } }] } })
  const limit = exchange({ nonce: 2, action: { type: 'order', orders: [{ a: 1, p: '3000', s: '0.1', t: { limit: { tif: 'Gtc' } } }] } })
  assert.ok(market.response.data.statuses[0].filled)
  const oid = limit.response.data.statuses[0].resting.oid
  assert.equal(info({ type: 'orderStatus', oid }).status, 'open')
  const cancelled = exchange({ action: { type: 'cancel', cancels: [{ o: oid }] } })
  assert.equal(cancelled.response.data.statuses[0].success, true)
  assert.equal(info({ type: 'orderStatus', oid }).status, 'canceled')
  assert.equal(info({ type: 'userFills' }).length, 1)
  assert.equal(info({ type: 'clearinghouseState' }).assetPositions[0].position.coin, 'BTC')
  assert.equal(info({ type: 'clearinghouseState' }).marginSummary.totalMarginUsed, '200.000000')
})

test('reduce-only closes positions and one-shot scenarios reset', () => {
  reset()
  exchange({ nonce: 1, action: { type: 'order', orders: [{ a: 2, b: true, p: '60000', s: '0.03', r: false, t: { limit: { tif: 'Ioc' } } }] } })
  exchange({ nonce: 2, action: { type: 'order', orders: [{ a: 2, b: false, p: '60000', s: '0.01', r: true, t: { limit: { tif: 'Ioc' } } }] } })
  assert.equal(info({ type: 'clearinghouseState' }).assetPositions[0].position.szi, '0.02')
  setScenario({ nextOrder: 'reject' })
  assert.equal(exchange({ action: { type: 'order', orders: [{}] } }).status, 'err')
  assert.equal(exchange({ action: { type: 'order', orders: [{ a: 2, p: '1', s: '1', t: { limit: { tif: 'Gtc' } } }] } }).status, 'ok')
  setScenario({ nextOrder: 'partial' })
  const partial = exchange({ action: { type: 'order', orders: [{ a: 1, b: true, p: '3000', s: '0.2', r: false, t: { limit: { tif: 'Ioc' } } }] } })
  assert.ok(partial.response.data.statuses[0].resting)
  assert.equal(info({ type: 'userFills' }).at(-1).sz, '0.1')
  setScenario({ nextOrder: 'unknown' })
  assert.throws(() => exchange({ action: { type: 'order', orders: [{}] } }), /unknown/)
  setScenario({ infoUnavailable: true })
  assert.throws(() => info({ type: 'clearinghouseState' }), /unavailable/)
})

test('usdSend appears as a destination ledger update', () => {
  const destination = `0x${'22'.repeat(20)}`
  reset()
  exchange({ nonce: 7, action: { type: 'usdSend', destination, amount: '5', time: 7 } })
  const entries = info({ type: 'userNonFundingLedgerUpdates', user: destination })
  assert.equal(entries.length, 1)
  assert.equal(entries[0].delta.usdc, '5')
})

test('admin access only accepts loopback and configured browser origins', () => {
  const origins = new Set(['http://127.0.0.1:4173'])
  assert.equal(isLoopbackAddress('::ffff:127.0.0.1'), true)
  assert.equal(
    validateAdminAccess('127.0.0.1', 'http://127.0.0.1:4173', origins),
    'http://127.0.0.1:4173',
  )
  assert.equal(validateAdminAccess('::1', undefined, origins), undefined)
  assert.throws(() => validateAdminAccess('10.0.0.2', undefined, origins), /loopback/)
  assert.throws(
    () => validateAdminAccess('127.0.0.1', 'https://attacker.example', origins),
    /origin/,
  )
  assert.throws(() => validateAdminAccess('127.0.0.1', 'null', origins), /origin/)
})

test('admin HTTP responses use exact-origin CORS and reject foreign origins', async () => {
  await new Promise((resolve) => server.listen(0, '127.0.0.1', resolve))
  try {
    const address = server.address()
    assert.equal(typeof address, 'object')
    const url = `http://127.0.0.1:${address.port}/admin/deposits`
    const allowed = await fetch(url, {
      method: 'OPTIONS',
      headers: { origin: 'http://127.0.0.1:4173' },
    })
    assert.equal(allowed.status, 204)
    assert.equal(allowed.headers.get('access-control-allow-origin'), 'http://127.0.0.1:4173')
    assert.equal(allowed.headers.get('vary'), 'Origin')

    const denied = await fetch(url, {
      method: 'OPTIONS',
      headers: { origin: 'https://attacker.example' },
    })
    assert.equal(denied.status, 403)
    assert.equal(denied.headers.get('access-control-allow-origin'), null)
  } finally {
    await new Promise((resolve, reject) =>
      server.close((error) => (error ? reject(error) : resolve())),
    )
  }
})

test('public websocket emits market-only snapshots and pong', async () => {
  await new Promise((resolve) => server.listen(0, '127.0.0.1', resolve))
  try {
    const address = server.address()
    const socket = new WebSocket(`ws://127.0.0.1:${address.port}/ws`)
    await new Promise((resolve) => socket.once('open', resolve))
    const message = new Promise((resolve) => socket.once('message', (data) => resolve(JSON.parse(data.toString()))))
    socket.send(JSON.stringify({ method: 'subscribe', subscription: { type: 'allMids' } }))
    const payload = await message
    assert.equal(payload.channel, 'allMids')
    assert.deepEqual(Object.keys(payload.data.mids), ['BTC', 'ETH'])
    assert.equal(JSON.stringify(payload).includes('user'), false)
    socket.close()
  } finally {
    await new Promise((resolve, reject) => server.close((error) => error ? reject(error) : resolve()))
  }
})
