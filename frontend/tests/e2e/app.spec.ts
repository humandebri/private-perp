import { execFileSync } from 'node:child_process'
import { resolve } from 'node:path'
import { readFileSync } from 'node:fs'
import { parseEnv } from 'node:util'
import { expect, test } from '@playwright/test'
import { createClients } from '../../src/client/ic'
import { resolveConfig } from '../../src/client/config'
import { hexToBytes } from '../../src/client/wallet'
import { unwrap } from '../../src/client/result'

test('SSR exposes a local-only shell and refuses writes', async ({ request }) => {
  for (const path of ['/', '/trade', '/funds', '/history', '/fallback']) {
    const response = await request.get(path)
    expect(response.ok()).toBeTruthy()
    expect(response.headers()['x-frame-options']).toBe('DENY')
    expect(response.headers()['cache-control']).toBe('no-store')
    expect(response.headers()['content-security-policy']).toContain('http://127.0.0.1:18100')
    const html = await response.text()
    expect(html).toContain('LOCAL MOCK')
    if (path !== '/trade') expect(html).not.toContain('data-testid="account-panel"')
  }
  expect((await request.post('/trade', { data: 'not-an-order' })).status()).toBe(405)
})

test('hashed assets remain immutable', async ({ request }) => {
  const page = await request.get('/trade')
  const asset = (await page.text()).match(/\/assets\/[^"']+\.(?:js|css)/)?.[0]
  expect(asset).toBeTruthy()
  const response = await request.get(asset as string)
  expect(response.headers()['cache-control']).toBe('public, max-age=31536000, immutable')
})

test('order ticket explains blocked actions and fits a mobile viewport', async ({ page }) => {
  await page.setViewportSize({ width: 390, height: 844 })
  await page.goto('/trade')
  await expect(page.getByRole('button', { name: '注文を受付' })).toBeDisabled()
  await expect(page.locator('#order-guidance')).toContainText('MetaMaskで接続')
  await page.getByLabel('銘柄').selectOption('ETH')
  await expect(page.getByLabel('注文価格')).toHaveValue('')
  await page.getByLabel('売買').selectOption('sell')
  await expect(page.getByText('売り価格の下限 (USDC)')).toBeVisible()
  await page.getByLabel('種別').selectOption('limit')
  await expect(page.getByLabel('スリッページ')).toBeDisabled()
  const ticket = await page.locator('#order-ticket').boundingBox()
  const chart = await page.locator('#market-chart').boundingBox()
  expect(ticket!.y).toBeLessThan(chart!.y)
  expect(await page.evaluate(() => document.documentElement.scrollWidth)).toBe(390)
})

test.describe('real local canister flow', () => {
  test.skip(process.env.LOCAL_E2E !== '1', 'scripts/local-e2e.sh owns the local replica')
  test('login, funds, agent, orders, recovery, withdrawal, history and logout', async ({
    page,
  }) => {
    test.setTimeout(240_000)
    const signer = resolve(process.cwd(), '../target/debug/e2e-signer')
    const address = execFileSync(signer, ['address'], { encoding: 'utf8' }).trim()
    const secondaryAddress = execFileSync(signer, ['address', '--secondary'], {
      encoding: 'utf8',
    }).trim()
    await page.exposeFunction('__e2eSignTypedData', (typedData: string) =>
      execFileSync(
        signer,
        JSON.parse(typedData).message.eoa.toLowerCase() === secondaryAddress.toLowerCase()
          ? ['--secondary']
          : [],
        { input: typedData, encoding: 'utf8' },
      ).trim(),
    )
    await page.addInitScript(
      ({ account }) => {
        Object.defineProperty(window, 'ethereum', {
          value: {
            request: async ({ method, params }: { method: string; params?: unknown[] }) => {
              if (method === 'eth_requestAccounts')
                return [
                  (globalThis as typeof globalThis & { __e2eAccount?: string }).__e2eAccount ??
                    account,
                ]
              if (method === 'eth_signTypedData_v4')
                return (
                  globalThis as typeof globalThis & {
                    __e2eSignTypedData(data: string): Promise<string>
                  }
                ).__e2eSignTypedData(String(params?.[1]))
              throw new Error(`unsupported provider method: ${method}`)
            },
          },
        })
      },
      { account: address },
    )

    await page.goto('/funds')
    await page.getByRole('button', { name: 'MetaMaskで接続' }).click()
    await expect(page.getByText(`${address.slice(0, 10)}…${address.slice(-6)}`)).toBeVisible()
    await page.getByLabel('金額').fill('100')
    const seedButton = page.getByRole('button', { name: 'LOCAL MOCK 入金seed' })
    await seedButton.click()
    await expect(seedButton).toBeEnabled({ timeout: 15_000 })
    await expect(page.getByRole('alert')).toHaveCount(0)
    await expect
      .poll(
        async () => {
          await page.getByRole('button', { name: '再読込' }).click()
          return page.getByTestId('account-balances').textContent()
        },
        { timeout: 30_000 },
      )
      .toContain('100.000000')

    await page.getByLabel('金額').fill('20')
    await page.getByRole('button', { name: '取引口座へ配分' }).click()
    await expect
      .poll(
        async () => {
          await page.getByRole('button', { name: '再読込' }).click()
          return page.getByTestId('account-balances').textContent()
        },
        { timeout: 30_000 },
      )
      .toContain('20.000000')
    await page.getByRole('link', { name: '取引' }).click()
    await page.getByRole('button', { name: 'Agentを生成・承認' }).click()
    await expect(page.getByText('Active', { exact: true })).toBeVisible({ timeout: 30_000 })

    const submitButton = page.getByRole('button', { name: '注文を受付' })
    await page.getByLabel('数量').fill('0')
    await expect(submitButton).toBeDisabled()
    await expect(page.locator('#order-guidance')).toContainText('数量は0より大きい')
    await page.getByLabel('数量').fill('0.0001')
    await expect(submitButton).toBeEnabled({ timeout: 30_000 })
    await submitButton.click()
    await expect(submitButton).toBeEnabled({ timeout: 15_000 })
    await expect(page.getByRole('alert')).toHaveCount(0)
    await expect
      .poll(
        async () => {
          const refresh = page.getByRole('button', { name: '再読込' })
          if (await refresh.isEnabled()) await refresh.click()
          return page.getByRole('cell', { name: 'Filled' }).count()
        },
        { timeout: 30_000 },
      )
      .toBeGreaterThan(0)
    await expect
      .poll(
        async () => {
          const refresh = page.getByRole('button', { name: '再読込' })
          if (await refresh.isEnabled()) await refresh.click()
          return page.getByRole('heading', { name: '建玉' }).count()
        },
        { timeout: 30_000 },
      )
      .toBe(1)
    await page.getByLabel('BTC Stop Loss').fill('55000')
    await page.getByLabel('BTC Take Profit').fill('65000')
    await page.getByRole('button', { name: 'SL/TP設定' }).click()
    await expect(page.getByRole('button', { name: 'SL/TP設定' })).toBeEnabled({ timeout: 30_000 })
    await page.getByLabel('種別').selectOption('limit')
    await submitButton.click()
    await expect(submitButton).toBeEnabled({ timeout: 15_000 })
    await expect(page.getByRole('alert')).toHaveCount(0)
    const cancelButton = page
      .getByRole('row')
      .filter({ has: page.getByRole('cell', { name: 'limit_gtc' }) })
      .getByRole('button', { name: '取消' })
    await expect
      .poll(
        async () => {
          const refresh = page.getByRole('button', { name: '再読込' })
          if (await refresh.isEnabled()) await refresh.click()
          await expect(refresh).toBeEnabled({ timeout: 5_000 })
          return cancelButton.isEnabled()
        },
        { timeout: 30_000 },
      )
      .toBe(true)
    await cancelButton.click()
    await expect
      .poll(
        async () => {
          const refresh = page.getByRole('button', { name: '再読込' })
          if (await refresh.isEnabled()) await refresh.click()
          return page.getByRole('cell', { name: 'Cancelled' }).count()
        },
        { timeout: 30_000 },
      )
      .toBeGreaterThan(0)

    await page.getByRole('link', { name: '資金' }).click()
    await page.getByLabel('金額').fill('5')
    await page.getByRole('button', { name: 'reserveへ回収' }).click()
    await page.getByRole('button', { name: 'MetaMask署名で出金' }).click()
    await page.getByRole('link', { name: '履歴' }).click()
    await expect(page.getByRole('cell', { name: 'Withdrawal' })).toBeVisible()
    await expect(
      page.getByRole('row').filter({ has: page.getByRole('cell', { name: 'Allocation' }) }),
    ).toContainText('Settled')
    await expect(page.getByRole('cell', { name: 'BTC' }).first()).toBeVisible()
    await page.getByRole('link', { name: '取引', exact: true }).click()
    // 通信失敗は口座表示を保持しても、新規注文をfail-closedにする。
    await expect(submitButton).toBeEnabled({ timeout: 30_000 })
    await page.route('http://127.0.0.1:18100/**', (route) => route.abort('connectionreset'))
    await page.getByRole('button', { name: '再読込' }).click()
    await expect(submitButton).toBeDisabled()
    await expect(page.locator('#order-guidance')).toContainText('更新できません')
    await page.unroute('http://127.0.0.1:18100/**')
    await expect(submitButton).toBeEnabled({ timeout: 30_000 })

    // ICには受付させ、ブラウザへは応答を返さない。注文は一度だけ送信される。
    let submitted = 0
    let allowLookup = false
    await page.route('http://127.0.0.1:18100/**', async (route) => {
      const body = route.request().postDataBuffer()
      if (body?.includes(Buffer.from('get_order_by_request')) && !allowLookup) {
        await route.abort('connectionreset')
      } else if (body?.includes(Buffer.from('submit_order'))) {
        submitted++
        await route.fetch()
        await route.abort('connectionreset')
      } else await route.continue()
    })
    await page.getByLabel('数量').fill('0.0001')
    await submitButton.click()
    await expect(page.getByRole('cell', { name: /応答不明・再送禁止/ })).toBeVisible()
    await expect(submitButton).toBeDisabled()
    allowLookup = true
    await expect(page.getByRole('cell', { name: /応答不明・再送禁止/ })).toHaveCount(0, {
      timeout: 30_000,
    })
    expect(submitted).toBe(1)
    await page.unroute('http://127.0.0.1:18100/**')

    // 自動取得中のログアウトで、旧口座の応答を後から復元させない。
    let release!: () => void
    const held = new Promise<void>((resolve) => {
      release = resolve
    })
    let intercepted!: () => void
    const started = new Promise<void>((resolve) => {
      intercepted = resolve
    })
    await page.route('http://127.0.0.1:18100/**', async (route) => {
      if (route.request().postDataBuffer()?.includes(Buffer.from('get_fund_status'))) {
        const response = await route.fetch()
        intercepted()
        await held
        await route.fulfill({ response })
      } else await route.continue()
    })
    await page.getByRole('button', { name: '再読込' }).click()
    await started
    await page.getByRole('button', { name: 'ログアウト' }).click()
    release()
    await expect(page.getByTestId('account-balances')).toHaveCount(0)
    await page.unroute('http://127.0.0.1:18100/**')
    await expect(page.getByRole('button', { name: 'MetaMaskで接続' })).toBeVisible()
    await page.getByRole('button', { name: 'MetaMaskで接続' }).click()
    await expect(page.getByTestId('account-balances')).toBeVisible()
    // ページ境界を超える実際の資金履歴を用意し、追加ページを読み込む。
    const clients = await createClients(resolveConfig(parseEnv(readFileSync('.env.local', 'utf8'))))
    const challenge = unwrap(
      await clients.vault.issue_challenge({
        principal: clients.principal,
        origin: 'http://127.0.0.1:4173',
        network: { Local: null },
        purpose: { Login: null },
        eoa_address: hexToBytes(address),
      }),
    )
    const signature = execFileSync(signer, [], {
      input: Buffer.from(challenge.typed_data),
      encoding: 'utf8',
    }).trim()
    const fixtureSession = unwrap(
      await clients.vault.open_session({
        challenge_id: challenge.challenge_id,
        eoa_signature: hexToBytes(signature),
      }),
    )
    try {
      for (let batch = 0; batch < 11; batch++) {
        await Promise.all(
          Array.from({ length: 10 }, async () =>
            unwrap(
              await clients.vault.request_allocation({
                session: fixtureSession,
                client_request_id: crypto.getRandomValues(new Uint8Array(32)),
                amount: 1n,
                target: { Trading: null },
                intent_signature: [],
              }),
            ),
          ),
        )
      }
    } finally {
      await clients.vault.revoke_session(fixtureSession)
    }
    await page.getByRole('link', { name: '履歴', exact: true }).click()
    await expect(page.getByRole('button', { name: '資金履歴をさらに表示' })).toBeVisible({
      timeout: 45_000,
    })
    await page.getByRole('button', { name: '資金履歴をさらに表示' }).click()
    const fundTable = page
      .getByRole('heading', { name: '資金履歴' })
      .locator('..')
      .locator('tbody tr')
    await expect.poll(() => fundTable.count()).toBeGreaterThan(100)
    await page.getByRole('button', { name: 'ログアウト' }).click()
    await page.evaluate((account) => {
      ;(globalThis as typeof globalThis & { __e2eAccount?: string }).__e2eAccount = account
    }, secondaryAddress)
    await page.getByRole('button', { name: 'MetaMaskで接続' }).click()
    await expect(
      page.getByText(`${secondaryAddress.slice(0, 10)}…${secondaryAddress.slice(-6)}`),
    ).toBeVisible()
    await expect(fundTable).toHaveCount(0)
    await expect(page.getByRole('button', { name: '資金履歴をさらに表示' })).toHaveCount(0)
    await page.getByRole('button', { name: 'ログアウト' }).click()
    expect(
      await page.evaluate(() => ({ local: localStorage.length, session: sessionStorage.length })),
    ).toEqual({ local: 0, session: 0 })
  })
})
