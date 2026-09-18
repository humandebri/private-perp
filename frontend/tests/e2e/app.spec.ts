import { expect, test } from '@playwright/test'

test('SSR serves only a public shell for account routes and refuses POST', async ({ request }) => {
  for (const path of ['/', '/trade', '/funds', '/history']) {
    const response = await request.get(path)
    expect(response.ok()).toBeTruthy()
    expect(response.headers()['x-frame-options']).toBe('DENY')
    const html = await response.text()
    expect(html).not.toContain('DEMO-001')
    expect(html).not.toContain('5,000.00')
  }
  expect((await request.post('/trade', { data: 'not-a-real-order' })).status()).toBe(405)
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

test('unknown cannot be resent and stale data blocks new orders', async ({ page }) => {
  await page.goto('/trade')
  await page.getByRole('button', { name: 'デモセッション開始' }).click()
  await page.getByRole('checkbox', { name: '口座データ遅延' }).check()
  await expect(page.getByRole('button', { name: '口座データ遅延 · 注文停止' })).toBeDisabled()
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
  await expect(page.getByText('4,900.00', { exact: false }).first()).toBeVisible()
  await page.getByRole('link', { name: '履歴', exact: true }).click()
  await expect(page.getByRole('cell', { name: 'allocate', exact: true })).toBeVisible()
  expect(
    await page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth),
  ).toBeTruthy()
})
