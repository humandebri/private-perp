import Std

namespace CanisterProofs.Ledger

def fitsI64 (n : Int) : Prop := -9223372036854775808 ≤ n ∧ n ≤ 9223372036854775807
instance (n : Int) : Decidable (fitsI64 n) := inferInstanceAs (Decidable (_ ∧ _))

-- Same left-to-right checked_add loop and zero-posting rejection as post_journal.
def checkedTotal : Int → List Int → Option Int
  | acc, [] => some acc
  | acc, x :: xs =>
    if x = 0 ∨ ¬ fitsI64 x ∨ ¬ fitsI64 (acc + x) then none
    else checkedTotal (acc + x) xs

theorem checkedTotal_correct (xs : List Int) (acc result : Int)
    (h : checkedTotal acc xs = some result) : result = acc + xs.sum := by
  induction xs generalizing acc with
  | nil => simp [checkedTotal] at h; simp [← h]
  | cons x xs ih =>
    simp only [checkedTotal] at h
    split at h
    · contradiction
    · have hi := ih (acc + x) h
      simp only [List.sum_cons]
      omega

def accepted (xs : List Int) : Prop := xs ≠ [] ∧ checkedTotal 0 xs = some 0

theorem accepted_balanced (xs : List Int) (h : accepted xs) : xs.sum = 0 := by
  have ht := checkedTotal_correct xs 0 0 h.2
  omega

-- Appending a committed journal represents successful atomic DB insertion.
inductive Reachable : List Int → Prop where
  | empty : Reachable []
  | post {old journal} : Reachable old → accepted journal → Reachable (old ++ journal)

theorem every_committed_history_balanced (xs : List Int) (h : Reachable xs) :
    xs.sum = 0 := by
  induction h with
  | empty => rfl
  | post _ valid ih => simp [List.sum_append, ih, accepted_balanced _ valid]

-- Exact posting amount sequences; account identities are intentionally projected away.
inductive Kind where
  | tradingDeposit | allocationStart | allocationConfirm | recoveryConfirm
  | deposit | unmatchedDeposit | claimDeposit | withdrawalReserve | withdrawalRelease | payout

def postings (k : Kind) (a : Int) : List Int :=
  match k with
  | .allocationStart | .allocationConfirm => [a, -a, a, -a]
  | .recoveryConfirm => [a, -a, -a, a]
  | .withdrawalRelease => [-a, a]
  | _ => [a, -a]

theorem posting_templates_balanced (k : Kind) (a : Int) : (postings k a).sum = 0 := by
  cases k <;> simp [postings] <;> omega

theorem positive_templates_accepted (k : Kind) (a : Int)
    (positive : 0 < a) (bounded : a ≤ 9223372036854775807) : accepted (postings k a) := by
  have ha : a ≠ 0 := by omega
  have hn : -a ≠ 0 := by omega
  cases k <;> simp [accepted, postings, checkedTotal, ha, hn, fitsI64] <;> omega

-- Gross transit is discharged; only the recipient's net amount becomes equity.
def allocationPostingsWithFee (gross fee : Int) : List Int :=
  [gross - fee, -gross, gross, -(gross - fee)]

theorem fee_allocation_balanced (gross fee : Int) :
    (allocationPostingsWithFee gross fee).sum = 0 := by
  simp only [allocationPostingsWithFee, List.sum_cons, List.sum_nil]
  omega

theorem fee_allocation_accepted (gross fee : Int)
    (positive : 0 < gross) (bounded : gross ≤ 9223372036854775807)
    (validFee : 0 ≤ fee ∧ fee < gross) : accepted (allocationPostingsWithFee gross fee) := by
  have hn : gross - fee ≠ 0 := by omega
  have hg : gross ≠ 0 := by omega
  have hng : -gross ≠ 0 := by omega
  have hnn : -(gross - fee) ≠ 0 := by omega
  simp [accepted, allocationPostingsWithFee, checkedTotal, hn, hg, hng, hnn, fitsI64]
  omega

-- Mathematical projection of successful checked_sub in user_balances.
def withdrawable (reserve holds : Nat) : Option Nat :=
  if holds ≤ reserve then some (reserve - holds) else none

theorem withdrawable_partition (reserve holds available : Nat)
    (h : withdrawable reserve holds = some available) :
    available + holds = reserve ∧ available ≤ reserve := by
  unfold withdrawable at h
  split at h
  · cases h; omega
  · contradiction

-- A mathematically balanced journal can still overflow in the Rust loop.
example : checkedTotal 0 [9223372036854775807, 1, -9223372036854775807, -1] = none := by
  decide

example : checkedTotal 0 [0] = none := by decide

end CanisterProofs.Ledger
