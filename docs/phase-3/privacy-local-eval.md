# Phase 3 correlation evaluation of synthetic public traces

Run date: 2026-09-24. This is a **local synthetic-data evaluation**, not real HL observation or load-test acceptance. Criteria from [privacy-evaluation.md](../phase-0/privacy-evaluation.md): top-1 ≤20%, ≥80% reduction versus A, and direct uniqueness ≤5% in groups of at least 20 users.

## Reproduction

Run `python3 tools/run_privacy_eval.py --output target/privacy-eval`. Generation, attack, and scoring run in separate processes; the attack receives only `public.json`. Ground truth is isolated in `private/truth.json` outside Git. Calibration seed: `20260924`; unseen evaluation seed: `20261001`. A/B0/B1/B1Exit receive the same deposits, timestamps, and account mappings for 30-day, 20/100-user scenarios. B1Exit uses B1 allocations but splits recovery and payouts on exit into different amounts delayed by 3–18 hours. Allocations remain within each user's deposited balance; each payout remains within that user's recovered amount at that point. These are synthetic simulations only.

Thirty days describe the input period; payouts for final-day exits may occur later.

The attack uses direct edges in the public graph, deposit/allocation amounts and timing, recovery/withdrawal amounts and timing, and summed split-exit amounts. Top-1 and Wilson 95% intervals are computed per user. This is screening with one attack implementation, not exclusion of other correlation techniques.

## Results

| Group | Arm | Top-1 | Wilson 95% | Reduction vs A | Direct uniqueness | Result |
|---|---|---:|---:|---:|---:|---|
| Calibration, 20 users | A | 100% | 83.9–100% | — | 100% | Control |
| Calibration, 20 users | B0 | 100% | 83.9–100% | 0% | 0% | Not met |
| Calibration, 20 users | B1 | 55% | 34.2–74.2% | 45% | 0% | Not met |
| Calibration, 20 users | B1Exit | 55% | 34.2–74.2% | 45% | 0% | Not met |
| Calibration, 100 users | A | 100% | 96.3–100% | — | 100% | Control |
| Calibration, 100 users | B0 | 100% | 96.3–100% | 0% | 0% | Not met |
| Calibration, 100 users | B1 | 51% | 41.4–60.6% | 49% | 0% | Not met |
| Calibration, 100 users | B1Exit | 48% | 38.5–57.7% | 52% | 0% | Not met |
| Unseen, 20 users | A | 100% | 83.9–100% | — | 100% | Control |
| Unseen, 20 users | B0 | 100% | 83.9–100% | 0% | 0% | Not met |
| Unseen, 20 users | B1 | 45% | 25.8–65.8% | 55% | 0% | Not met |
| Unseen, 20 users | B1Exit | 45% | 25.8–65.8% | 55% | 0% | Not met |
| Unseen, 100 users | A | 100% | 96.3–100% | — | 100% | Control |
| Unseen, 100 users | B0 | 100% | 96.3–100% | 0% | 0% | Not met |
| Unseen, 100 users | B1 | 49% | 39.4–58.7% | 51% | 0% | Not met |
| Unseen, 100 users | B1Exit | 48% | 38.5–57.7% | 52% | 0% | Not met |

Unseen B1 top-1 by scenario, for 20/100 users respectively: repeated deposits 0%/0%, small amounts 0%/0%, distinctive amounts 0%/0%, partial exit 100%/100%, full exit 100%/100%, and profit/loss 100%/100%. For B1Exit, all three unseen 20-user exit groups remain at 100%; unseen 100-user partial exit is 94.1%, full exit 100%, and profit/loss 100%. Splitting single exit transfers still permits matching through total amounts and time windows. Scenario counts and Wilson intervals are in `target/privacy-eval/summary.json`. Do not apply B1/B1Exit allocation or exit methods to production.

Real HL fees, finalization times, event granularity, balances, fills, and PnL are not incorporated. Evaluation including real observations remains incomplete, and these numbers must not be used as an anonymity guarantee.
