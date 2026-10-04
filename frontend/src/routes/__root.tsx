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
      { title: `VEIL / Private Perpetuals — ${isTestnet ? 'Testnet' : 'Local'}` },
    ],
    links: [{ rel: 'stylesheet', href: stylesheet }],
  }),
  shellComponent: Document,
  notFoundComponent: () => (
    <main className="page">
      <h1>Page not found</h1>
      <Link to="/">Go home</Link>
    </main>
  ),
})

function Document({ children }: { children: ReactNode }) {
  return (
    <html lang="en">
      <head>
        <HeadContent />
      </head>
      <body>
        <header className="topbar">
          <Link to="/" className="brand">
            <span className="brand-mark">V</span> VEIL<span className="brand-sub"> / PERPS</span>
          </Link>
          <nav aria-label="Main navigation">
            <Link to="/trade">Trade</Link>
            <Link to="/funds">Funds</Link>
            <Link to="/history">History</Link>
          </nav>
          <span className="badge">{isTestnet ? 'HL TESTNET' : 'LOCAL MOCK'}</span>
        </header>
        <LocalSessionProvider>{children}</LocalSessionProvider>
        <footer>
          <span>
            <i className="status-dot" />
            {isTestnet ? 'Public canister connection' : 'Local canister connection'}
          </span>
          <span>
            {isTestnet
              ? 'HL testnet only · Test USDC only'
              : 'Localhost only · No real funds, testnet or mainnet'}
          </span>
        </footer>
        <Scripts />
      </body>
    </html>
  )
}
