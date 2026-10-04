# Privacy evaluation inputs, attacker-visible information, and acceptance criteria

- Basis: `Plan.md` 16.6 (U29), 8.3, roadmap section 5
- Status: design contract. Evaluation is planned for Phase 1 (`Implementation.md` 1-11); this phase fixes inputs and acceptance criteria.

## 1. Goal and limits

- Compare public-trace correlation resistance of A/B0/B1 using identical inputs to choose a funding route.
- Numbers are **engineering screening criteria**, not mathematical anonymity guarantees. No guarantees for isolated or low usage.
- Route B starts safety verification; it is not a complete amount/time hiding scheme. Immediate same-amount allocation B0 remains a **failure control**. Do not hide this or call the prototype a privacy product.
- Do not count synthetic users as an anonymity population or inflate production usage with synthetic data.

## 2. Comparison arms

| Arm | Behavior | Role |
|---|---|---|
| A | Direct owner HL→trading deposit | Baseline with direct transfer link |
| B0 | Shared reserve followed immediately by same-amount allocation | Failure control retaining amount/time correlation |
| B1 | Simulated separation of allocation timing and batching | Candidate; results determine adoption |

B1 must not lend others' funds without consent. Always retain holds/backing and do not fix an unproven scheme as production behavior.

## 3. Inputs

| Item | Requirement |
|---|---|
| Users | 1 / 5 / 20 / 100; acceptance groups require at least 20 |
| Period | Equivalent to 30 days |
| Scenarios | Repeated deposits/withdrawals, small amounts, distinctive amounts, partial/full exit, profit/loss |
| Seed | Fixed, recorded, reproducible |
| Split | Separate attack tuning and unseen evaluation; do not expose truth mappings to attacks |
| Real observation | Add testnet round-trip fees, finalization times, and event granularity absent from simulation |
| Artifacts | Input generator, private evaluation-only mappings, public-trace-only attack inputs |

- Attackers cannot access truth mappings; compare them only during scoring.
- Do not inflate accuracy by treating repeated events as independent samples. Report per-user uncertainty intervals.

## 4. Attacker-visible information

### 4.1 Primary evaluator: public observer

- Entire public transfer graph: sources, destinations, amounts, times.
- Public HL account information: balances, positions, fills, transfer history.
- Matching amounts, fees, and timing.
- Repeated trading patterns.
- PnL.

### 4.2 Observers evaluated separately

| Observer | Visible information | Reason for separate treatment |
|---|---|---|
| Hyperliquid | Accounts, orders, fund transfers | Cannot be hidden while using HL |
| Entry provider such as Cloudflare | Source IP, timing, delivered content | UI delivery trust boundary outside guard delay |
| TEE/SNS authorities | Decrypted state, upgrade authority | Separate trust assumptions |
| Fund-layer upgrade authority | Ledger and mappings | Separate compromise-impact assessment |

## 5. Development acceptance criteria

Each group with at least 20 users must meet:

| Metric | Target |
|---|---|
| Top-1 matching success | ≤20% |
| Success reduction versus A | ≥80% |
| Directly unique traceability | ≤5% |

- Report averages and each repeated/small/distinctive/partial-exit/full-exit/profit-loss scenario.
- Report independent per-user uncertainty intervals.
- Passing does not establish mathematical anonymity, privacy under isolated/low usage, secrecy from HL/entry providers/authorities, or secrecy of past data.

## 6. Report format

- Success tables by arm, user group, and scenario, with averages/intervals.
- Concrete failure conditions: matching amount/time, repetition, exit behavior.
- Public information added through real testnet observations and its effect on simulation results.
- Separate unseen evaluation results from attack-tuning inputs.
- Reproduction: seed, generator revision, environment, observation sources/dates.

## 7. Decisions when criteria fail

| Condition | Decision |
|---|---|
| Privacy fails | Redesign allocation; do not silently combine trading accounts or switch to agent-only |
| Safety fails | Stop fund integration and fix state machines/authorization |
| Confidential infrastructure unavailable | Continue synthetic/testnet checks in an ordinary environment only |
| Admission→HL acceptance p95 >5 s | Preserve confidentiality; test limit-order-focused use and improve on testnet |

- Do not start new confidential deposits while criteria remain unmet.
- Do not indefinitely delay existing withdrawals for anonymity. Warn before execution if withdrawal weakens protection.
- Do not invest heavily in route-specific UI/production implementation while the funding route remains undecided.

## 8. Deliverables

- Reproducible simulation: input generator, attack, scoring, revisions.
- Public-trace evaluation inputs and private evaluation-only truth mappings.
- Real testnet observation records.
- Correlation report in section 6 format.
- Adoption decision for B1 and redesign policy after failed criteria.

## 9. Pending decisions

| Item | Timing |
|---|---|
| Candidate B1 algorithms | Phase 1 before evaluation |
| Attack scope: graph/time/amount fingerprints | Phase 1 |
| Additional public information from testnet | Phase 1 |
| Production usage requirements and independent reviewers | Phase 5 |
