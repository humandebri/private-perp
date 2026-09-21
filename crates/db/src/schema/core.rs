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
    venue_state TEXT,
    state TEXT NOT NULL CHECK (state IN ('pending', 'open', 'partially_filled', 'filled', 'cancelled', 'rejected', 'unknown')),
    filled_quantity TEXT NOT NULL DEFAULT '0',
    hl_oid INTEGER,
    cancel_requested INTEGER NOT NULL DEFAULT 0 CHECK (cancel_requested IN (0, 1)),
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
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
    fee INTEGER NOT NULL CHECK (fee >= 0),
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
];
