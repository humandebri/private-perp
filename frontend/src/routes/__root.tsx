import { createRootRoute, HeadContent, Link, Scripts } from '@tanstack/react-router'
import type { ReactNode } from 'react'
import stylesheet from '../styles.css?url'
import { LocalSessionProvider } from '../ui/local-session'

const isTestnet = import.meta.env.VITE_APP_STAGE === 'testnet'

export const Route = createRootRoute({
  head: () => ({
    meta: [
      { charSet: 'utf-8' },
      { name: 'viewport', content: 'width=device-width, initial-scale=1' },
      { title: `VEIL / Private Perpetuals — ${isTestnet ? 'HL Testnet' : 'Local'}` },
    ],
    links: [{ rel: 'stylesheet', href: stylesheet }],
  }),
  shellComponent: Document,
  notFoundComponent: () => (
    <main className="page">
      <h1>ページが見つかりません</h1>
      <Link to="/">ホームへ</Link>
    </main>
  ),
})

function Document({ children }: { children: ReactNode }) {
  return (
    <html lang="ja">
      <head>
        <HeadContent />
      </head>
      <body>
        <header className="topbar">
          <Link to="/" className="brand">
            <span className="brand-mark">V</span> VEIL<span className="brand-sub"> / PERPS</span>
          </Link>
          <nav aria-label="メインナビゲーション">
            <Link to="/trade">取引</Link>
            <Link to="/funds">資金</Link>
            <Link to="/history">履歴</Link>
            <Link to="/fallback">最小クライアント</Link>
          </nav>
          <span className="badge">{isTestnet ? 'HL TESTNET' : 'LOCAL MOCK'}</span>
        </header>
        <LocalSessionProvider>{children}</LocalSessionProvider>
        <footer>
          <span>
            <i className="status-dot" />
            {isTestnet ? 'ICP公開Canister接続' : 'ローカルCanister接続'}
          </span>
          <span>
            {isTestnet
              ? 'HL testnetの模擬USDCのみ · 本番資金は対象外'
              : 'loopback限定 · 実資金・testnet・mainnetは対象外'}
          </span>
        </footer>
        <Scripts />
      </body>
    </html>
  )
}
