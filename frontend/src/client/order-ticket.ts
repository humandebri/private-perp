const SCALE = 1_000_000n

function decimal(value: string): bigint {
  if (!/^\d+(\.\d{1,6})?$/.test(value))
    throw new Error('Use a positive amount with up to 6 decimals.')
  const [whole, fraction = ''] = value.split('.')
  const parsed = BigInt(whole) * SCALE + BigInt(fraction.padEnd(6, '0'))
  if (parsed <= 0n) throw new Error('Enter an amount greater than zero.')
  return parsed
}
export function decimalText(value: bigint, places = 6): string {
  const scale = 10n ** BigInt(places)
  const fraction = (value % scale).toString().padStart(places, '0').replace(/0+$/, '')
  return `${value / scale}${fraction ? '.' + fraction : ''}`
}
function significantStep(value: bigint, scale: number): bigint {
  return 10n ** BigInt(Math.max(0, value.toString().length - 5, scale))
}
export function quoteOrder(args: {
  amount: string
  unit: 'margin' | 'coin'
  reference: string
  limit: string
  kind: 'market' | 'limit'
  side: 'buy' | 'sell'
  leverage: number
  slippageBps: number
  sizeDecimals: number
}): { quantity: string; price: string; notional: bigint; margin: bigint } {
  if (!Number.isInteger(args.leverage) || args.leverage < 1 || args.leverage > 5)
    throw new Error('Choose leverage from 1x to 5x.')
  if (!Number.isInteger(args.sizeDecimals) || args.sizeDecimals < 0 || args.sizeDecimals > 6)
    throw new Error('Waiting for market precision.')
  if (
    args.kind === 'market' &&
    (!Number.isInteger(args.slippageBps) || args.slippageBps < 1 || args.slippageBps > 1000)
  )
    throw new Error('Slippage must be between 1 and 1000 bps.')
  const reference = decimal(args.kind === 'limit' ? args.limit : args.reference)
  let price = reference
  if (args.kind === 'market') {
    price =
      (reference * BigInt(10_000 + (args.side === 'buy' ? args.slippageBps : -args.slippageBps))) /
      10_000n
    const step = significantStep(price, args.sizeDecimals)
    // Round inside the user's bound: never widen their maximum slippage.
    price = args.side === 'buy' ? (price / step) * step : ((price + step - 1n) / step) * step
  } else if (price % significantStep(price, args.sizeDecimals) !== 0n) {
    throw new Error('Limit price exceeds this market’s precision (up to 5 significant digits).')
  }
  const input = decimal(args.amount)
  if (price <= 0n) throw new Error('Price is below this market’s precision.')
  const scale = 10n ** BigInt(args.sizeDecimals)
  const marketPrice = decimal(args.reference)
  const budgetPrice = price > marketPrice ? price : marketPrice
  let quantity =
    args.unit === 'margin'
      ? (input * BigInt(args.leverage) * scale) / budgetPrice
      : (input * scale) / SCALE
  if (args.unit === 'coin' && (input * scale) % SCALE !== 0n)
    throw new Error(`Quantity allows up to ${args.sizeDecimals} decimals.`)
  const step = significantStep(quantity, 0)
  if (args.unit === 'coin' && quantity % step !== 0n)
    throw new Error('Quantity allows up to 5 significant digits.')
  quantity = (quantity / step) * step
  if (quantity <= 0n) throw new Error('Amount is below the market’s minimum quantity.')
  const notional = (quantity * budgetPrice + scale - 1n) / scale
  return {
    quantity: decimalText(quantity, args.sizeDecimals),
    price: decimalText(price),
    notional,
    margin: (notional + BigInt(args.leverage) - 1n) / BigInt(args.leverage),
  }
}

export function availableMargin(equity: bigint, used: bigint, reserved: bigint): bigint {
  const free = equity - used - reserved
  return free > 0n ? free : 0n
}
