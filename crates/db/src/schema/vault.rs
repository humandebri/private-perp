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
    user_id BLOB REFERENCES identities (user_id),
    kind TEXT NOT NULL CHECK (kind IN ('reserve', 'trading')),
    derivation_path TEXT NOT NULL UNIQUE,
    master_address BLOB NOT NULL CHECK (length(master_address) = 20),
    network TEXT NOT NULL,
    state TEXT NOT NULL CHECK (state IN ('pending', 'active', 'stopped')),
    created_at INTEGER NOT NULL,
    CHECK ((kind = 'reserve' AND user_id IS NULL) OR (kind = 'trading' AND user_id IS NOT NULL))
);

CREATE UNIQUE INDEX custody_accounts_by_user ON custody_accounts (user_id, kind);
CREATE UNIQUE INDEX custody_shared_reserve ON custody_accounts (kind) WHERE kind = 'reserve';
CREATE UNIQUE INDEX custody_addresses ON custody_accounts (master_address);

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
CREATE TABLE deposit_history_cursors (
    network TEXT NOT NULL,
    address BLOB NOT NULL CHECK (length(address) = 20),
    start_time INTEGER NOT NULL CHECK (start_time >= 0),
    PRIMARY KEY (network, address)
);

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

/// v8: 環境設定（network・HL endpoint・tECDSA key ID）。
///
/// 環境は「network」「鍵」「endpoint」で分離し、起動時に検証可能な値で判定する
/// （`docs/phase-0/environments.md` 4節）。未設定はnetwork既定（local）を使う。
const ENVIRONMENT: &str = "
CREATE TABLE vault_config (
    singleton INTEGER PRIMARY KEY CHECK (singleton = 1),
    network TEXT,
    exchange_url TEXT,
    info_url TEXT,
    ecdsa_key_id TEXT,
    updated_at INTEGER
);
";

/// v9: 未配線の旧鍵registryを空であることを確認して削除する。
const REMOVE_LEGACY_KEY_REGISTRY: &str = "
CREATE TABLE vault_legacy_cleanup_guard (
    ok INTEGER NOT NULL CHECK (ok = 1)
);
INSERT INTO vault_legacy_cleanup_guard (ok)
SELECT CASE WHEN (SELECT COUNT(*) FROM key_registry) = 0 THEN 1 ELSE 0 END;
DROP TABLE key_registry;
DROP TABLE vault_legacy_cleanup_guard;
";

/// v10: allocation着金を要求単位へ対応付ける。
///
/// Hyperliquidのledger updateにはclient request IDが含まれないため、同一取引口座の
/// 実行中要求へ作成順で充当する。1イベントが複数要求を満たす場合と、1要求が複数の
/// 部分着金で満たされる場合の両方を記録する。
const ALLOCATION_CONFIRMATIONS: &str = "
CREATE TABLE allocation_confirmations (
    external_event_id BLOB NOT NULL CHECK (length(external_event_id) = 32),
    user_id BLOB NOT NULL CHECK (length(user_id) = 32),
    client_request_id BLOB NOT NULL,
    amount INTEGER NOT NULL CHECK (amount > 0),
    confirmed_at INTEGER NOT NULL,
    PRIMARY KEY (external_event_id, user_id, client_request_id),
    FOREIGN KEY (user_id, client_request_id)
        REFERENCES fund_requests (user_id, client_request_id)
);

CREATE INDEX allocation_confirmations_by_request
    ON allocation_confirmations (user_id, client_request_id);
";

/// v11: policyをHL REST予算の単一の調整者として参照する。
const REST_BUDGET_POLICY: &str = "
ALTER TABLE vault_config ADD COLUMN policy_principal BLOB;
";

/// v12: 回収フェンスのcore設定と永続的な世代・照合位置。
const RECOVERY_FENCES: &str = "
ALTER TABLE vault_config ADD COLUMN core_principal BLOB;
ALTER TABLE fund_actions ADD COLUMN recovery_fence_epoch INTEGER;
ALTER TABLE fund_actions ADD COLUMN recovery_checked_until INTEGER;
ALTER TABLE fund_actions ADD COLUMN recovery_window_ms INTEGER NOT NULL DEFAULT 3600000;
ALTER TABLE fund_actions ADD COLUMN recovery_match_hash BLOB;
ALTER TABLE fund_actions ADD COLUMN recovery_ambiguous INTEGER NOT NULL DEFAULT 0;
ALTER TABLE fund_actions ADD COLUMN recovery_fence_released_at INTEGER;
";

/// v13: testnetで履歴の完全性を確認するまでは未実行の自動判定を禁止する。
const RECOVERY_HISTORY_GATE: &str = "
ALTER TABLE vault_config ADD COLUMN recovery_history_verified INTEGER NOT NULL DEFAULT 0
    CHECK (recovery_history_verified IN (0, 1));
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
    Migration {
        version: 8,
        sql: ENVIRONMENT,
    },
    Migration {
        version: 9,
        sql: REMOVE_LEGACY_KEY_REGISTRY,
    },
    Migration {
        version: 10,
        sql: ALLOCATION_CONFIRMATIONS,
    },
    Migration {
        version: 11,
        sql: REST_BUDGET_POLICY,
    },
    Migration {
        version: 12,
        sql: RECOVERY_FENCES,
    },
    Migration {
        version: 13,
        sql: RECOVERY_HISTORY_GATE,
    },
    Migration {
        version: 14,
        sql: super::send_journal::CLIENT_SQL,
    },
    Migration {
        version: 15,
        sql: "CREATE TABLE hpke_requests (
            request_id BLOB PRIMARY KEY NOT NULL CHECK(length(request_id) = 32),
            method TEXT NOT NULL, caller BLOB NOT NULL,
            received_at INTEGER NOT NULL, expires_at INTEGER NOT NULL);
            CREATE INDEX hpke_requests_by_expiry ON hpke_requests(expires_at);",
    },
    Migration {
        version: 16,
        sql: "ALTER TABLE send_journal_client ADD COLUMN guard BLOB;",
    },
    Migration {
        version: 17,
        sql: super::send_journal::STAGE_SQL,
    },
    Migration {
        version: 18,
        sql: "CREATE TABLE eligibility_config (
            singleton INTEGER PRIMARY KEY CHECK(singleton = 1),
            terms_version INTEGER NOT NULL CHECK(terms_version > 0),
            issuer_address BLOB NOT NULL CHECK(length(issuer_address) = 20),
            mock_issuer INTEGER NOT NULL CHECK(mock_issuer IN (0, 1))
        );
        CREATE TABLE eligibility_tokens (
            user_id BLOB PRIMARY KEY CHECK(length(user_id) = 32),
            principal BLOB NOT NULL,
            account_id BLOB NOT NULL CHECK(length(account_id) = 32),
            network TEXT NOT NULL,
            terms_version INTEGER NOT NULL,
            issued_at INTEGER NOT NULL,
            expires_at INTEGER NOT NULL,
            nonce BLOB NOT NULL UNIQUE CHECK(length(nonce) = 32),
            digest BLOB NOT NULL CHECK(length(digest) = 32)
        );",
    },
    Migration {
        version: 19,
        sql: super::send_journal::CYCLES_SQL,
    },
    Migration {
        version: 20,
        sql:
            "CREATE TABLE eligibility_nonces (
            nonce BLOB PRIMARY KEY CHECK(length(nonce) = 32),
            digest BLOB NOT NULL CHECK(length(digest) = 32)
        );
        INSERT INTO eligibility_nonces(nonce, digest) SELECT nonce, digest FROM eligibility_tokens;",
    },
    Migration {
        version: 21,
        sql: "CREATE TABLE builder_fee_mock_consents (
            user_id BLOB PRIMARY KEY CHECK(length(user_id) = 32),
            principal BLOB NOT NULL,
            account_id BLOB NOT NULL CHECK(length(account_id) = 32),
            builder_address BLOB NOT NULL CHECK(length(builder_address) = 20),
            network TEXT NOT NULL,
            issued_at INTEGER NOT NULL,
            expires_at INTEGER NOT NULL,
            nonce BLOB NOT NULL UNIQUE CHECK(length(nonce) = 32),
            digest BLOB NOT NULL CHECK(length(digest) = 32),
            eoa_signature BLOB NOT NULL CHECK(length(eoa_signature) = 65),
            approved_at INTEGER NOT NULL,
            fee_decibps INTEGER NOT NULL DEFAULT 0 CHECK(fee_decibps = 0)
        );
        CREATE TABLE builder_fee_mock_nonces (
            nonce BLOB PRIMARY KEY CHECK(length(nonce) = 32),
            digest BLOB NOT NULL CHECK(length(digest) = 32)
        );
        CREATE TABLE builder_fee_accounting (
            event_id BLOB PRIMARY KEY CHECK(length(event_id) = 32),
            user_id BLOB NOT NULL CHECK(length(user_id) = 32),
            account_id BLOB NOT NULL CHECK(length(account_id) = 32),
            event_kind TEXT NOT NULL CHECK(event_kind = 'mock_approval'),
            amount_micros INTEGER NOT NULL CHECK(amount_micros = 0),
            recorded_at INTEGER NOT NULL
        );",
    },
    Migration {
        version: 22,
        sql: super::send_journal::RECOVERY_STAGE_SQL,
    },
    Migration {
        version: 23,
        sql: super::send_journal::CLIENT_WRITER_FENCE_SQL,
    },
    Migration {
        version: 24,
        sql: super::send_journal::RECOVERY_RECEIPTS_SQL,
    },
    Migration {
        version: 25,
        sql: super::send_journal::REPLAY_VALIDATION_SQL,
    },
];
