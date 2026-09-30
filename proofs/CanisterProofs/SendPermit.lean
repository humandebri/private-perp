import Std

namespace CanisterProofs.SendPermit

-- An existing journal intent has a permanent permit row only in the new protocol.
inductive State where
  | legacy | prepared | authorized | cancelled
  deriving DecidableEq, Repr

def authorize : State → State × Bool
  | .prepared => (.authorized, true)
  | s => (s, false)

def cancel : State → State × Bool
  | .prepared => (.cancelled, true)
  | .cancelled => (.cancelled, true)
  | s => (s, false)

theorem successful_cancel_is_terminal (s : State) (h : (cancel s).2 = true) :
    (cancel s).1 = .cancelled := by
  cases s <;> simp_all [cancel]

theorem cancelled_never_authorizes : authorize .cancelled = (.cancelled, false) := rfl

theorem authorized_never_cancels : cancel .authorized = (.authorized, false) := rfl

theorem authorization_is_single_use (s : State) (h : (authorize s).2 = true) :
    (authorize (authorize s).1).2 = false := by
  cases s <;> simp_all [authorize]

theorem legacy_is_not_unsent_evidence : (cancel .legacy).2 = false := rfl

inductive Step : State → State → Prop where
  | authorize (s) : Step s (authorize s).1
  | cancel (s) : Step s (cancel s).1
  | idle (s) : Step s s

inductive Trace : State → State → Prop where
  | refl (s) : Trace s s
  | next {a b c} : Trace a b → Step b c → Trace a c

theorem cancellation_survives_interleavings (s out : State)
    (h : Trace s out) (hs : s = .cancelled) : out = .cancelled := by
  induction h with
  | refl => exact hs
  | next _ step ih =>
    cases step <;> simp_all [authorize, cancel]

theorem delayed_callback_cannot_send (s out : State)
    (h : Trace s out) (hs : s = .cancelled) : (authorize out).2 = false := by
  rw [cancellation_survives_interleavings s out h hs]
  rfl

theorem authorization_survives_interleavings (s out : State)
    (h : Trace s out) (hs : s = .authorized) : out = .authorized := by
  induction h with
  | refl => exact hs
  | next _ step ih =>
    cases step <;> simp_all [authorize, cancel]

theorem cancellation_excludes_prior_authorization (out : State)
    (history : Trace .authorized out) : (cancel out).2 = false := by
  rw [authorization_survives_interleavings .authorized out history rfl]
  rfl

end CanisterProofs.SendPermit
