use crate::error::Error;
use crate::repo::sql;
use ic_sqlite_vfs::db::{UpdateConnection, connection::Connection};
use ic_sqlite_vfs::params;

pub struct ConsentRow {
    pub principal: Vec<u8>,
    pub account_id: [u8; 32],
    pub builder_address: [u8; 20],
    pub network: String,
    pub expires_at: u64,
}

pub fn consent(c: &Connection, user_id: &[u8; 32]) -> Result<Option<ConsentRow>, Error> {
    let row = c
        .query_optional(
            "SELECT principal, account_id, builder_address, network, expires_at
         FROM builder_fee_mock_consents WHERE user_id = ?1",
            params![user_id.as_slice()],
            |r| {
                Ok((
                    r.get::<Vec<u8>>(0)?,
                    r.get::<Vec<u8>>(1)?,
                    r.get::<Vec<u8>>(2)?,
                    r.get::<String>(3)?,
                    r.get::<i64>(4)?,
                ))
            },
        )
        .map_err(sql)?;
    row.map(
        |(principal, account_id, builder_address, network, expires_at)| {
            Ok(ConsentRow {
                principal,
                account_id: account_id
                    .try_into()
                    .map_err(|_| Error::Invariant("bad mock fee account"))?,
                builder_address: builder_address
                    .try_into()
                    .map_err(|_| Error::Invariant("bad mock builder"))?,
                network,
                expires_at: u64::try_from(expires_at).map_err(|_| Error::Overflow)?,
            })
        },
    )
    .transpose()
}

#[allow(clippy::too_many_arguments)]
pub fn register(
    c: &mut UpdateConnection<'_>,
    user_id: &[u8; 32],
    principal: &[u8],
    account_id: &[u8; 32],
    builder_address: &[u8; 20],
    network: &str,
    issued_at: u64,
    expires_at: u64,
    nonce: &[u8; 32],
    digest: &[u8; 32],
    signature: &[u8; 65],
    now: u64,
) -> Result<(), Error> {
    let old_digest = c
        .query_optional_scalar::<Vec<u8>>(
            "SELECT digest FROM builder_fee_mock_nonces WHERE nonce = ?1",
            params![nonce.as_slice()],
        )
        .map_err(sql)?;
    if let Some(old_digest) = old_digest {
        let current_digest = c
            .query_optional_scalar::<Vec<u8>>(
                "SELECT digest FROM builder_fee_mock_consents WHERE user_id = ?1 AND nonce = ?2",
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
        "INSERT INTO builder_fee_mock_nonces(nonce, digest) VALUES(?1, ?2)",
        params![nonce.as_slice(), digest.as_slice()],
    )
    .map_err(sql)?;
    c.execute(
        "INSERT INTO builder_fee_mock_consents(user_id, principal, account_id, builder_address,
            network, issued_at, expires_at, nonce, digest, eoa_signature, approved_at, fee_decibps)
         VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, 0)
         ON CONFLICT(user_id) DO UPDATE SET principal=excluded.principal,
            account_id=excluded.account_id, builder_address=excluded.builder_address,
            network=excluded.network, issued_at=excluded.issued_at,
            expires_at=excluded.expires_at, nonce=excluded.nonce, digest=excluded.digest,
            eoa_signature=excluded.eoa_signature, approved_at=excluded.approved_at
         WHERE excluded.issued_at >= builder_fee_mock_consents.issued_at",
        params![
            user_id.as_slice(),
            principal,
            account_id.as_slice(),
            builder_address.as_slice(),
            network,
            i64::try_from(issued_at).map_err(|_| Error::Overflow)?,
            i64::try_from(expires_at).map_err(|_| Error::Overflow)?,
            nonce.as_slice(),
            digest.as_slice(),
            signature.as_slice(),
            i64::try_from(now).map_err(|_| Error::Overflow)?
        ],
    )
    .map_err(sql)?;
    if crate::cas::changes(c)? != 1 {
        return Err(Error::Conflict);
    }
    c.execute(
        "INSERT INTO builder_fee_accounting(event_id, user_id, account_id, event_kind, amount_micros, recorded_at)
         VALUES(?1, ?2, ?3, 'mock_approval', 0, ?4)",
        params![nonce.as_slice(), user_id.as_slice(), account_id.as_slice(), i64::try_from(now).map_err(|_| Error::Overflow)?],
    ).map_err(sql)?;
    Ok(())
}

pub fn accounting(c: &Connection, user_id: &[u8; 32]) -> Result<(u64, u64), Error> {
    let (count, amount) = c.query_optional(
        "SELECT COUNT(*), COALESCE(SUM(amount_micros), 0) FROM builder_fee_accounting WHERE user_id = ?1",
        params![user_id.as_slice()],
        |r| Ok((r.get::<i64>(0)?, r.get::<i64>(1)?)),
    ).map_err(sql)?.ok_or(Error::NotFound)?;
    Ok((
        u64::try_from(count).map_err(|_| Error::Overflow)?,
        u64::try_from(amount).map_err(|_| Error::Overflow)?,
    ))
}
