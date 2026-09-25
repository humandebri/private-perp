use crate::error::Error;
use crate::repo::sql;
use ic_sqlite_vfs::db::{UpdateConnection, connection::Connection};
use ic_sqlite_vfs::params;

pub struct Config {
    pub terms_version: u64,
    pub issuer_address: [u8; 20],
    pub mock_issuer: bool,
}

pub struct TokenRow {
    pub principal: Vec<u8>,
    pub account_id: [u8; 32],
    pub network: String,
    pub terms_version: u64,
    pub expires_at: u64,
}

pub fn config(c: &Connection) -> Result<Option<Config>, Error> {
    let row = c.query_optional(
        "SELECT terms_version, issuer_address, mock_issuer FROM eligibility_config WHERE singleton = 1",
        params![],
        |r| Ok((r.get::<i64>(0)?, r.get::<Vec<u8>>(1)?, r.get::<i64>(2)?)),
    ).map_err(sql)?;
    row.map(|(version, address, mock)| {
        Ok(Config {
            terms_version: u64::try_from(version).map_err(|_| Error::Overflow)?,
            issuer_address: address
                .try_into()
                .map_err(|_| Error::Invariant("bad issuer"))?,
            mock_issuer: mock != 0,
        })
    })
    .transpose()
}

pub fn configure(
    c: &mut UpdateConnection<'_>,
    version: u64,
    address: &[u8; 20],
    mock: bool,
) -> Result<(), Error> {
    c.execute(
        "INSERT INTO eligibility_config(singleton, terms_version, issuer_address, mock_issuer)
         VALUES(1, ?1, ?2, ?3)
         ON CONFLICT(singleton) DO UPDATE SET terms_version = excluded.terms_version,
           issuer_address = excluded.issuer_address, mock_issuer = excluded.mock_issuer",
        params![
            i64::try_from(version).map_err(|_| Error::Overflow)?,
            address.as_slice(),
            i64::from(mock)
        ],
    )
    .map_err(sql)
}

pub fn token(c: &Connection, user_id: &[u8; 32]) -> Result<Option<TokenRow>, Error> {
    let row = c
        .query_optional(
            "SELECT principal, account_id, network, terms_version, expires_at
         FROM eligibility_tokens WHERE user_id = ?1",
            params![user_id.as_slice()],
            |r| {
                Ok((
                    r.get::<Vec<u8>>(0)?,
                    r.get::<Vec<u8>>(1)?,
                    r.get::<String>(2)?,
                    r.get::<i64>(3)?,
                    r.get::<i64>(4)?,
                ))
            },
        )
        .map_err(sql)?;
    row.map(|(principal, account, network, version, expires)| {
        Ok(TokenRow {
            principal,
            account_id: account
                .try_into()
                .map_err(|_| Error::Invariant("bad token account"))?,
            network,
            terms_version: u64::try_from(version).map_err(|_| Error::Overflow)?,
            expires_at: u64::try_from(expires).map_err(|_| Error::Overflow)?,
        })
    })
    .transpose()
}

#[allow(clippy::too_many_arguments)]
pub fn register(
    c: &mut UpdateConnection<'_>,
    user_id: &[u8; 32],
    principal: &[u8],
    account_id: &[u8; 32],
    network: &str,
    version: u64,
    issued_at: u64,
    expires_at: u64,
    nonce: &[u8; 32],
    digest: &[u8; 32],
) -> Result<(), Error> {
    let existing_nonce = c
        .query_optional_scalar::<Vec<u8>>(
            "SELECT digest FROM eligibility_nonces WHERE nonce = ?1",
            params![nonce.as_slice()],
        )
        .map_err(sql)?;
    if let Some(old_digest) = existing_nonce {
        let current_digest = c
            .query_optional_scalar::<Vec<u8>>(
                "SELECT digest FROM eligibility_tokens WHERE user_id = ?1 AND nonce = ?2",
                params![user_id.as_slice(), nonce.as_slice()],
            )
            .map_err(sql)?;
        return if old_digest == digest && current_digest.as_deref() == Some(digest.as_slice()) {
            Ok(())
        } else {
            Err(Error::Conflict)
        };
    }
    c.execute(
        "INSERT INTO eligibility_nonces(nonce, digest) VALUES(?1, ?2)",
        params![nonce.as_slice(), digest.as_slice()],
    )
    .map_err(sql)?;
    c.execute(
        "INSERT INTO eligibility_tokens(user_id, principal, account_id, network, terms_version, issued_at, expires_at, nonce, digest)
         VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
         ON CONFLICT(user_id) DO UPDATE SET principal=excluded.principal, account_id=excluded.account_id,
           network=excluded.network, terms_version=excluded.terms_version,
           issued_at=excluded.issued_at, expires_at=excluded.expires_at,
           nonce=excluded.nonce, digest=excluded.digest
         WHERE excluded.issued_at >= eligibility_tokens.issued_at",
        params![user_id.as_slice(), principal, account_id.as_slice(), network,
            i64::try_from(version).map_err(|_| Error::Overflow)?,
            i64::try_from(issued_at).map_err(|_| Error::Overflow)?,
            i64::try_from(expires_at).map_err(|_| Error::Overflow)?, nonce.as_slice(), digest.as_slice()],
    ).map_err(sql)?;
    if crate::cas::changes(c)? != 1 {
        return Err(Error::Conflict);
    }
    Ok(())
}
