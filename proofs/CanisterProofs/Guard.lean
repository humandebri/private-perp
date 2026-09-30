import Std

/- A single persisted upgrade_id, not all upgrades for a target. SQL operations
   are modeled as atomic. Content contains hashes, not the original bytes. -/
namespace CanisterProofs.Guard

inductive Phase where
  | pending | executable | executing | executed | cancelled
  deriving DecidableEq, Repr

structure Content where
  target : Nat
  wasmHash : Nat
  argHash : Nat
  deriving DecidableEq, Repr

structure Reservation where
  content : Content
  scheduledAt : Nat
  executableAt : Nat
  phase : Phase
  deriving DecidableEq, Repr

def delay : Nat := 604800000
def maxU64 : Nat := 18446744073709551615

def schedule (content : Content) (now : Nat) : Reservation :=
  ⟨content, now, min (now + delay) maxU64, .pending⟩

def active (p : Phase) : Bool := p == .pending || p == .executable

def claim (r : Reservation) : Option Reservation :=
  if active r.phase then some { r with phase := .executing } else none

def execute (r : Reservation) (input : Content) (now : Nat) : Option Reservation :=
  if r.executableAt ≤ now ∧ input = r.content then claim r else none

def cancel (r : Reservation) : Option Reservation :=
  if active r.phase then some { r with phase := .cancelled } else none

def finish (r : Reservation) : Option Reservation :=
  if r.phase = .executing then some { r with phase := .executed } else none

def retry (r : Reservation) : Option Reservation :=
  if r.phase = .executing then some { r with phase := .executable } else none

theorem execute_checks (r out : Reservation) (input : Content) (now : Nat)
    (h : execute r input now = some out) :
    r.executableAt ≤ now ∧ input = r.content := by
  unfold execute at h
  split at h
  · assumption
  · contradiction

theorem seven_day_delay (content input : Content) (scheduled now : Nat) (out : Reservation)
    (fits : scheduled + delay ≤ maxU64)
    (h : execute (schedule content scheduled) input now = some out) :
    scheduled + delay ≤ now := by
  have hc := (execute_checks _ _ _ _ h).1
  simpa [schedule, Nat.min_eq_left fits] using hc

theorem claim_sets_executing (r out : Reservation) (h : claim r = some out) :
    out = { r with phase := .executing } := by
  unfold claim at h
  split at h
  · exact (Option.some.inj h).symm
  · contradiction

-- Excludes another successful claim until an explicit retry; not exactly-once install.
theorem no_second_claim (r out : Reservation) (h : claim r = some out) :
    claim out = none := by
  rw [claim_sets_executing r out h]
  simp [claim, active]

theorem cannot_cancel_in_flight (r out : Reservation) (h : claim r = some out) :
    cancel out = none := by
  rw [claim_sets_executing r out h]
  simp [cancel, active]

theorem terminal_cannot_execute (r : Reservation) (input : Content) (now : Nat)
    (h : r.phase = .executed ∨ r.phase = .cancelled) :
    execute r input now = none := by
  rcases h with h | h <;> simp [execute, claim, active, h]

theorem retry_preserves_reservation (r out : Reservation) (h : retry r = some out) :
    out.content = r.content ∧ out.scheduledAt = r.scheduledAt ∧
    out.executableAt = r.executableAt ∧ out.phase = .executable := by
  unfold retry at h
  split at h
  · cases h
    exact ⟨rfl, rfl, rfl, rfl⟩
  · contradiction

-- The non-overflow premise in seven_day_delay is necessary for saturating_add.
example : (schedule ⟨0, 0, 0⟩ maxU64).executableAt = maxU64 := by decide

example : execute (schedule ⟨1, 2, 3⟩ 0) ⟨1, 2, 3⟩ delay =
    some ⟨⟨1, 2, 3⟩, 0, delay, .executing⟩ := by decide

example : execute (schedule ⟨1, 2, 3⟩ 0) ⟨1, 2, 3⟩ (delay - 1) = none := by decide

example : execute (schedule ⟨1, 2, 3⟩ 0) ⟨1, 2, 4⟩ delay = none := by decide

end CanisterProofs.Guard
