# Canister behavior proofs (Lean 4)

This project proves selected safety properties of mathematical models derived
manually from this repository's canister implementation. It does **not** verify
the compiled Rust/Wasm or the complete application. Lean is used because the
targets are state transitions, arithmetic invariants, and arbitrary finite traces.

## Composition scope

The source snapshot includes the single-canister role-scoped composition. The SQL
transition models still cover the same checked arithmetic and atomic state changes.
`Guard` describes the standalone control-guard module; the unified application does
not export that upgrade-governance module. `SendPermit` assumes its state remains
monotonic: restoring the whole unified canister to an older snapshot also restores
the journal and is outside that proof. Separate SQLite scopes in one canister do
not provide an independent backup trust boundary. The ledger model includes gross-to-net allocation fee postings. It does not
prove that venue receipt evidence is complete or that live balances match.
Spot conversion preparation, receipt matching, and journal encoding compatibility
are outside these mathematical models and require separate regression tests.

## Reproduce

With Python 3 and Lean's `elan`/`lake` available, run from the repository root:

```sh
python3 proofs/verify.py
```

The project pins Lean **4.30.0**, uses only its bundled `Std` library, and has no
Mathlib or other package dependencies. The installed toolchain is sufficient;
no global default toolchain setting is needed. If the pinned toolchain is absent,
elan needs to download it.

The verifier checks the Rust source snapshot, runs `lake build --wfail`, and checks
`#print axioms` for every named theorem in the five model modules. It rejects
missing audit entries and dependencies other than Lean's standard `propext`,
`Classical.choice`, and `Quot.sound`. These are logical axioms, not assumptions
about the Internet Computer. There are no `sorry`, `admit`, custom axioms, or
native-evaluation proof shortcuts in the models. Boundary examples use kernel
checked `decide` or ordinary tactics.

Verified locally: **44 named theorems**, all boundary examples, no warnings,
and matching source hashes. To check just the mathematical project independently
of source drift, run `lake build` from this directory. Source hash equality only
detects changes; it is not a refinement proof.

## Proof plan and results

The proof sequence is definitions → local lemmas → safety theorems → trace
induction where applicable. The following table gives the manual refinement links.
Rust paths are relative to the repository root.

| Model and theorem | Proven claim | Implementation correspondence |
| --- | --- | --- |
| `Guard.execute_checks` | Successful execution admission requires the stored deadline and matching target/Wasm hash/argument hash. | `crates/control-guard/src/lib.rs`: `execute_upgrade` checks before `claim_upgrade` |
| `Guard.seven_day_delay` | An admitted freshly scheduled reservation has waited 604800000 ms, assuming deadline addition does not saturate. | `schedule_upgrade`, `crates/api-types/src/guard.rs`: `UPGRADE_DELAY_MS` |
| `Guard.claim_sets_executing`, `no_second_claim` | A successful atomic claim changes the row to `executing`; a subsequent claim on that row fails until a retry transition. | `crates/db/src/repo/guard.rs`: `claim_upgrade` SQL predicate |
| `Guard.cannot_cancel_in_flight` | An executing row cannot be cancelled by the active-row cancellation operation. | `cancel_active_for_target` |
| `Guard.terminal_cannot_execute` | Executed/cancelled rows cannot pass execution admission. | `active_upgrade_for_target`, `claim_upgrade` |
| `Guard.retry_preserves_reservation` | Retry changes only the phase, preserving content and both timestamps. | `revert_executing` |
| `Budget.consume_preserves_safe` | A successful grant preserves total ≤ capacity, risk ≤ total, and risk ≤ capacity − exit reserve, starting from a safe aggregate. | `crates/db/src/repo/budget.rs`: arithmetic checks in `consume` |
| `Budget.paused_only_reconciles` | During recovery pause only reconciliation can receive a grant. | `consume` recovery pause check |
| `Budget.risk_leaves_exit_capacity` | Risk usage plus the exit reserve does not exceed capacity, for a valid configuration and safe usage. | `crates/policy/src/lib.rs`: configuration bounds; budget risk check |
| `Budget.all_reachable_safe` | For a fixed configuration, every finite trace of grants and expiry from empty usage preserves all three aggregate inequalities. | `consume` adds usage; `status` filters expired usage |
| `Budget.recent_dispatch_is_charged` | A dispatch newer than one window and strictly before its grant expiry passes both SQL usage filters, if expiry ≤ consumption + window. | `status` predicates; `crates/api-types/src/operations.rs`: `RestBudgetRequest::valid_at` |
| `Budget.collected_grant_cannot_be_valid` | A grant whose expiry meets the GC threshold cannot be valid at that time. | `consume` DELETE expiry condition; `valid_at` exclusive expiry |
| `Ledger.checkedTotal_correct` | Every successful left-to-right checked sum equals the mathematical sum plus its initial accumulator. | `crates/db/src/repo/ledger.rs`: `post_journal` checked-add loop |
| `Ledger.accepted_balanced` | An accepted journal's posting amounts sum to zero. | `post_journal` nonempty, nonzero and total checks |
| `Ledger.every_committed_history_balanced` | Any finite history of accepted, atomically appended journals has zero total. | Successful `post_journal` calls inside `db::tx::update` |
| `Ledger.posting_templates_balanced`, `positive_templates_accepted` | All ten modeled posting templates balance; amounts from 1 through i64::MAX pass the arithmetic checks, including every intermediate sum. | Ten posting constructors listed below |
| `Ledger.fee_allocation_balanced`, `fee_allocation_accepted` | Gross transit settles while net receipt becomes trading equity; the postings balance and pass all checked sums for valid fees. | `allocation_confirm_with_fee` |
| `Ledger.withdrawable_partition` | A successful reserve-minus-allocation-holds calculation gives available + holds = reserve and available ≤ reserve. | `user_balances`; `crates/db/src/repo/funds.rs`: `held_allocation_total` |
| `Outbox.stale_cas_rejected`, `successful_cas_matches`, `successful_cas_step` | The executable CAS model rejects a mismatched worker epoch; successful matching lifecycle CAS operations correspond to modeled steps. | `crates/db/src/repo/actions.rs`: `mark_signed`, `cas_transition` |
| `Outbox.step_epoch_monotone`, `trace_epoch_monotone`, `stale_after_reclaim` | Epochs never decrease. After reclaim, the old epoch fails every later modeled lifecycle CAS, regardless of intervening steps. | `claim_action` increments epoch; callbacks retain their original `action.worker_epoch` |
| `Outbox.step_preserves_safe`, `trace_preserves_safe`, `at_most_one_dispatch_claim` | For any finite interleaving from queued, the same action receives at most one dispatch claim. | `claim_action`, `mark_signed`, `mark_dispatching`, completion and abort transitions |
| `Outbox.edge_preserves_post`, `step_preserves_post`, `no_redispatch_after_unknown`, `post_cannot_abort` | Post-dispatch states cannot return to pre-dispatch or acquire another dispatch claim; unknown can be reconciled but not aborted as unsent. | `mark_unknown`, `mark_reconciled`, `reconcile_unknown`, `mark_resolved`, `abort_unsent` |
| `Outbox.send_transaction_trace` | The actual signing-to-dispatching transaction is representable by the model's two consecutive steps. | `crates/funds-vault/src/outbox.rs`: allocation, withdrawal, recovery send transactions |

The ten ledger templates are `trading_deposit_confirmed`, `allocation_start`,
`allocation_confirm`, `recovery_confirm`, `deposit_confirmed`, `unmatched_deposit`,
`claim_unmatched_deposit`, `withdrawal_reserve`, `withdrawal_release`, and
`payout_settled`. Posting order and signs match the implementation. Account
identities are projected away, so these theorems say nothing about whether the
correct user's account was chosen.

## Outbox interleavings

`Outbox.lean` models one existing `fund_action` ID, its phase and worker epoch,
and a proof-only counter of successful dispatch claims. `Edge` enumerates all
ordinary lifecycle CAS edges. `Step` additionally permits claim/reclaim and
stuttering (awaiting, failed transactions, or irrelevant metadata updates).
`Trace` allows arbitrary finite interleavings, including repeated reclamation
while an earlier worker awaits signature, budget, or journal responses.

The checked example follows queued → worker 1 signing → lease expiry → worker 2
signing → signed → dispatching → unknown. Separate executable examples show worker
1's CAS failing and worker 2's CAS succeeding at epoch 2. The general
`stale_after_reclaim` theorem covers any subsequent trace, not just this example.

The implementation commits `mark_signed`, journal receipt recording, and
`mark_dispatching` together before calling `venue::post_usd_send`. The model permits
an intermediate signed state, so it allows more interleavings than that transaction.
The lifecycle safety proof holds even with those extra interleavings. Actual
signature bytes, journal receipt verification, and business validation are omitted;
their rejection paths do not grant additional lifecycle transitions.

`grants` counts **dispatch claims**, not HTTP calls or settled transfers.
In the three inspected send paths, a successful send transaction is followed by
one call to `post_usd_send`, whose body contains one `.send().await`. This is a
manual correspondence observation, not a machine-checked Rust control-flow or
transport proof. HTTP runtime behavior and external exactly-once execution remain
outside the theorem.

## Send authorization and manual cancellation

`SendPermit.lean` models the independent journal permit for one send intent.
A new intent starts `prepared`; `authorize` grants permission exactly once and
moves permanently to `authorized`, while `cancel` moves permanently to `cancelled`.
Legacy intents have no permit and cannot supply cancellation evidence.

The nine theorems prove terminal cancellation, single-use authorization, refusal
to cancel authorized/legacy intents, and preservation of both terminal states
under arbitrary finite interleavings. In particular, cancellation excludes prior
successful authorization and prevents a delayed callback from acquiring it later.
The correspondence is manual: `crates/db/src/repo/send_journal.rs` implements the
conditional SQL updates; its schema migration creates permits without backfilling
legacy records; `crates/send-journal/src/lib.rs` authenticates the worker and
atomically prepares intents with permits. `crates/journal-client/src/lib.rs`
requires cancellation evidence before `finish_cancelled_send` in
`crates/db/src/repo/send_journal_client.rs` aborts the unsigned action, releases
its reservation, acknowledges the cancelled intent, and releases the writer.
All three fund send paths require fresh authorization before HTTP dispatch.

## Assumptions and limits

For measured rollback behavior and the manual recovery protocol,
see [DB atomicity evidence](db-atomicity.md). Those tests support, but do not
formally discharge, the atomicity assumption below.

The Lean theorems hold for the definitions in this project. Applying them to the
running canisters requires the following **unproved refinement obligations**;
they are not silently introduced as Lean axioms.

1. **Atomicity and persistence.** Successful modeled steps correspond to committed
   SQLite transactions, rejected transactions leave no partial postings, and CAS
   operations serialize. `crates/db/src/tx.rs` documents and delegates this to
   `ic-sqlite-vfs`. That dependency, the IC scheduler, upgrades, rollback/recovery,
   and stable memory are not formally modeled here.
2. **Guard scope.** One reservation ID is modeled, not all rows for a target.
   The SQL unique index covers `pending`/`executable`, not `executing`.
   `no_second_claim` therefore does not prove exclusion across distinct reservations
   or exactly-once external installation. The failure path can call
   `revert_executing`, including after a failed status query, and allow a retry.
   Hash equality is modeled; collision resistance and byte equality are not proven.
   Principals/hashes are abstract equality tokens. SNS/controller authorization
   and management-canister behavior are outside this model.
3. **Guard timestamps.** `schedule` includes u64 saturation. `seven_day_delay`
   explicitly requires `scheduled + delay ≤ u64::MAX`. Successful correspondence
   to the DB also requires timestamps representable as nonnegative i64 values:
   the SQL write uses `as i64`, and row conversion rejects negative timestamps.
   The clock and its relation to elapsed real time are unproved.
4. **Budget configuration.** Trace safety assumes a fixed configuration. The
   implementation permits lowering capacity below current usage; a Lean example
   demonstrates why an unconditional invariant across reconfiguration is false.
   The exit reserve limits **risk** consumption; exits and reconciliation can use
   remaining total capacity, so this is not a guarantee of immediate exit service.
5. **Budget refinement.** Totals are natural numbers representing successful SQL
   sums/conversions. Expiry transitions assume filtered nonnegative rows with risk
   a subset of total. Successful validated grants have positive weights, consistent
   expiry-encoded IDs, and timestamps in the supported i64 range. SQL time filters
   use signed integers (including negative floors near time zero). Authorization,
   duplicate-ID rejection, ID encoding, SQL execution, and one dispatch per grant
   are not proved. In particular, the per-grant retention lemma is **not** a proof
   of the external venue's complete rolling-window rate limit.
6. **Ledger refinement.** Posting amounts and every intermediate sum are checked
   against i64 bounds in the model. DB constraints may still reject a mathematically
   accepted journal. Journal trace induction assumes successful atomic insertion;
   duplicate event/request protection, account ownership, solvency, and external
   balance reconciliation are not proven. The withdrawable lemma models successful
   checked subtraction only, not the entire reservation lifecycle.
7. **Outbox scope and arithmetic.** The transition list is explicit and exhaustive
   *within the model*. Its correspondence to all production paths remains a manual
   obligation. Theorems cover the ordinary lifecycle of one existing action, not
   deletion/recreation, journal reconstruction, DB rollback to an earlier snapshot,
   arbitrary migrations, or direct state rewrites. Epochs are natural numbers;
   application to SQLite requires nonnegative, exactly represented i64 epochs and
   increments within range. SQLite's overflow behavior is not proved.
8. **Outbox scheduling and effects.** Claims include the strict `lease < now`
   predicate but lease storage and clocks are abstracted as inputs, deliberately
   allowing extra reclamations. No fairness or eventual-completion claim is made.
   A stale worker may already have called signing or journal services before its
   lifecycle CAS is rejected. The theorem prevents its stale lifecycle update;
   it does not exclude all external activity. Crash after dispatch claim but before
   HTTP send can leave zero transfers, which is consistent with at-most-once
   admission and does not prove liveness.

9. **Send permit refinement.** The independent journal retains its permit state
   across vault recovery. Restoring or downgrading the journal to discard permits,
   or running a sender that bypasses authorization, is outside the model. Worker
   authentication, SQL atomicity, wire delivery, local candidate selection,
   reservation release and migration correctness are manually reviewed/tested,
   not proved. A lost authorization reply can leave an authorized but unsent
   action; the protocol deliberately does not infer that it is safe to cancel.

Spot receipt matching, conversion settlement and automatic ownership claims are tested in PocketIC, not proved by this suite.

Trading order execution, the full send journal, authentication/HPKE, signature
correctness, liveness, and real-money safety remain outside this proof suite.

## Updating the implementation

`source-snapshot.json` records SHA-256 hashes of the nineteen reviewed Rust files.
When the verifier reports drift, review the affected definitions and the mapping
above, revise the proofs if necessary, and only then refresh the corresponding
hashes. Do not treat refreshing hashes as verification of model correspondence.
`Audit.lean` must list each newly added named theorem.

## Manual worker integration correspondence

The integration adds durable permission checks around existing lifecycle transitions. Candidate filters restrict execution; consuming a permission before an await does not introduce a dispatch transition or reset an epoch. `claim_action`, signing-to-dispatch transactions, journal authorization, nonce binding, and post-dispatch CAS edges retain their existing semantics. Database wakeup callbacks run after the transaction releases its connection and may query committed work only. Allocation-arrival reads and migration maintenance change scheduling, not the modeled send lifecycle. The source hashes were refreshed after reviewing these differences. Permission ownership, generation handling, crash persistence, migration scheduling, and eventual completion are tested in PocketIC and remain outside the Lean model.
