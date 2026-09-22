import { createFileRoute, Link } from '@tanstack/react-router'

export const Route = createFileRoute('/')({ component: Home })
function Home() {
  return (
    <main className="landing">
      <div className="eyebrow">PRIVATE PERPETUALS / LOCAL INTEGRATION</div>
      <h1>
        取引に集中する。
        <br />
        <span>公開する情報は、少なく。</span>
      </h1>
      <p className="lead">
        Hyperliquidの取引体験と、ICPによる資金管理をつなぐ。
        <br />
        MetaMask認証と実Canisterを一巡させる、loopback限定の開発環境。
      </p>
      <div className="hero-actions">
        <Link to="/trade" className="primary button">
          ローカル取引画面を開く ↗
        </Link>
        <Link to="/funds" className="button secondary">
          資金フローを見る
        </Link>
      </div>
      <div className="feature-grid">
        <article>
          <span>01 / EXECUTION</span>
          <h2>HL標準の取引</h2>
          <p>約定・証拠金・清算はHLへ。独自の清算エンジンは作りません。</p>
        </article>
        <article>
          <span>02 / CUSTODY</span>
          <h2>資金と取引権限を分離</h2>
          <p>Canisterがmaster鍵を管理する設計。ユーザー単独の回収保証はありません。</p>
        </article>
        <article>
          <span>03 / PRIVACY</span>
          <h2>機密性は検証して示す</h2>
          <p>HL上の取引情報は公開です。金額・時刻の相関への耐性は未検証です。</p>
        </article>
      </div>
      <aside className="notice">
        LOCAL MOCKは実資金を扱いません。接続先はloopbackのIC replicaとmock
        venueだけに制限され、testnet・mainnetでは起動しません。
      </aside>
    </main>
  )
}
