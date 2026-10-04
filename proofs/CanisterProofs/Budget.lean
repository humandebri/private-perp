import Std

namespace CanisterProofs.Budget

inductive Class where
  | risk | exit | reconcile
  deriving DecidableEq, Repr

structure Config where
  capacity : Nat
  exitReserve : Nat

structure Usage where
  total : Nat
  risk : Nat
  deriving DecidableEq, Repr

def Safe (c : Config) (s : Usage) : Prop :=
  s.risk ≤ s.total ∧ s.total ≤ c.capacity ∧ s.risk ≤ c.capacity - c.exitReserve

-- Input validation, worker authorization and duplicate detection happen before
-- this arithmetic core. Those checks can only reject an additional request.
def consume (c : Config) (s : Usage) (kind : Class) (weight : Nat)
    (paused : Bool) : Option Usage :=
  if paused = true ∧ kind ≠ .reconcile then none
  else if s.total + weight > c.capacity ∨
      (kind = .risk ∧ s.risk + weight > c.capacity - c.exitReserve) then none
  else some ⟨s.total + weight, if kind = .risk then s.risk + weight else s.risk⟩

theorem consume_preserves_safe (c : Config) (s out : Usage) (kind : Class)
    (weight : Nat) (paused : Bool) (hs : Safe c s)
    (h : consume c s kind weight paused = some out) : Safe c out := by
  unfold consume at h
  split at h
  · contradiction
  · split at h
    · contradiction
    · cases h
      unfold Safe at *
      split <;> simp_all <;> omega

theorem paused_only_reconciles (c : Config) (s out : Usage) (kind : Class)
    (weight : Nat) (h : consume c s kind weight true = some out) : kind = .reconcile := by
  cases kind <;> simp_all [consume]

theorem risk_leaves_exit_capacity (c : Config) (s : Usage)
    (configValid : c.exitReserve ≤ c.capacity) (h : Safe c s) :
    s.risk + c.exitReserve ≤ c.capacity := by
  unfold Safe at h
  omega

-- Expiry removes entries from both totals. Removed risk is part of removed total;
-- remaining risk must also be part of remaining total (the SQL filtered sums).
inductive Step (c : Config) : Usage → Usage → Prop where
  | grant {s out kind weight paused} :
      consume c s kind weight paused = some out → Step c s out
  | expire {s out} : out.total ≤ s.total → out.risk ≤ s.risk →
      out.risk ≤ out.total → Step c s out

inductive Reachable (c : Config) : Usage → Prop where
  | empty : Reachable c ⟨0, 0⟩
  | next {s out} : Reachable c s → Step c s out → Reachable c out

theorem all_reachable_safe (c : Config) (s : Usage) (h : Reachable c s) : Safe c s := by
  induction h with
  | empty => simp [Safe]
  | next _ step ih =>
    cases step with
    | grant hg => exact consume_preserves_safe _ _ _ _ _ _ ih hg
    | expire ht hr hrt => unfold Safe at *; omega

-- Signed integers reproduce SQL's now - WINDOW even for now < WINDOW.
def window : Int := 60000
def charged (consumed expires now : Int) : Prop :=
  consumed > now - 2 * window ∧ expires > now - window

-- A dispatch in (now-WINDOW, now], made strictly before expiry using a
-- grant with expiry <= consumed+WINDOW, must still appear in status's sum.
theorem recent_dispatch_is_charged (consumed expires dispatch now : Int)
    (validGrant : expires ≤ consumed + window)
    (beforeExpiry : dispatch < expires) (recent : now - window < dispatch) :
    charged consumed expires now := by
  unfold charged window at *
  omega

theorem collected_grant_cannot_be_valid (expires now : Int)
    (gc : expires ≤ now - window) : ¬ now < expires := by
  unfold window at gc
  omega

example : consume ⟨10, 3⟩ ⟨7, 7⟩ .risk 1 false = none := by decide
example : consume ⟨10, 3⟩ ⟨7, 7⟩ .exit 3 false = some ⟨10, 7⟩ := by decide
example : consume ⟨10, 3⟩ ⟨7, 7⟩ .exit 1 true = none := by decide
example : consume ⟨10, 3⟩ ⟨7, 7⟩ .reconcile 1 true = some ⟨8, 7⟩ := by decide
-- Changing configuration is deliberately not a Reachable transition.
example : Safe ⟨10, 3⟩ ⟨10, 7⟩ ∧ ¬ Safe ⟨9, 3⟩ ⟨10, 7⟩ := by simp [Safe]

end CanisterProofs.Budget
