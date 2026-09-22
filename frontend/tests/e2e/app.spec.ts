import { execFileSync } from 'node:child_process'
import { resolve } from 'node:path'
import { expect, test } from '@playwright/test'

test('SSR exposes a local-only shell and refuses writes', async ({ request }) => {
  for (const path of ['/', '/trade', '/funds', '/history']) {
    const response = await request.get(path)
    expect(response.ok()).toBeTruthy()
    expect(response.headers()['x-frame-options']).toBe('DENY')
    expect(response.headers()['cache-control']).toBe('no-store')
    expect(response.headers()['content-security-policy']).toContain('http://127.0.0.1:18100')
    const html = await response.text()
    expect(html).toContain('LOCAL MOCK')
    expect(html).not.toContain('data-testid="account-panel"')
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

test.describe('real local canister flow', () => {
  test.skip(process.env.LOCAL_E2E !== '1', 'scripts/local-e2e.sh owns the local replica')
  test('login, funds, agent, orders, recovery, withdrawal, history and logout', async ({
    page,
  }) => {
    test.setTimeout(120_000)
    const signer = resolve(process.cwd(), '../target/debug/e2e-signer')
    const address = execFileSync(signer, ['address'], { encoding: 'utf8' }).trim()
    await page.exposeFunction('__e2eSignTypedData', (typedData: string) =>
      execFileSync(signer, [], { input: typedData, encoding: 'utf8' }).trim(),
    )
    await page.addInitScript(
      ({ account }) => {
        Object.defineProperty(window, 'ethereum', {
          value: {
            request: async ({ method, params }: { method: string; params?: unknown[] }) => {
              if (method === 'eth_requestAccounts') return [account]
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

    await page.getByLabel('数量').fill('0.0001')
    const submitButton = page.getByRole('button', { name: '注文を受付' })
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
    await expect(page.getByRole('cell', { name: 'BTC' })).toBeVisible()
    await page.getByRole('button', { name: 'ログアウト' }).click()
    await expect(page.getByRole('button', { name: 'MetaMaskで接続' })).toBeVisible()
    expect(
      await page.evaluate(() => ({ local: localStorage.length, session: sessionStorage.length })),
    ).toEqual({ local: 0, session: 0 })
  })
})
