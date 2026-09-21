import { expect, test } from '@playwright/test'

test('SSR serves only the public shell for account routes and refuses POST', async ({
  request,
}) => {
  for (const path of ['/', '/trade', '/funds', '/history']) {
    const response = await request.get(path)
    expect(response.ok()).toBeTruthy()
    expect(response.headers()['x-frame-options']).toBe('DENY')
    expect(response.headers()['cache-control']).toBe('no-store')
    const html = await response.text()
    // Positive control: the public shell really is server-rendered.
    expect(html).toContain('VEIL')
    // The account workspace and its balances are client-only. If a change ever
    // let the server render them, these markers would appear in the HTML.
    expect(html).not.toContain('data-testid="account-panel"')
    expect(html).not.toContain('data-testid="account-balances"')
    expect(html).not.toContain('デモセッション開始')
  }
  expect((await request.post('/trade', { data: 'not-a-real-order' })).status()).toBe(405)
})

test('hashed assets are immutable while HTML is never cached', async ({ request }) => {
  const page = await request.get('/trade')
  expect(page.headers()['cache-control']).toBe('no-store')
  const asset = (await page.text()).match(/\/assets\/[^"']+\.(?:js|css)/)?.[0]
  expect(asset).toBeTruthy()
  const assetResponse = await request.get(asset as string)
  expect(assetResponse.ok()).toBeTruthy()
  expect(assetResponse.headers()['cache-control']).toBe('public, max-age=31536000, immutable')
  expect((await request.get('/missing-page')).headers()['cache-control']).toBe('no-store')
})

test('partial fill, cancellation and logout clear the session without network writes', async ({
  page,
}) => {
  const unsafe: string[] = []
  page.on('request', (request) => {
    if (
      !['GET', 'HEAD'].includes(request.method()) ||
      !request.url().startsWith('http://127.0.0.1:4173')
    )
      unsafe.push(`${request.method()} ${request.url()}`)
  })
  await page.goto('/trade')
  await page.getByRole('button', { name: 'デモセッション開始' }).click()
  await page.getByLabel('応答シナリオ').selectOption('partial')
  await page.getByRole('button', { name: 'デモ注文を送信' }).click()
  await expect(page.getByRole('cell', { name: '部分約定（模擬）', exact: true })).toBeVisible()
  await page.getByRole('button', { name: '取消', exact: true }).click()
  await expect(page.getByRole('cell', { name: '取消済み（模擬）', exact: true })).toBeVisible()
  await expect(page.getByRole('cell', { name: '0.005', exact: true })).toBeVisible()
  await page.getByRole('button', { name: 'デモ終了' }).click()
  await expect(page.getByText('注文はまだありません')).toBeVisible()
  expect(unsafe).toEqual([])
  expect(
    await page.evaluate(() => ({ local: localStorage.length, session: sessionStorage.length })),
  ).toEqual({ local: 0, session: 0 })
})

test('cancelling an unsent order never turns into a fill', async ({ page }) => {
  await page.goto('/trade')
  await page.getByRole('button', { name: 'デモセッション開始' }).click()
  await page.getByLabel('応答シナリオ').selectOption('cancel-race')
  // Click the row action in the same frame it appears, so the request is still
  // queued: that is the state where a cancellation race must not fabricate a fill.
  await page.evaluate(() => {
    const clickCancel = () => {
      const button = [...document.querySelectorAll('button')].find(
        (candidate) => candidate.textContent?.trim() === '取消',
      )
      if (button) button.click()
      else requestAnimationFrame(clickCancel)
    }
    requestAnimationFrame(clickCancel)
  })
  await page.getByRole('button', { name: 'デモ注文を送信' }).click()
  await expect(page.getByRole('cell', { name: '取消済み（模擬）', exact: true })).toBeVisible()
  // The in-flight settlement callback must not resurrect the cancelled order.
  await page.waitForTimeout(1000)
  await expect(page.getByRole('cell', { name: '取消済み（模擬）', exact: true })).toBeVisible()
  await expect(page.getByRole('cell', { name: '約定（模擬）', exact: true })).toHaveCount(0)
})

test('unknown cannot be resent and stale data blocks new orders', async ({ page }) => {
  await page.goto('/trade')
  await page.getByRole('button', { name: 'デモセッション開始' }).click()
  await page.getByRole('checkbox', { name: '口座データ遅延' }).check()
  await expect(page.getByRole('button', { name: '口座データ遅延 · 注文停止' })).toBeDisabled()
  await expect(page.getByText('10秒以上前 · 新規注文停止')).toBeVisible()
  await page.getByRole('checkbox', { name: '口座データ遅延' }).uncheck()
  await page.getByLabel('応答シナリオ').selectOption('unknown')
  await page.getByRole('button', { name: 'デモ注文を送信' }).click()
  await expect(page.getByRole('cell', { name: '結果不明・再送しない' })).toBeVisible()
  await expect(page.getByRole('button', { name: 'この要求は受付済みです' })).toBeDisabled()
  await expect(page.getByRole('button', { name: '新しい注文を入力' })).toHaveCount(0)
  await page.reload()
  await expect(page.getByRole('button', { name: 'デモセッション開始' })).toBeVisible()
})

test('fund confirmation is explicit and mobile routes remain usable', async ({ page }) => {
  await page.setViewportSize({ width: 390, height: 844 })
  await page.goto('/funds')
  await page.getByRole('button', { name: 'デモセッション開始' }).click()
  await page.getByLabel('資金移動額').fill('100')
  await page.getByRole('button', { name: '内容を確認' }).click()
  await expect(page.getByRole('dialog')).toBeVisible()
  await page.getByRole('button', { name: '模擬実行', exact: true }).click()
  await expect(page.getByText('4,900.000000', { exact: false }).first()).toBeVisible()
  await page.getByRole('link', { name: '履歴', exact: true }).click()
  await expect(page.getByRole('cell', { name: 'allocate', exact: true })).toBeVisible()
  expect(
    await page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth),
  ).toBeTruthy()
})

test('sub-unit amounts stay visible at 1e-6 precision', async ({ page }) => {
  await page.goto('/funds')
  await page.getByRole('button', { name: 'デモセッション開始' }).click()
  await page.getByLabel('操作').selectOption('deposit')
  await page.getByLabel('資金移動額').fill('0.000001')
  await page.getByRole('button', { name: '内容を確認' }).click()
  await expect(page.getByRole('dialog')).toContainText('0.000001 USDC')
  await page.getByRole('button', { name: '模擬実行', exact: true }).click()
  await expect(page.getByText('5,000.000001', { exact: false }).first()).toBeVisible()
})
