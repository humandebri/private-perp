//! `funds_vault` のスキーマ。`Implementation.md` 14.1、`docs/phase-0/api-contract.md` 2節。

use crate::Migration;

/// 認証（EOAとuser_idの対応、challenge、セッション）。
const AUTH: &str = "
CREATE TABLE identities (
    eoa_address BLOB PRIMARY KEY NOT NULL CHECK (length(eoa_address) = 20),
    user_id BLOB NOT NULL UNIQUE CHECK (length(user_id) = 32),
    status TEXT NOT NULL CHECK (status IN ('active', 'frozen')),
    revocation_generation INTEGER NOT NULL DEFAULT 0 CHECK (revocation_generation >= 0),
    created_at INTEGER NOT NULL,
    last_login_at INTEGER
);

CREATE TABLE challenges (
    challenge_id BLOB PRIMARY KEY NOT NULL CHECK (length(challenge_id) = 32),
    nonce BLOB NOT NULL UNIQUE CHECK (length(nonce) = 32),
    eoa_address BLOB NOT NULL CHECK (length(eoa_address) = 20),
    principal BLOB NOT NULL,
    purpose TEXT NOT NULL CHECK (purpose IN ('login', 'withdrawal')),
    network TEXT NOT NULL,
    origin TEXT NOT NULL,
    issued_at INTEGER NOT NULL,
    expires_at INTEGER NOT NULL,
    consumed_at INTEGER
);

CREATE INDEX challenges_by_expiry ON challenges (expires_at);

CREATE TABLE sessions (
    session_id BLOB PRIMARY KEY NOT NULL CHECK (length(session_id) = 32),
    user_id BLOB NOT NULL REFERENCES identities (user_id),
    principal BLOB NOT NULL,
    issued_at INTEGER NOT NULL,
    expires_at INTEGER NOT NULL,
    revocation_generation INTEGER NOT NULL,
    revoked_at INTEGER
);

CREATE INDEX sessions_by_user ON sessions (user_id, expires_at);
";

/// 口座と複式台帳。
///
/// `postings.amount` は符号付き整数（正=借方、負=貸方）で、journal内の合計が0で
/// なければならない。残高はpostingsから導出する。
const LEDGER: &str = "
CREATE TABLE custody_accounts (
    account_id BLOB PRIMARY KEY NOT NULL CHECK (length(account_id) = 32),
    user_id BLOB NOT NULL REFERENCES identities (user_id),
    kind TEXT NOT NULL CHECK (kind IN ('reserve', 'trading')),
    derivation_path TEXT NOT NULL UNIQUE,
    master_address BLOB NOT NULL CHECK (length(master_address) = 20),
    network TEXT NOT NULL,
    state TEXT NOT NULL CHECK (state IN ('pending', 'active', 'stopped')),
    created_at INTEGER NOT NULL
);

CREATE INDEX custody_accounts_by_user ON custody_accounts (user_id, kind);

CREATE TABLE accounts (
    account_id INTEGER PRIMARY KEY AUTOINCREMENT,
    name TEXT NOT NULL UNIQUE,
    kind TEXT NOT NULL CHECK (kind IN ('asset', 'liability', 'suspense', 'income')),
    asset TEXT NOT NULL CHECK (asset IN ('usdc'))
);

CREATE TABLE journals (
    journal_id INTEGER PRIMARY KEY AUTOINCREMENT,
    kind TEXT NOT NULL,
    external_event_id BLOB,
    request_id BLOB,
    memo TEXT,
    at INTEGER NOT NULL
);

CREATE UNIQUE INDEX journals_by_external_event ON journals (external_event_id)
    WHERE external_event_id IS NOT NULL;

CREATE TABLE postings (
    journal_id INTEGER NOT NULL REFERENCES journals (journal_id),
    account_id INTEGER NOT NULL REFERENCES accounts (account_id),
    amount INTEGER NOT NULL CHECK (amount <> 0),
    PRIMARY KEY (journal_id, account_id)
);

CREATE INDEX postings_by_account ON postings (account_id);
";

/// 資金要求・予約・outbox・nonce。
const FUNDS: &str = "
CREATE TABLE fund_requests (
    user_id BLOB NOT NULL CHECK (length(user_id) = 32),
    client_request_id BLOB NOT NULL,
    body_hash BLOB NOT NULL CHECK (length(body_hash) = 32),
    kind TEXT NOT NULL CHECK (kind IN ('allocation', 'recovery', 'withdrawal')),
    account_id BLOB CHECK (length(account_id) = 32),
    amount INTEGER NOT NULL CHECK (amount > 0),
    destination TEXT,
    state TEXT NOT NULL CHECK (state IN ('accepted', 'reserved', 'executing', 'settled', 'rejected', 'unknown')),
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    PRIMARY KEY (user_id, client_request_id)
);

CREATE INDEX fund_requests_by_state ON fund_requests (state, updated_at);

CREATE TABLE reservations (
    user_id BLOB NOT NULL CHECK (length(user_id) = 32),
    client_request_id BLOB NOT NULL,
    account_name TEXT NOT NULL,
    amount INTEGER NOT NULL CHECK (amount > 0),
    state TEXT NOT NULL CHECK (state IN ('held', 'released', 'consumed')),
    created_at INTEGER NOT NULL,
    released_at INTEGER,
    PRIMARY KEY (user_id, client_request_id)
);

CREATE TABLE fund_actions (
    action_id BLOB PRIMARY KEY NOT NULL CHECK (length(action_id) = 32),
    user_id BLOB NOT NULL CHECK (length(user_id) = 32),
    client_request_id BLOB,
    kind TEXT NOT NULL,
    signer_id TEXT NOT NULL,
    canonical_action BLOB NOT NULL,
    digest BLOB NOT NULL CHECK (length(digest) = 32),
    nonce INTEGER NOT NULL,
    signature BLOB,
    wire_payload BLOB,
    dispatch_state TEXT NOT NULL CHECK (dispatch_state IN ('queued', 'signing', 'signed', 'dispatching', 'reconciled', 'unknown', 'aborted')),
    worker_epoch INTEGER NOT NULL DEFAULT 0,
    lease_until INTEGER,
    attempt INTEGER NOT NULL DEFAULT 0,
    reason_code TEXT,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    CHECK (
        (dispatch_state IN ('dispatching', 'reconciled', 'unknown') AND signature IS NOT NULL AND wire_payload IS NOT NULL)
        OR (dispatch_state IN ('queued', 'signing', 'aborted') AND signature IS NULL)
        OR (dispatch_state = 'signed' AND signature IS NOT NULL AND wire_payload IS NOT NULL)
    )
);

CREATE INDEX fund_actions_by_state ON fund_actions (dispatch_state, updated_at);

CREATE TABLE master_nonces (
    signer_id TEXT PRIMARY KEY NOT NULL,
    last_nonce INTEGER NOT NULL
);

CREATE TABLE action_events (
    event_id INTEGER PRIMARY KEY AUTOINCREMENT,
    action_id BLOB NOT NULL,
    from_state TEXT,
    to_state TEXT NOT NULL,
    reason_code TEXT,
    at INTEGER NOT NULL
);

CREATE INDEX action_events_by_action ON action_events (action_id, event_id);
";

/// 外部イベント・HPKE鍵・監査。
const EXTERNAL: &str = "
CREATE TABLE external_events (
    event_id BLOB NOT NULL CHECK (length(event_id) = 32),
    network TEXT NOT NULL,
    account_address BLOB NOT NULL CHECK (length(account_address) = 20),
    counterparty BLOB NOT NULL CHECK (length(counterparty) = 20),
    asset TEXT NOT NULL CHECK (asset IN ('usdc')),
    amount INTEGER NOT NULL CHECK (amount > 0),
    kind TEXT NOT NULL CHECK (kind IN ('deposit', 'allocation', 'recovery', 'payout')),
    at INTEGER NOT NULL,
    evidence_ref TEXT,
    ingested_at INTEGER NOT NULL,
    PRIMARY KEY (network, event_id)
);

CREATE TABLE key_registry (
    key_id BLOB PRIMARY KEY NOT NULL,
    purpose TEXT NOT NULL CHECK (purpose IN ('hpke')),
    public_key BLOB NOT NULL,
    created_at INTEGER NOT NULL,
    expires_at INTEGER NOT NULL,
    retired_at INTEGER
);

CREATE INDEX key_registry_by_purpose ON key_registry (purpose, expires_at);

CREATE TABLE audit (
    audit_id INTEGER PRIMARY KEY AUTOINCREMENT,
    at INTEGER NOT NULL,
    actor TEXT NOT NULL,
    action TEXT NOT NULL,
    subject TEXT,
    reason_code TEXT
);
";

/// Agent世代（`Implementation.md` 7章）。承認はmaster署名で行い、状態をここに持つ。
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

/// HPKEの鍵世代（`Plan.md` 16.5）。秘密鍵はcanister外へ出さない。
const HPKE_KEYS: &str = "
CREATE TABLE hpke_keys (
    generation INTEGER PRIMARY KEY CHECK (generation > 0),
    secret BLOB NOT NULL CHECK (length(secret) = 32),
    public BLOB NOT NULL CHECK (length(public) = 32),
    created_at INTEGER NOT NULL,
    retired_at INTEGER
);
";

/// v7: 定期照合のカーソル、出金intentのnonce単回使用、仕訳の要求ID一意。
///
/// - `reconcile_cursor`: 入金先の定期照合を `(created_at, master_address)` のキーセットで
///   巡回する（先頭N件固定だと3人目以降が永久に対象外になる）。
/// - `used_intent_nonces`: 署名済み出金intentのnonceを単回使用にする（リプレイ防止）。
/// - `journal_requests`: 同一 `request_id` の**同じ種別**の仕訳二重計上をDBで拒否する
///   （`journals.request_id` は一意制約を持てないため別表で担保する。予約と解放のように
///   1つの要求に複数種別の仕訳が対応するため、要求IDだけでは一意にできない）。
const RECONCILE_AND_UNIQUENESS: &str = "
CREATE TABLE reconcile_cursor (
    singleton INTEGER PRIMARY KEY CHECK (singleton = 1),
    last_created_at INTEGER NOT NULL,
    last_address BLOB NOT NULL CHECK (length(last_address) = 20),
    updated_at INTEGER NOT NULL
);

CREATE TABLE used_intent_nonces (
    user_id BLOB NOT NULL CHECK (length(user_id) = 32),
    nonce INTEGER NOT NULL,
    client_request_id BLOB NOT NULL,
    used_at INTEGER NOT NULL,
    PRIMARY KEY (user_id, nonce)
);

CREATE TABLE journal_requests (
    request_id BLOB NOT NULL,
    kind TEXT NOT NULL,
    journal_id INTEGER NOT NULL,
    at INTEGER NOT NULL,
    PRIMARY KEY (request_id, kind)
);
";

/// `funds_vault` のMigration一覧。
pub const MIGRATIONS: &[Migration] = &[
    Migration {
        version: 1,
        sql: AUTH,
    },
    Migration {
        version: 2,
        sql: LEDGER,
    },
    Migration {
        version: 3,
        sql: FUNDS,
    },
    Migration {
        version: 4,
        sql: EXTERNAL,
    },
    Migration {
        version: 5,
        sql: AGENTS,
    },
    Migration {
        version: 6,
        sql: HPKE_KEYS,
    },
    Migration {
        version: 7,
        sql: RECONCILE_AND_UNIQUENESS,
    },
];
