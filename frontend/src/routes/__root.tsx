import { createRootRoute, HeadContent, Link, Scripts } from '@tanstack/react-router'
import type { ReactNode } from 'react'
import stylesheet from '../styles.css?url'

export const Route = createRootRoute({
  head: () => ({
    meta: [
      { charSet: 'utf-8' },
      { name: 'viewport', content: 'width=device-width, initial-scale=1' },
      { title: 'VEIL / Private Perpetuals — Demo' },
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
          </nav>
          <span className="badge">DEMO · 合成データ</span>
        </header>
        {children}
        <footer>
          <span>
            <i className="status-dot" />
            ローカルシミュレーション
          </span>
          <span>ICP 未接続 · 実資金・ウォレットは使用しません</span>
        </footer>
        <Scripts />
      </body>
    </html>
  )
}
