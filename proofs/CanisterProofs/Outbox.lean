import Std

/- One existing fund_action ID. Successful DB updates serialize; awaiting work
   retains a token (worker_epoch). Grants is a ghost count of dispatch claims,
   not a count of HTTP requests or external transfers. -/
namespace CanisterProofs.Outbox

inductive Phase where
  | queued | signing | signed | dispatching | unknown | reconciled | aborted
  deriving DecidableEq, Repr

def pre (p : Phase) : Prop := p = .queued ∨ p = .signing ∨ p = .signed
def post (p : Phase) : Prop := p = .dispatching ∨ p = .unknown ∨ p = .reconciled

structure State where
  phase : Phase
  epoch : Nat
  grants : Nat
  deriving DecidableEq, Repr

-- The WHERE worker_epoch = token AND dispatch_state = expected predicate.
def cas (s : State) (token : Nat) (expected target : Phase) : Option State :=
  if s.epoch = token ∧ s.phase = expected then some { s with phase := target }
  else none

theorem stale_cas_rejected (s : State) (token : Nat) (expected target : Phase)
    (stale : token ≠ s.epoch) : cas s token expected target = none := by
  simp [cas, Ne.symm stale]

theorem successful_cas_matches (s out : State) (token : Nat) (expected target : Phase)
    (h : cas s token expected target = some out) :
    s.epoch = token ∧ s.phase = expected ∧ out = { s with phase := target } := by
  unfold cas at h
  split at h
  · rename_i hm
    exact ⟨hm.1, hm.2, (Option.some.inj h).symm⟩
  · contradiction

-- Exhaustive list of the ordinary lifecycle CAS edges, not arbitrary SQL writes.
inductive Edge : Phase → Phase → Prop where
  | sign : Edge .signing .signed
  | dispatch : Edge .signed .dispatching
  | unknown : Edge .dispatching .unknown
  | reconcile : Edge .dispatching .reconciled
  | resolve : Edge .unknown .reconciled
  | abort {p} : pre p → Edge p .aborted

def increment (target : Phase) : Nat := if target = .dispatching then 1 else 0

-- Lease values are provided explicitly. Failed operations and await/resume alone
-- stutter. Extra business checks may reject transitions, never add new edges.
inductive Step : State → State → Prop where
  | claim {s : State} (now : Nat) (lease : Option Nat) :
      (s.phase = .queued ∨ s.phase = .signing) →
      (lease = none ∨ ∃ deadline, lease = some deadline ∧ deadline < now) →
      Step s { s with phase := .signing, epoch := s.epoch + 1 }
  | update {s : State} (token : Nat) (target : Phase) :
      token = s.epoch → Edge s.phase target →
      Step s { s with phase := target, grants := s.grants + increment target }
  | idle (s : State) : Step s s

inductive Trace : State → State → Prop where
  | refl (s) : Trace s s
  | next {a b c} : Trace a b → Step b c → Trace a c

-- Connect the executable CAS model with lifecycle transitions and ghost counting.
theorem successful_cas_step (s out : State) (token : Nat) (expected target : Phase)
    (edge : Edge expected target) (h : cas s token expected target = some out) :
    Step s { out with grants := s.grants + increment target } := by
  obtain ⟨he, hp, ho⟩ := successful_cas_matches s out token expected target h
  subst out
  have e : Edge s.phase target := by simpa [hp] using edge
  exact Step.update token target he.symm e

theorem step_epoch_monotone {s out : State} (h : Step s out) : s.epoch ≤ out.epoch := by
  cases h <;> simp

theorem trace_epoch_monotone {s out : State} (h : Trace s out) : s.epoch ≤ out.epoch := by
  induction h with
  | refl => exact Nat.le_refl _
  | next _ step ih => exact Nat.le_trans ih (step_epoch_monotone step)

-- Even arbitrarily many intervening events cannot make an old callback current.
theorem stale_after_reclaim (s out : State)
    (h : Trace { s with phase := .signing, epoch := s.epoch + 1 } out)
    (expected target : Phase) : cas out s.epoch expected target = none := by
  have hm := trace_epoch_monotone h
  apply stale_cas_rejected
  simp only at hm
  omega

def Safe (s : State) : Prop :=
  s.grants ≤ 1 ∧ (pre s.phase → s.grants = 0) ∧ (post s.phase → s.grants = 1)

theorem step_preserves_safe {s out : State} (hs : Safe s) (h : Step s out) : Safe out := by
  cases h with
  | claim now lease eligible expired =>
    rcases eligible with hp | hp <;> simp_all [Safe, pre, post]
  | update token target current edge =>
    rcases s with ⟨phase, epoch, grants⟩
    cases phase <;> cases edge <;> simp_all [Safe, pre, post, increment]
  | idle => exact hs

theorem trace_preserves_safe {s out : State} (hs : Safe s) (h : Trace s out) : Safe out := by
  induction h with
  | refl => exact hs
  | next _ step ih => exact step_preserves_safe ih step

theorem at_most_one_dispatch_claim (initialEpoch : Nat) (out : State)
    (h : Trace ⟨.queued, initialEpoch, 0⟩ out) : out.grants ≤ 1 := by
  exact (trace_preserves_safe (by simp [Safe, pre, post]) h).1

theorem edge_preserves_post {a b : Phase} (ha : post a) (edge : Edge a b) : post b := by
  cases a <;> cases edge <;> simp_all [pre, post]

theorem step_preserves_post {s out : State} (hs : post s.phase) (h : Step s out) :
    post out.phase ∧ out.grants = s.grants := by
  cases h with
  | claim now lease eligible expired =>
    rcases eligible with hp | hp <;> simp_all [post]
  | update token target current edge =>
    rcases s with ⟨phase, epoch, grants⟩
    cases phase <;> cases edge <;> simp_all [pre, post, increment]
  | idle => exact ⟨hs, rfl⟩

theorem no_redispatch_after_unknown (s out : State) (unknown : s.phase = .unknown)
    (h : Trace s out) : post out.phase ∧ out.grants = s.grants := by
  have hp : post s.phase := by simp [post, unknown]
  induction h with
  | refl => exact ⟨hp, rfl⟩
  | next _ step ih =>
    have hs := step_preserves_post ih.1 step
    exact ⟨hs.1, hs.2.trans ih.2⟩

theorem post_cannot_abort (p : Phase) (hp : post p) : ¬ Edge p .aborted := by
  intro edge
  cases edge with
  | abort hpre => rcases hpre with h | h | h <;> simp_all [post]

-- Actual outbox commits mark_signed and mark_dispatching in one transaction.
-- This model splits it into two steps: the real operation is a permitted trace.
theorem send_transaction_trace (epoch grants : Nat) :
    Trace ⟨.signing, epoch, grants⟩ ⟨.dispatching, epoch, grants + 1⟩ := by
  have sign : Step ⟨.signing, epoch, grants⟩ ⟨.signed, epoch, grants⟩ := by
    simpa [increment] using Step.update (s := ⟨.signing, epoch, grants⟩) epoch .signed rfl Edge.sign
  have dispatch : Step ⟨.signed, epoch, grants⟩ ⟨.dispatching, epoch, grants + 1⟩ := by
    simpa [increment] using Step.update (s := ⟨.signed, epoch, grants⟩) epoch .dispatching rfl Edge.dispatch
  exact .next (.next (.refl _) sign) dispatch

-- Non-vacuous asynchronous scenario: reclaim while worker 1 awaits a response.
example : cas ⟨.signing, 2, 0⟩ 1 .signing .signed = none := by decide
example : cas ⟨.signing, 2, 0⟩ 2 .signing .signed = some ⟨.signed, 2, 0⟩ := by decide
example : Trace ⟨.queued, 0, 0⟩ ⟨.unknown, 2, 1⟩ := by
  have first : Step ⟨.queued, 0, 0⟩ ⟨.signing, 1, 0⟩ :=
    .claim 0 none (Or.inl rfl) (Or.inl rfl)
  have reclaim : Step ⟨.signing, 1, 0⟩ ⟨.signing, 2, 0⟩ :=
    .claim 11 (some 10) (Or.inr rfl) (Or.inr ⟨10, rfl, by decide⟩)
  have sign : Step ⟨.signing, 2, 0⟩ ⟨.signed, 2, 0⟩ :=
    .update 2 .signed rfl .sign
  have dispatch : Step ⟨.signed, 2, 0⟩ ⟨.dispatching, 2, 1⟩ :=
    .update 2 .dispatching rfl .dispatch
  have unknown : Step ⟨.dispatching, 2, 1⟩ ⟨.unknown, 2, 1⟩ :=
    .update 2 .unknown rfl .unknown
  exact .next (.next (.next (.next (.next (.refl _) first) reclaim) sign) dispatch) unknown

end CanisterProofs.Outbox
