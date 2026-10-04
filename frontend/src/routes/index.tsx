import { createFileRoute, Link } from '@tanstack/react-router'

const isTestnet = import.meta.env.VITE_APP_STAGE === 'testnet'

export const Route = createFileRoute('/')({ component: Home })
function Home() {
  return (
    <main className="landing">
      <div className="eyebrow">
        PRIVATE PERPETUALS / {isTestnet ? 'HL TESTNET' : 'LOCAL INTEGRATION'}
      </div>
      <h1>
        Focus on trading.
        <br />
        <span>Keep less information public.</span>
      </h1>
      <p className="lead">
        Hyperliquid trading with custody on ICP.
        <br />
        {isTestnet
          ? 'Connect MetaMask to try custody and trading on HL testnet.'
          : 'A localhost development environment with MetaMask authentication and real canisters.'}
      </p>
      <div className="hero-actions">
        <Link to="/trade" className="primary button">
          {isTestnet ? 'Open testnet trading ↗' : 'Open local trading ↗'}
        </Link>
        <Link to="/funds" className="button secondary">
          Explore funding
        </Link>
      </div>
      <div className="feature-grid">
        <article>
          <span>01 / EXECUTION</span>
          <h2>Native HL execution</h2>
          <p>HL handles fills, margin and liquidations. There is no custom liquidation engine.</p>
        </article>
        <article>
          <span>02 / CUSTODY</span>
          <h2>Separate custody and trading access</h2>
          <p>
            The canister holds the master keys. Recovery without the canister is not guaranteed.
          </p>
        </article>
        <article>
          <span>03 / PRIVACY</span>
          <h2>Privacy requires evidence</h2>
          <p>
            Trading activity on HL is public. Resistance to amount and timing correlation has not
            been validated.
          </p>
        </article>
      </div>
      <aside className="notice">
        {isTestnet
          ? 'Connects a public IC canister to Hyperliquid testnet. Use test USDC only. Register eligibility before allocating funds or placing new orders.'
          : 'LOCAL MOCK uses no real funds. Connections are restricted to the local IC replica and mock venue; testnet and mainnet are disabled.'}
      </aside>
    </main>
  )
}
