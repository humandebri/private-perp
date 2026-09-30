# DB atomicity and manual recovery

Verified locally on 2026-09-29 with Rust 1.97.0, PocketIC server 16.0.0,
and the application's pinned `ic-sqlite-vfs = 2.0.0` (`sqlite-precompiled` for
Wasm). The vault retains permanent MemoryId 0. Send-journal migration 3 adds a
`send_permits` table; existing intents are deliberately not backfilled.

## Reproduced problem and fix

Before this change, rollback of the local send transaction left an independently
committed journal intent and a held local journal writer. Ordinary retry returned
`JournalWriterBusy`; SNS-authorized `resume_journal` repeatedly returned
`PolicyUnavailable`. Thus local DB atomicity was correct, but the tested manual
recovery path could not release the reservation or writer. These were controlled
fault reproductions, not evidence of a production incident or its frequency.

New allocation, withdrawal and recovery sends atomically append an intent and a
`prepared` permit to the independent send journal. After committing local
signature/receipt/dispatch state, the sender must obtain a single-use remote
`authorized` permit before HTTP dispatch. Authorization cannot be granted twice.

Manual resume can permanently cancel a `prepared` permit. It requires one staged
intent matching the outstanding writer, an unsigned queued/signing action, a
reserved request, contiguous send history and a matching recovery-event stream.
After remote cancellation succeeds, one local transaction aborts the action,
releases the reservation (including withdrawal ledger reversal), rejects the
request, acknowledges the cancelled intent and releases the writer. The receipt
records reconciliation of the intent, not successful transfer.

Cancellation and authorization are mutually exclusive. Delayed sender callbacks
cannot obtain authorization after cancellation. Resume also advances the writer
epoch, and final unlock checks that epoch, so an older resume cannot unlock a
newer writer. Dispatch rechecks the local lock, action phase and worker epoch
after awaiting authorization.

## Manual operating procedure

1. Upgrade **send-journal before funds-vault**, through the project's existing
   authorized upgrade process. The added journal methods must exist before the
   vault uses them. No deployment was performed as part of this verification.
2. Investigate and remove the triggering fault. Through the existing SNS-authorized
   control-guard route, invoke `resume_journal(vault_principal)`.
3. Verify successful resume and the vault's unlocked journal status. A recoverable
   unsigned request is now rejected/aborted and its reserved funds are released.
   Submit a **new request ID** if the user still wants that transfer. Resume does
   not resend the cancelled request. Repeated successful resume is harmless.
4. If resume returns `PolicyUnavailable`, retain the stop and investigate the
   external journal/venue state. Do not clear the writer or infer unsent status
   from missing local signatures alone.

Legacy intents without a permit, already-authorized sends (even if the permission
reply was lost), signed/dispatching actions, multiple staged intents, and more
complex backup/recovery gaps are not resolved by this cancellation path. Such
states still need evidence-based reconciliation. The change cannot retroactively
make pre-upgrade incidents safely cancellable. Journal permit history must remain
durable and independent of vault backup rollback; bypassing it via downgrade or
journal restore invalidates this safety argument.

## Application transaction faults

The actual send transactions have three `test-venue`-only checkpoints:

| Checkpoint | Completed writes inside the failing transaction | Application `Err` | IC trap |
| --- | --- | --- | --- |
| 1 | Signature, wire payload, signed phase and action event | Rollback verified | Rollback verified |
| 2 | Above plus local journal receipt and writer release | Rollback verified | Rollback verified |
| 3 | Above plus dispatching phase and action event | Rollback verified | Rollback verified |

Four PocketIC tests exercise **12 cases**: allocation/withdrawal × three checkpoints
× Err/trap, each using a fresh canister. They assert the injected error marker,
unsigned `signing` state, no receipt or signed/dispatching events, retained funds,
and no HTTP request. Ordinary lease-based retry still returns `JournalWriterBusy`.
Unauthorized resume is rejected. Authorized resume succeeds twice, releases the
reservation exactly once, leaves the old action aborted and allows a new request
to send once. Upgrading the vault preserves the recovered state.

A separate test grants remote authorization to an intent whose local transaction
was rolled back, modeling ambiguous local evidence. Manual resume refuses to
release its funds. This tests the conservative guard, not an actual external POST.
The send-journal tests check both cancellation/authorization orderings, repeated
calls, unauthorized callers and refusal to reinterpret legacy intents.

The checkpoints also exist in recovery sends, but its fault matrix is not covered.
These injections happen before the enclosing DB transaction commits; they do not
inject faults inside VFS stable-page writes. Existing successful-send, uncertain
reply, backup replay and recovery tests passed: **21 Outbox + 11 send-journal +
15 vault recovery = 47 tests** (the final Outbox refusal test was run separately).

The test endpoints require a controller and expose fixed observations and fault
selection, not arbitrary SQL. The module and calls compile only with `test-venue`;
CI checks that production artifacts exclude the endpoint names.

## Reproduce

Build both artifact variants and run the relevant suites from the repository root:

```sh
bash scripts/pocket-ic-test.sh --test vault_outbox --test send_journal --test vault_recovery -- --test-threads=1
python3 proofs/verify.py
```

The upstream VFS failpoint tests were run against a local copy of the exact
published dependency, as described above, with:

```sh
cargo test --locked --manifest-path target/atomicity-vfs-2.0.0/Cargo.toml \
  --target-dir target/atomicity-vfs-build --lib sqlite_vfs::failpoint_tests -- --test-threads=1
```

## Relation to Lean

All **42 named theorems** pass the axiom audit, including nine new send-permit
properties. The source snapshot includes the reviewed permit schema/repository,
canister endpoints, journal client and recovery finalizer. See [proof scope](README.md).

DB atomicity remains an **unproved refinement assumption** in Lean. Fault tests
provide evidence, not a theorem about SQLite C code, VFS Rust code or the IC runtime.
Exhaustive Wasm VFS commit failures, instruction/cycle exhaustion and the recovery
send fault matrix remain outside this verification.
