//! `trading_core` のスキーマ。`Implementation.md` 4.3。
//!
//! S2では表の定義のみを行い、操作はS3で実装する。

use crate::Migration;

/// ユーザー・HL口座・Agent・受付・action・nonce。
const CORE: &str = "
CREATE TABLE users (
    user_id BLOB PRIMARY KEY NOT NULL CHECK (length(user_id) = 32),
    status TEXT NOT NULL CHECK (status IN ('active', 'frozen')),
    created_at INTEGER NOT NULL
);

CREATE TABLE accounts (
    account_id BLOB PRIMARY KEY NOT NULL CHECK (length(account_id) = 32),
    user_id BLOB NOT NULL REFERENCES users (user_id),
    hl_master_address BLOB NOT NULL CHECK (length(hl_master_address) = 20),
    ownership_checked_at INTEGER,
    state TEXT NOT NULL CHECK (state IN ('pending', 'active', 'stopped')),
    created_at INTEGER NOT NULL
);

CREATE TABLE agents (
    account_id BLOB NOT NULL REFERENCES accounts (account_id),
    generation INTEGER NOT NULL CHECK (generation > 0),
    agent_address BLOB NOT NULL CHECK (length(agent_address) = 20),
    derivation_path TEXT NOT NULL,
    approved_at INTEGER,
    expires_at INTEGER,
    revoked_at INTEGER,
    checked_at INTEGER,
    state TEXT NOT NULL CHECK (state IN ('requested', 'approving', 'active', 'expiring', 'revoked', 'failed')),
    PRIMARY KEY (account_id, generation),
    UNIQUE (agent_address)
);

CREATE TABLE requests (
    user_id BLOB NOT NULL CHECK (length(user_id) = 32),
    client_request_id BLOB NOT NULL,
    body_hash BLOB NOT NULL CHECK (length(body_hash) = 32),
    accepted_at INTEGER NOT NULL,
    PRIMARY KEY (user_id, client_request_id)
);

CREATE TABLE actions (
    action_id BLOB PRIMARY KEY NOT NULL CHECK (length(action_id) = 32),
    account_id BLOB NOT NULL,
    agent_address BLOB NOT NULL CHECK (length(agent_address) = 20),
    generation INTEGER NOT NULL,
    network TEXT NOT NULL,
    nonce INTEGER NOT NULL,
    expires_after INTEGER,
    canonical_action BLOB NOT NULL,
    digest BLOB NOT NULL CHECK (length(digest) = 32),
    signature BLOB,
    wire_payload BLOB,
    dispatch_state TEXT NOT NULL CHECK (dispatch_state IN ('queued', 'signing', 'signed', 'dispatching', 'reconciled', 'unknown', 'aborted')),
    worker_epoch INTEGER NOT NULL DEFAULT 0,
    lease_until INTEGER,
    attempt INTEGER NOT NULL DEFAULT 0,
    policy_version INTEGER,
    dispatch_started_at INTEGER,
    next_check_at INTEGER,
    response_summary TEXT,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    UNIQUE (agent_address, nonce),
    CHECK (
        (dispatch_state IN ('dispatching', 'reconciled', 'unknown') AND signature IS NOT NULL AND wire_payload IS NOT NULL)
        OR (dispatch_state IN ('queued', 'signing', 'aborted') AND signature IS NULL)
        OR (dispatch_state = 'signed' AND signature IS NOT NULL AND wire_payload IS NOT NULL)
    )
);

CREATE TABLE nonces (
    agent_address BLOB PRIMARY KEY NOT NULL CHECK (length(agent_address) = 20),
    last_nonce INTEGER NOT NULL
);
";

/// 注文・約定・リスク予約・メタデータ。
const ORDERS: &str = "
CREATE TABLE orders (
    dispatch_state TEXT NOT NULL DEFAULT 'queued',
    wire_payload BLOB,
    signature BLOB,
    cancel_dispatch_state TEXT,
    order_id BLOB PRIMARY KEY NOT NULL CHECK (length(order_id) = 32),
    user_id BLOB NOT NULL,
    account_id BLOB NOT NULL,
    client_request_id BLOB NOT NULL,
    cloid BLOB UNIQUE CHECK (length(cloid) = 16),
    market TEXT NOT NULL,
    asset_index INTEGER NOT NULL,
    side TEXT NOT NULL CHECK (side IN ('buy', 'sell')),
    kind TEXT NOT NULL CHECK (kind IN ('market_ioc', 'limit_gtc')),
    price TEXT,
    quantity TEXT NOT NULL,
    reduce_only INTEGER NOT NULL CHECK (reduce_only IN (0, 1)),
    trigger_kind TEXT CHECK (trigger_kind IN ('stop_loss', 'take_profit')),
    trigger_price TEXT,
    trigger_is_market INTEGER CHECK (trigger_is_market IN (0, 1)),
    venue_state TEXT,
    state TEXT NOT NULL CHECK (state IN ('pending', 'open', 'partially_filled', 'filled', 'cancelled', 'rejected', 'unknown')),
    filled_quantity TEXT NOT NULL DEFAULT '0',
    hl_oid INTEGER,
    cancel_requested INTEGER NOT NULL DEFAULT 0 CHECK (cancel_requested IN (0, 1)),
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    -- トリガ（SL/TP）の列は3つ揃っているか、すべてNULLかのいずれかとする。
    CHECK (
        (trigger_kind IS NULL AND trigger_price IS NULL AND trigger_is_market IS NULL)
        OR (trigger_kind IS NOT NULL AND trigger_price IS NOT NULL AND trigger_is_market IS NOT NULL)
    )
);

CREATE INDEX orders_by_account ON orders (account_id, state, updated_at);

CREATE TABLE action_orders (
    action_id BLOB NOT NULL,
    order_id BLOB NOT NULL,
    operation TEXT NOT NULL CHECK (operation IN ('place', 'cancel', 'modify')),
    position INTEGER NOT NULL,
    PRIMARY KEY (action_id, order_id, operation)
);

CREATE TABLE order_events (
    event_id INTEGER PRIMARY KEY AUTOINCREMENT,
    order_id BLOB NOT NULL,
    from_state TEXT,
    to_state TEXT NOT NULL,
    reason_code TEXT,
    at INTEGER NOT NULL
);

CREATE TABLE risk_reservations (
    reservation_id INTEGER PRIMARY KEY AUTOINCREMENT,
    account_id BLOB NOT NULL,
    client_request_id BLOB NOT NULL,
    notional INTEGER NOT NULL CHECK (notional > 0),
    state TEXT NOT NULL CHECK (state IN ('held', 'released', 'consumed')),
    created_at INTEGER NOT NULL,
    released_at INTEGER,
    UNIQUE (account_id, client_request_id)
);

CREATE TABLE meta_cache (
    network TEXT NOT NULL,
    dex TEXT NOT NULL,
    fetched_at INTEGER NOT NULL,
    digest BLOB NOT NULL CHECK (length(digest) = 32),
    universe TEXT NOT NULL,
    PRIMARY KEY (network, dex)
);
";

/// 運営が設定するブートストラップ値（vault principalなど）。
const CONFIG: &str = "
CREATE TABLE core_config (
    singleton INTEGER PRIMARY KEY CHECK (singleton = 1),
    vault_principal BLOB,
    policy_principal BLOB,
    network TEXT,
    dex TEXT
);
";

/// Agent世代（`Implementation.md` 7章。鍵はcoreが導出・保管し、vaultはmaster署名で承認する）。
const AGENTS: &str = "
CREATE TABLE agent_generations (
    account_id BLOB NOT NULL CHECK (length(account_id) = 32),
    generation INTEGER NOT NULL CHECK (generation > 0),
    agent_address BLOB NOT NULL CHECK (length(agent_address) = 20),
    derivation_path TEXT NOT NULL,
    approved_at INTEGER,
    expires_at INTEGER,
    revoked_at INTEGER,
    state TEXT NOT NULL CHECK (state IN ('requested', 'approving', 'active', 'expiring', 'revoked', 'failed')),
    created_at INTEGER NOT NULL,
    PRIMARY KEY (account_id, generation),
    UNIQUE (agent_address)
);
";

/// 約定（`/info`照合で取り込む）。
const FILLS: &str = "
CREATE TABLE fills (
    fill_id INTEGER PRIMARY KEY AUTOINCREMENT,
    tid INTEGER NOT NULL UNIQUE,
    user_id BLOB NOT NULL CHECK (length(user_id) = 32),
    order_id BLOB NOT NULL CHECK (length(order_id) = 32),
    market TEXT NOT NULL,
    price TEXT NOT NULL,
    quantity TEXT NOT NULL,
    fee INTEGER NOT NULL,
    filled_at INTEGER NOT NULL
);
";

/// 建玉（`/info`のclearinghouseState照合で更新する）。
const POSITIONS: &str = "
CREATE TABLE positions (
    account_id BLOB NOT NULL CHECK (length(account_id) = 32),
    market TEXT NOT NULL,
    size TEXT NOT NULL,
    entry_price TEXT NOT NULL,
    liquidation_price TEXT,
    unrealized_pnl INTEGER NOT NULL,
    leverage INTEGER NOT NULL,
    margin_mode TEXT NOT NULL,
    stop_loss TEXT,
    take_profit TEXT,
    observed_at INTEGER NOT NULL,
    PRIMARY KEY (account_id, market)
);
";

/// HPKEの鍵世代（`Plan.md` 16.5、`api-contract.md` 6節）。秘密鍵はcanister外へ出さない。
const HPKE_KEYS: &str = "
CREATE TABLE hpke_keys (
    generation INTEGER PRIMARY KEY CHECK (generation > 0),
    secret BLOB NOT NULL CHECK (length(secret) = 32),
    public BLOB NOT NULL CHECK (length(public) = 32),
    created_at INTEGER NOT NULL,
    retired_at INTEGER
);
";

/// 封筒の`request_id`の単回使用（再送拒否）。期限切れは随時掃除する。
const HPKE_REQUESTS: &str = "
CREATE TABLE hpke_requests (
    request_id BLOB PRIMARY KEY NOT NULL CHECK (length(request_id) = 32),
    method TEXT NOT NULL,
    caller BLOB NOT NULL,
    received_at INTEGER NOT NULL,
    expires_at INTEGER NOT NULL
);

CREATE INDEX hpke_requests_by_expiry ON hpke_requests (expires_at);
";

/// v10: 環境設定（HL endpointとtECDSA key ID）。
///
/// networkは`core_config.network`（`set_market_context`）を出所とし、この表は
/// endpointとkey IDを持つ。環境は「network」「鍵」「endpoint」で分離し、起動時に
/// 検証可能な値で判定する（`docs/phase-0/environments.md` 4節）。
const ENVIRONMENT: &str = "
CREATE TABLE core_environment (
    singleton INTEGER PRIMARY KEY CHECK (singleton = 1),
    exchange_url TEXT,
    info_url TEXT,
    ecdsa_key_id TEXT,
    updated_at INTEGER
);
";

/// v11: 注文outboxのlease、注文契約値、空建玉を含む観測時刻、hot path index。
const ORDER_RELIABILITY: &str = "
CREATE TABLE account_observations (
    account_id BLOB PRIMARY KEY NOT NULL CHECK (length(account_id) = 32),
    observed_at INTEGER NOT NULL
);

ALTER TABLE orders ADD COLUMN worker_epoch INTEGER NOT NULL DEFAULT 0;
ALTER TABLE orders ADD COLUMN lease_until INTEGER;
ALTER TABLE orders ADD COLUMN attempt INTEGER NOT NULL DEFAULT 0;
ALTER TABLE orders ADD COLUMN next_check_at INTEGER;
ALTER TABLE orders ADD COLUMN last_error TEXT;
ALTER TABLE orders ADD COLUMN cancel_worker_epoch INTEGER NOT NULL DEFAULT 0;
ALTER TABLE orders ADD COLUMN cancel_lease_until INTEGER;
ALTER TABLE orders ADD COLUMN cancel_attempt INTEGER NOT NULL DEFAULT 0;
ALTER TABLE orders ADD COLUMN effective_leverage INTEGER NOT NULL DEFAULT 3;
ALTER TABLE orders ADD COLUMN slippage_tolerance_bps INTEGER;
ALTER TABLE orders ADD COLUMN expires_after INTEGER;
ALTER TABLE orders ADD COLUMN preflight_state TEXT NOT NULL DEFAULT 'queued';
ALTER TABLE orders ADD COLUMN preflight_wire_payload BLOB;
ALTER TABLE orders ADD COLUMN preflight_signature BLOB;
ALTER TABLE orders ADD COLUMN preflight_nonce INTEGER;
ALTER TABLE orders ADD COLUMN order_nonce INTEGER;
ALTER TABLE orders ADD COLUMN cancel_nonce INTEGER;
ALTER TABLE orders ADD COLUMN cancel_wire_payload BLOB;
ALTER TABLE orders ADD COLUMN cancel_signature BLOB;

CREATE INDEX orders_dispatch_queue ON orders (dispatch_state, lease_until, next_check_at, created_at);
CREATE INDEX orders_cancel_queue ON orders (cancel_requested, cancel_dispatch_state, cancel_lease_until, updated_at);
CREATE INDEX orders_by_user ON orders (user_id);
CREATE INDEX orders_by_oid ON orders (account_id, hl_oid);
CREATE INDEX fills_by_user ON fills (user_id, fill_id);
CREATE INDEX risk_reservations_held ON risk_reservations (account_id, state);
";

/// v12: 実装で置き換え済みの旧テーブルを、安全確認後に削除する。
/// 既存データが1件でもあればCHECK違反でmigration全体をrollbackする。
const REMOVE_LEGACY_TABLES: &str = "
CREATE TABLE core_legacy_cleanup_guard (
    ok INTEGER NOT NULL CHECK (ok = 1)
);
INSERT INTO core_legacy_cleanup_guard (ok)
SELECT CASE WHEN
    (SELECT COUNT(*) FROM agents) = 0 AND
    (SELECT COUNT(*) FROM actions) = 0 AND
    (SELECT COUNT(*) FROM action_orders) = 0 AND
    (SELECT COUNT(*) FROM order_events) = 0
THEN 1 ELSE 0 END;
DROP TABLE order_events;
DROP TABLE action_orders;
DROP TABLE actions;
DROP TABLE agents;
DROP TABLE core_legacy_cleanup_guard;
";

/// v13: controllerによる不明preflight解決の監査証跡。
const ORDER_RESOLUTION_EVENTS: &str = "
CREATE TABLE order_resolution_events (
    event_id INTEGER PRIMARY KEY AUTOINCREMENT,
    order_id BLOB NOT NULL CHECK (length(order_id) = 32),
    actor BLOB NOT NULL,
    outcome TEXT NOT NULL CHECK (outcome IN ('applied', 'rejected')),
    at INTEGER NOT NULL
);
CREATE INDEX order_resolution_events_by_order ON order_resolution_events (order_id, event_id);
";

/// v14: 取引所が返す口座サマリを注文リスク予約と分離して保存する。
const ACCOUNT_METRICS: &str = "
CREATE TABLE account_metrics (
    account_id BLOB PRIMARY KEY NOT NULL CHECK (length(account_id) = 32),
    margin_used INTEGER NOT NULL CHECK (margin_used >= 0),
    unrealized_pnl INTEGER NOT NULL,
    observed_at INTEGER NOT NULL
);
";

/// v15: userFills is expensive; retain its last successful poll across upgrades.
const FILL_POLL: &str = "
ALTER TABLE account_observations ADD COLUMN last_fills_checked_at INTEGER;
";

/// v16: unresolved orders get one priority reconciliation slot per sweep.
const PRIORITY_RECONCILE_CURSOR: &str = "
CREATE TABLE reconcile_priority_cursor (
    singleton INTEGER PRIMARY KEY CHECK (singleton = 1),
    last_account_id BLOB NOT NULL CHECK (length(last_account_id) = 32),
    updated_at INTEGER NOT NULL
);
";

/// v17: vaultとの回収プロトコル。終端行を保持して世代を再利用しない。
const RECOVERY_FENCES: &str = "
CREATE TABLE recovery_fences (
    account_id BLOB PRIMARY KEY NOT NULL CHECK (length(account_id) = 32),
    user_id BLOB NOT NULL CHECK (length(user_id) = 32),
    request_id BLOB NOT NULL,
    epoch INTEGER NOT NULL CHECK (epoch > 0),
    state TEXT NOT NULL CHECK (state IN ('preparing', 'ready', 'committed', 'unknown', 'released')),
    checked_at INTEGER,
    updated_at INTEGER NOT NULL
);
CREATE TABLE recovery_migration_lock (
    id INTEGER PRIMARY KEY CHECK (id = 1),
    locked INTEGER NOT NULL CHECK (locked IN (0, 1))
);
INSERT INTO recovery_migration_lock (id, locked) VALUES (1, 0);
";

/// 照合の巡回カーソル（有効な口座を順に巡回する）。
const RECONCILE_CURSOR: &str = "
CREATE TABLE reconcile_cursor (
    singleton INTEGER PRIMARY KEY CHECK (singleton = 1),
    last_account_id BLOB NOT NULL CHECK (length(last_account_id) = 32),
    updated_at INTEGER NOT NULL
);
";

/// v27: 口座・銘柄ごとの確定済みレバレッジと、送信中の変更を区別する。
const LEVERAGE_CACHE: &str = "
CREATE TABLE leverage_cache (
    account_id BLOB NOT NULL CHECK (length(account_id) = 32),
    asset_index INTEGER NOT NULL CHECK (asset_index >= 0),
    confirmed_leverage INTEGER CHECK (confirmed_leverage > 0),
    confirmed_at INTEGER,
    pending_order_id BLOB CHECK (pending_order_id IS NULL OR length(pending_order_id) = 32),
    pending_leverage INTEGER CHECK (pending_leverage IS NULL OR pending_leverage > 0),
    pending_state TEXT CHECK (pending_state IN ('reserved', 'unknown')),
    PRIMARY KEY (account_id, asset_index),
    CHECK ((pending_order_id IS NULL AND pending_leverage IS NULL AND pending_state IS NULL)
        OR (pending_order_id IS NOT NULL AND pending_leverage IS NOT NULL AND pending_state IS NOT NULL))
);
";

/// `trading_core` のMigration一覧。
pub const MIGRATIONS: &[Migration] = &[
    Migration {
        version: 1,
        sql: CORE,
    },
    Migration {
        version: 2,
        sql: ORDERS,
    },
    Migration {
        version: 3,
        sql: CONFIG,
    },
    Migration {
        version: 4,
        sql: AGENTS,
    },
    Migration {
        version: 5,
        sql: FILLS,
    },
    Migration {
        version: 6,
        sql: POSITIONS,
    },
    Migration {
        version: 7,
        sql: HPKE_KEYS,
    },
    Migration {
        version: 8,
        sql: HPKE_REQUESTS,
    },
    Migration {
        version: 9,
        sql: RECONCILE_CURSOR,
    },
    Migration {
        version: 10,
        sql: ENVIRONMENT,
    },
    Migration {
        version: 11,
        sql: ORDER_RELIABILITY,
    },
    Migration {
        version: 12,
        sql: REMOVE_LEGACY_TABLES,
    },
    Migration {
        version: 13,
        sql: ORDER_RESOLUTION_EVENTS,
    },
    Migration {
        version: 14,
        sql: ACCOUNT_METRICS,
    },
    Migration {
        version: 15,
        sql: FILL_POLL,
    },
    Migration {
        version: 16,
        sql: PRIORITY_RECONCILE_CURSOR,
    },
    Migration {
        version: 17,
        sql: RECOVERY_FENCES,
    },
    Migration {
        version: 18,
        sql: super::send_journal::CLIENT_SQL,
    },
    Migration {
        version: 19,
        sql: "ALTER TABLE send_journal_client ADD COLUMN guard BLOB;",
    },
    Migration {
        version: 20,
        sql: super::send_journal::STAGE_SQL,
    },
    Migration {
        version: 21,
        sql: super::send_journal::CYCLES_SQL,
    },
    Migration {
        version: 22,
        sql: "CREATE TABLE market_thresholds (
        market TEXT PRIMARY KEY CHECK(market IN ('BTC','ETH')),
        expected_index INTEGER NOT NULL CHECK(expected_index >= 0),
        min_day_notional_usdc INTEGER NOT NULL CHECK(min_day_notional_usdc > 0),
        max_spread_bps INTEGER NOT NULL CHECK(max_spread_bps BETWEEN 1 AND 10000),
        min_each_side_depth_usdc INTEGER NOT NULL CHECK(min_each_side_depth_usdc > 0)
      );
      CREATE TABLE market_observations (
        market TEXT PRIMARY KEY CHECK(market IN ('BTC','ETH')),
        observed_at INTEGER,
        checked_at INTEGER NOT NULL,
        reason_code TEXT,
        asset_index INTEGER,
        day_notional_usdc INTEGER,
        spread_bps INTEGER,
        bid_depth_usdc INTEGER,
        ask_depth_usdc INTEGER
      );",
    },
    Migration {
        version: 23,
        sql: super::send_journal::RECOVERY_STAGE_SQL,
    },
    Migration {
        version: 24,
        sql: super::send_journal::CLIENT_WRITER_FENCE_SQL,
    },
    Migration {
        version: 25,
        sql: super::send_journal::RECOVERY_RECEIPTS_SQL,
    },
    Migration {
        version: 26,
        sql: super::send_journal::REPLAY_VALIDATION_SQL,
    },
    Migration {
        version: 27,
        sql: LEVERAGE_CACHE,
    },
];
