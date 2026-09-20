//! EOA認証（challengeとセッション）。`Implementation.md` 14.1、`Plan.md` 16.1。
//!
//! 行の読み出しはSQLiteの型（`i64`・`Vec<u8>`・`String`）で受け取り、ドメイン型への
//! 変換はクエリの外で行う（`FromColumn` は `u64` を持たないため）。

use crate::error::Error;
use crate::repo::{amount_u64, sql};
use ic_sqlite_vfs::db::UpdateConnection;
use ic_sqlite_vfs::db::connection::Connection;
use ic_sqlite_vfs::params;

/// EOAとユーザーの対応。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Identity {
    pub user_id: [u8; 32],
    pub eoa_address: [u8; 20],
    pub status: String,
    pub revocation_generation: u64,
}

/// challengeの記録。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChallengeRow {
    pub challenge_id: [u8; 32],
    pub nonce: [u8; 32],
    pub eoa_address: [u8; 20],
    pub principal: Vec<u8>,
    pub purpose: String,
    pub network: String,
    pub origin: String,
    pub expires_at: u64,
    pub consumed_at: Option<u64>,
}

/// セッションの記録。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionRow {
    pub session_id: [u8; 32],
    pub user_id: [u8; 32],
    pub principal: Vec<u8>,
    pub expires_at: u64,
    pub revocation_generation: u64,
    pub revoked_at: Option<u64>,
}

type RawIdentity = (Vec<u8>, Vec<u8>, String, i64);
type RawChallenge = (
    Vec<u8>,
    Vec<u8>,
    Vec<u8>,
    Vec<u8>,
    String,
    String,
    String,
    i64,
    Option<i64>,
);
type RawSession = (Vec<u8>, Vec<u8>, Vec<u8>, i64, i64, Option<i64>);

fn convert_identity(raw: RawIdentity) -> Result<Identity, Error> {
    Ok(Identity {
        user_id: raw
            .0
            .try_into()
            .map_err(|_| Error::Invariant("expected a 32-byte user id"))?,
        eoa_address: raw
            .1
            .try_into()
            .map_err(|_| Error::Invariant("expected a 20-byte address"))?,
        status: raw.2,
        revocation_generation: amount_u64(raw.3, "negative revocation generation")?,
    })
}

fn convert_challenge(raw: RawChallenge) -> Result<ChallengeRow, Error> {
    Ok(ChallengeRow {
        challenge_id: raw
            .0
            .try_into()
            .map_err(|_| Error::Invariant("expected a 32-byte challenge id"))?,
        nonce: raw
            .1
            .try_into()
            .map_err(|_| Error::Invariant("expected a 32-byte nonce"))?,
        eoa_address: raw
            .2
            .try_into()
            .map_err(|_| Error::Invariant("expected a 20-byte address"))?,
        principal: raw.3,
        purpose: raw.4,
        network: raw.5,
        origin: raw.6,
        expires_at: amount_u64(raw.7, "negative expiry")?,
        consumed_at: raw
            .8
            .map(|value| amount_u64(value, "negative timestamp"))
            .transpose()?,
    })
}

fn convert_session(raw: RawSession) -> Result<SessionRow, Error> {
    Ok(SessionRow {
        session_id: raw
            .0
            .try_into()
            .map_err(|_| Error::Invariant("expected a 32-byte session id"))?,
        user_id: raw
            .1
            .try_into()
            .map_err(|_| Error::Invariant("expected a 32-byte user id"))?,
        principal: raw.2,
        expires_at: amount_u64(raw.3, "negative expiry")?,
        revocation_generation: amount_u64(raw.4, "negative revocation generation")?,
        revoked_at: raw
            .5
            .map(|value| amount_u64(value, "negative timestamp"))
            .transpose()?,
    })
}

/// EOAから本人を引く。
pub fn find_identity_by_eoa(
    connection: &Connection,
    eoa_address: &[u8; 20],
) -> Result<Option<Identity>, Error> {
    let raw = connection
        .query_optional(
            "SELECT user_id, eoa_address, status, revocation_generation
               FROM identities WHERE eoa_address = ?1",
            params![eoa_address.as_slice()],
            |row| {
                Ok((
                    row.get::<Vec<u8>>(0)?,
                    row.get::<Vec<u8>>(1)?,
                    row.get::<String>(2)?,
                    row.get::<i64>(3)?,
                ))
            },
        )
        .map_err(sql)?;
    raw.map(convert_identity).transpose()
}

/// EOAとユーザーを新規登録する。既に同じEOAがあれば既存を返す。
pub fn ensure_identity(
    connection: &mut UpdateConnection<'_>,
    eoa_address: &[u8; 20],
    user_id: &[u8; 32],
    now: u64,
) -> Result<Identity, Error> {
    if let Some(identity) = find_identity_by_eoa(connection, eoa_address)? {
        return Ok(identity);
    }
    connection
        .execute(
            "INSERT INTO identities (eoa_address, user_id, status, revocation_generation, created_at, last_login_at)
             VALUES (?1, ?2, 'active', 0, ?3, NULL)",
            params![eoa_address.as_slice(), user_id.as_slice(), now as i64],
        )
        .map_err(sql)?;
    find_identity_by_eoa(connection, eoa_address)?.ok_or(Error::NotFound)
}

/// 現在の失効世代。
pub fn revocation_generation(connection: &Connection, user_id: &[u8; 32]) -> Result<u64, Error> {
    let value = connection
        .query_optional_scalar::<i64>(
            "SELECT revocation_generation FROM identities WHERE user_id = ?1",
            params![user_id.as_slice()],
        )
        .map_err(sql)?
        .ok_or(Error::NotFound)?;
    amount_u64(value, "negative revocation generation")
}

/// 失効世代を進める（既存セッションを失効させる）。
pub fn bump_revocation_generation(
    connection: &mut UpdateConnection<'_>,
    user_id: &[u8; 32],
) -> Result<u64, Error> {
    connection
        .execute(
            "UPDATE identities SET revocation_generation = revocation_generation + 1 WHERE user_id = ?1",
            params![user_id.as_slice()],
        )
        .map_err(sql)?;
    revocation_generation(connection, user_id)
}

/// 最終ログイン時刻を記録する。
pub fn touch_login(
    connection: &mut UpdateConnection<'_>,
    user_id: &[u8; 32],
    now: u64,
) -> Result<(), Error> {
    connection
        .execute(
            "UPDATE identities SET last_login_at = ?2 WHERE user_id = ?1",
            params![user_id.as_slice(), now as i64],
        )
        .map_err(sql)
}

/// challengeを保存する。nonceは一回性（UNIQUE）。
pub fn insert_challenge(
    connection: &mut UpdateConnection<'_>,
    challenge: &ChallengeRow,
    issued_at: u64,
) -> Result<(), Error> {
    connection
        .execute(
            "INSERT INTO challenges
               (challenge_id, nonce, eoa_address, principal, purpose, network, origin, issued_at, expires_at, consumed_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, NULL)",
            params![
                challenge.challenge_id.as_slice(),
                challenge.nonce.as_slice(),
                challenge.eoa_address.as_slice(),
                challenge.principal.as_slice(),
                challenge.purpose.as_str(),
                challenge.network.as_str(),
                challenge.origin.as_str(),
                issued_at as i64,
                challenge.expires_at as i64,
            ],
        )
        .map_err(sql)
}

/// challengeを一度だけ消費する。期限切れ・再使用は拒否する。
pub fn consume_challenge(
    connection: &mut UpdateConnection<'_>,
    challenge_id: &[u8; 32],
    now: u64,
) -> Result<ChallengeRow, Error> {
    let raw = connection
        .query_optional(
            "SELECT challenge_id, nonce, eoa_address, principal, purpose, network, origin, expires_at, consumed_at
               FROM challenges WHERE challenge_id = ?1",
            params![challenge_id.as_slice()],
            |row| {
                Ok((
                    row.get::<Vec<u8>>(0)?,
                    row.get::<Vec<u8>>(1)?,
                    row.get::<Vec<u8>>(2)?,
                    row.get::<Vec<u8>>(3)?,
                    row.get::<String>(4)?,
                    row.get::<String>(5)?,
                    row.get::<String>(6)?,
                    row.get::<i64>(7)?,
                    row.get::<Option<i64>>(8)?,
                ))
            },
        )
        .map_err(sql)?
        .ok_or(Error::NotFound)?;
    let challenge = convert_challenge(raw)?;

    if challenge.consumed_at.is_some() {
        return Err(Error::Conflict);
    }
    if now > challenge.expires_at {
        return Err(Error::Invariant("challenge expired"));
    }

    connection
        .execute(
            "UPDATE challenges SET consumed_at = ?2 WHERE challenge_id = ?1 AND consumed_at IS NULL",
            params![challenge_id.as_slice(), now as i64],
        )
        .map_err(sql)?;

    let changed = crate::cas::changes(connection)?;
    crate::cas::ensure_changed(changed, "unconsumed challenge", "consumed challenge")?;
    Ok(challenge)
}

/// セッションを作る。
pub fn create_session(
    connection: &mut UpdateConnection<'_>,
    session_id: &[u8; 32],
    user_id: &[u8; 32],
    principal: &[u8],
    now: u64,
    expires_at: u64,
    revocation_generation: u64,
) -> Result<(), Error> {
    connection
        .execute(
            "INSERT INTO sessions
               (session_id, user_id, principal, issued_at, expires_at, revocation_generation, revoked_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, NULL)",
            params![
                session_id.as_slice(),
                user_id.as_slice(),
                principal,
                now as i64,
                expires_at as i64,
                revocation_generation as i64,
            ],
        )
        .map_err(sql)
}

/// 有効なセッションを返す。期限切れ・失効・世代不一致は `None`。
pub fn find_valid_session(
    connection: &Connection,
    session_id: &[u8; 32],
    now: u64,
) -> Result<Option<SessionRow>, Error> {
    let raw = connection
        .query_optional(
            "SELECT s.session_id, s.user_id, s.principal, s.expires_at, s.revocation_generation, s.revoked_at
               FROM sessions s
               JOIN identities i ON i.user_id = s.user_id
              WHERE s.session_id = ?1",
            params![session_id.as_slice()],
            |row| {
                Ok((
                    row.get::<Vec<u8>>(0)?,
                    row.get::<Vec<u8>>(1)?,
                    row.get::<Vec<u8>>(2)?,
                    row.get::<i64>(3)?,
                    row.get::<i64>(4)?,
                    row.get::<Option<i64>>(5)?,
                ))
            },
        )
        .map_err(sql)?;

    let Some(raw) = raw else {
        return Ok(None);
    };
    let session = convert_session(raw)?;
    if session.revoked_at.is_some() || now > session.expires_at {
        return Ok(None);
    }
    let current = revocation_generation(connection, &session.user_id)?;
    if session.revocation_generation != current {
        return Ok(None);
    }
    Ok(Some(session))
}

/// セッションを失効させる。
pub fn revoke_session(
    connection: &mut UpdateConnection<'_>,
    session_id: &[u8; 32],
    now: u64,
) -> Result<(), Error> {
    connection
        .execute(
            "UPDATE sessions SET revoked_at = ?2 WHERE session_id = ?1 AND revoked_at IS NULL",
            params![session_id.as_slice(), now as i64],
        )
        .map_err(sql)?;
    Ok(())
}

/// 発行済みセッションの失効時刻を記録する（ログアウト時の一括失効）。
pub fn revoke_all_sessions(
    connection: &mut UpdateConnection<'_>,
    user_id: &[u8; 32],
    now: u64,
) -> Result<u64, Error> {
    connection
        .execute(
            "UPDATE sessions SET revoked_at = ?2 WHERE user_id = ?1 AND revoked_at IS NULL",
            params![user_id.as_slice(), now as i64],
        )
        .map_err(sql)?;
    let changed = crate::cas::changes(connection)?;
    Ok(u64::try_from(changed).unwrap_or(0))
}

/// 指定時刻以降に発行したchallengeの件数（発行レートの上限に使う）。
pub fn count_recent_challenges(
    connection: &Connection,
    eoa_address: &[u8; 20],
    since: u64,
) -> Result<u64, Error> {
    let count = connection
        .query_scalar::<i64>(
            "SELECT COUNT(*) FROM challenges WHERE eoa_address = ?1 AND issued_at >= ?2",
            params![eoa_address.as_slice(), since as i64],
        )
        .map_err(sql)?;
    amount_u64(count, "negative challenge count")
}

/// セッション行を（有効性を問わず）返す。失効・期限切れの区別に使う。
pub fn session_row(
    connection: &Connection,
    session_id: &[u8; 32],
) -> Result<Option<SessionRow>, Error> {
    let raw = connection
        .query_optional(
            "SELECT session_id, user_id, principal, expires_at, revocation_generation, revoked_at
               FROM sessions WHERE session_id = ?1",
            params![session_id.as_slice()],
            |row| {
                Ok((
                    row.get::<Vec<u8>>(0)?,
                    row.get::<Vec<u8>>(1)?,
                    row.get::<Vec<u8>>(2)?,
                    row.get::<i64>(3)?,
                    row.get::<i64>(4)?,
                    row.get::<Option<i64>>(5)?,
                ))
            },
        )
        .map_err(sql)?;
    raw.map(convert_session).transpose()
}
