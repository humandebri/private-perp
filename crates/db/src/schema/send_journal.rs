use crate::Migration;

const INITIAL: &str = "
CREATE TABLE journal_workers (
  role TEXT PRIMARY KEY CHECK(role IN ('vault','core')),
  principal BLOB NOT NULL UNIQUE CHECK(length(principal) BETWEEN 1 AND 29)
);
CREATE TABLE journal_heads (
  worker BLOB PRIMARY KEY,
  sequence INTEGER NOT NULL CHECK(sequence >= 0),
  hash BLOB NOT NULL CHECK(length(hash) = 32)
);
CREATE TABLE journal_records (
  worker BLOB NOT NULL,
  sequence INTEGER NOT NULL CHECK(sequence > 0),
  kind TEXT NOT NULL,
  request_id BLOB NOT NULL CHECK(length(request_id) = 32),
  account_id BLOB NOT NULL CHECK(length(account_id) = 32),
  nonce INTEGER NOT NULL CHECK(nonce > 0),
  digest BLOB NOT NULL CHECK(length(digest) = 32),
  previous_hash BLOB NOT NULL CHECK(length(previous_hash) = 32),
  hash BLOB NOT NULL CHECK(length(hash) = 32),
  PRIMARY KEY(worker, sequence),
  UNIQUE(worker, kind, request_id)
);
";

const RECOVERY_EVENTS: &str = "
CREATE TABLE recovery_event_heads (
  worker BLOB PRIMARY KEY,
  sequence INTEGER NOT NULL CHECK(sequence >= 0),
  hash BLOB NOT NULL CHECK(length(hash) = 32)
);
CREATE TABLE recovery_event_records (
  worker BLOB NOT NULL,
  sequence INTEGER NOT NULL CHECK(sequence > 0),
  logical_id BLOB NOT NULL CHECK(length(logical_id) = 32),
  version INTEGER NOT NULL CHECK(version = 1),
  payload BLOB NOT NULL CHECK(length(payload) BETWEEN 1 AND 4096),
  previous_hash BLOB NOT NULL CHECK(length(previous_hash) = 32),
  hash BLOB NOT NULL CHECK(length(hash) = 32),
  PRIMARY KEY(worker, sequence),
  UNIQUE(worker, logical_id)
);
";

pub const MIGRATIONS: &[Migration] = &[
    Migration {
        version: 1,
        sql: INITIAL,
    },
    Migration {
        version: 2,
        sql: RECOVERY_EVENTS,
    },
];

/// vault/coreの双方へ適用する受領証跡。連番欠落は起動時の送信停止条件。
pub const CLIENT_SQL: &str = "
CREATE TABLE send_journal_client (
  singleton INTEGER PRIMARY KEY CHECK(singleton = 1),
  principal BLOB NOT NULL CHECK(length(principal) BETWEEN 1 AND 29),
  locked INTEGER NOT NULL DEFAULT 1 CHECK(locked IN (0, 1))
);
CREATE TABLE send_journal_receipts (
  sequence INTEGER PRIMARY KEY CHECK(sequence > 0),
  kind TEXT NOT NULL,
  request_id BLOB NOT NULL CHECK(length(request_id) = 32),
  account_id BLOB NOT NULL CHECK(length(account_id) = 32),
  nonce INTEGER NOT NULL CHECK(nonce > 0),
  digest BLOB NOT NULL CHECK(length(digest) = 32),
  hash BLOB NOT NULL CHECK(length(hash) = 32),
  UNIQUE(kind, request_id)
);
";

/// 復元時に独立ジャーナルから取り込んだ差分。受領記録とは分離し、照合前に送信権を与えない。
pub const STAGE_SQL: &str = "
CREATE TABLE send_journal_stage (
  sequence INTEGER PRIMARY KEY CHECK(sequence > 0),
  kind TEXT NOT NULL,
  request_id BLOB NOT NULL CHECK(length(request_id) = 32),
  account_id BLOB NOT NULL CHECK(length(account_id) = 32),
  nonce INTEGER NOT NULL CHECK(nonce > 0),
  digest BLOB NOT NULL CHECK(length(digest) = 32),
  previous_hash BLOB NOT NULL CHECK(length(previous_hash) = 32),
  hash BLOB NOT NULL CHECK(length(hash) = 32)
);
";

/// V2 events recovered from the independent journal. Staging never authorizes sends.
pub const RECOVERY_STAGE_SQL: &str = "
CREATE TABLE recovery_event_stage (
  sequence INTEGER PRIMARY KEY CHECK(sequence > 0),
  logical_id BLOB NOT NULL UNIQUE CHECK(length(logical_id) = 32),
  version INTEGER NOT NULL CHECK(version = 1),
  payload BLOB NOT NULL CHECK(length(payload) BETWEEN 1 AND 4096),
  previous_hash BLOB NOT NULL CHECK(length(previous_hash) = 32),
  hash BLOB NOT NULL CHECK(length(hash) = 32)
);
";

/// Only one journal append may be in flight per worker. The epoch rejects a
/// callback that arrives after guard reconciliation released an old writer.
pub const CLIENT_WRITER_FENCE_SQL: &str = "
ALTER TABLE send_journal_client ADD COLUMN writer_epoch INTEGER NOT NULL DEFAULT 0 CHECK(writer_epoch >= 0);
ALTER TABLE send_journal_client ADD COLUMN writer_kind TEXT;
ALTER TABLE send_journal_client ADD COLUMN writer_request_id BLOB CHECK(writer_request_id IS NULL OR length(writer_request_id) = 32);
";

/// V2 business-event acknowledgements committed with the corresponding local
/// business update. These are distinct from staged, unapplied restore records.
pub const RECOVERY_RECEIPTS_SQL: &str = "
CREATE TABLE recovery_event_receipts (
  sequence INTEGER PRIMARY KEY CHECK(sequence > 0),
  logical_id BLOB NOT NULL UNIQUE CHECK(length(logical_id) = 32),
  version INTEGER NOT NULL CHECK(version = 1),
  payload BLOB NOT NULL CHECK(length(payload) BETWEEN 1 AND 4096),
  hash BLOB NOT NULL CHECK(length(hash) = 32)
);
";

/// A staged business replay is not a complete old-backup recovery until the
/// baseline and all non-journaled state have been independently reconciled.
pub const REPLAY_VALIDATION_SQL: &str = "
ALTER TABLE send_journal_client ADD COLUMN replay_pending_validation INTEGER NOT NULL DEFAULT 0
  CHECK(replay_pending_validation IN (0, 1));
";

pub const CYCLES_SQL: &str = "
CREATE TABLE cycles_config (
  singleton INTEGER PRIMARY KEY CHECK(singleton = 1),
  daily_floor TEXT NOT NULL,
  exit_reserve TEXT NOT NULL
);
CREATE TABLE cycles_samples (
  hour_bucket INTEGER PRIMARY KEY CHECK(hour_bucket >= 0),
  balance TEXT NOT NULL,
  observed_at INTEGER NOT NULL
);
";
