//! Fault injection for the real outbox transaction; absent from production Wasm.
use std::cell::Cell;

use ic_sqlite_vfs::params;

thread_local! {
    // Explicitly cleared by a separate message: a trap also rolls back heap writes.
    static FAULT: Cell<(u8, bool)> = const { Cell::new((0, false)) };
}

fn controller() {
    assert!(ic_cdk::api::is_controller(&ic_cdk::api::msg_caller()));
}

#[ic_cdk::update]
fn test_set_outbox_fault(stage: u8, trap: bool) {
    controller();
    assert!(stage <= 3);
    FAULT.with(|fault| fault.set((stage, trap)));
}

pub(crate) fn checkpoint(stage: u8) -> Result<(), db::error::Error> {
    let (selected, trap) = FAULT.with(Cell::get);
    if selected != stage {
        return Ok(());
    }
    if trap {
        ic_cdk::trap(format!("outbox atomicity fault at stage {stage}"));
    }
    Err(db::error::Error::Invariant("outbox atomicity fault"))
}

// Observe persisted state without exposing arbitrary SQL or modifying the database.

#[ic_cdk::query]
fn test_outbox_atomicity_snapshot(
    request_id: Vec<u8>,
) -> Result<(String, bool, bool, i64, i64, i64), String> {
    controller();
    db::tx::query(|c| {
        c.query_optional(
            "SELECT dispatch_state, signature IS NOT NULL, wire_payload IS NOT NULL,
                    worker_epoch,
                    (SELECT COUNT(*) FROM send_journal_receipts j WHERE j.request_id = a.action_id),
                    (SELECT COUNT(*) FROM action_events e WHERE e.action_id = a.action_id
                     AND e.to_state IN ('signed', 'dispatching'))
             FROM fund_actions a WHERE a.client_request_id = ?1",
            params![request_id],
            |r| {
                Ok((
                    r.get(0)?,
                    r.get::<i64>(1)? != 0,
                    r.get::<i64>(2)? != 0,
                    r.get(3)?,
                    r.get(4)?,
                    r.get(5)?,
                ))
            },
        )
        .map_err(|e| db::error::Error::Sql(e.to_string()))
        .and_then(|row| row.ok_or(db::error::Error::NotFound))
    })
    .map_err(|e| format!("{e:?}"))
}
