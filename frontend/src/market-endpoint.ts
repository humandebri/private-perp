/** Public read-only info API on the same venue as the configured WebSocket. */
export function marketInfoUrl(marketWs: string): string {
  const url = new URL(marketWs)
  if (!['ws:', 'wss:'].includes(url.protocol)) throw new Error('Invalid market WebSocket URL')
  url.protocol = url.protocol === 'wss:' ? 'https:' : 'http:'
  url.pathname = '/info'
  url.search = ''
  url.hash = ''
  return url.toString()
}
