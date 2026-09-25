#!/usr/bin/env python3
"""Fixed-seed, public-trace-only A/B0/B1 correlation screening.

Run `generate`, `attack`, and `evaluate` as separate commands. The attack command
accepts only the public trace, never the private answer key. Outputs are synthetic
and must not be described as observed Hyperliquid privacy.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import math
import random
from pathlib import Path

SCENARIOS = (
    "repeated",
    "small",
    "distinctive",
    "partial_exit",
    "full_exit",
    "pnl",
)
ARMS = ("A", "B0", "B1", "B1Exit")
DAY = 86_400


def write_json(path: Path, data: object) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(data, sort_keys=True, indent=2) + "\n", encoding="utf-8")


def read_json(path: Path) -> dict:
    return json.loads(path.read_text(encoding="utf-8"))


def event(source: str, destination: str, amount: int, at: int, kind: str) -> dict:
    return {"from": source, "to": destination, "usdc_micro": amount, "at": at, "kind": kind}


def rng_for(seed: int, user: int, purpose: str) -> random.Random:
    digest = hashlib.sha256(f"{seed}/{user}/{purpose}".encode()).digest()
    return random.Random(int.from_bytes(digest, "big"))


def generate(count: int, seed: int, arm: str) -> tuple[dict, dict]:
    if count not in (1, 5, 20, 100) or arm not in ARMS:
        raise ValueError("count must be 1/5/20/100 and arm A/B0/B1/B1Exit")
    rng = random.Random(seed)
    wallets = [f"wallet-{i:03d}" for i in range(count)]
    trading = [f"trading-{i:03d}" for i in range(count)]
    rng.shuffle(trading)
    truth = dict(zip(wallets, trading, strict=True))
    scenario = {wallet: SCENARIOS[i % len(SCENARIOS)] for i, wallet in enumerate(wallets)}
    events: list[dict] = []
    backing: dict[str, int] = {wallet: 0 for wallet in wallets}
    for i, wallet in enumerate(wallets):
        user_rng = rng_for(seed, i, "input")
        delay_rng = rng_for(seed, i, "B1-delay")
        trader = truth[wallet]
        label = scenario[wallet]
        deposits = 4 if label == "repeated" else 2
        base = (20 + user_rng.randrange(200)) * 1_000_000
        if label == "small":
            base = 1_000_000 + user_rng.randrange(500_000)
        if label == "distinctive":
            base += i * 7919 + 37
        for j in range(deposits):
            at = (2 + j * 6) * DAY + user_rng.randrange(DAY)
            amount = base + (0 if label == "repeated" else j * 1_234_567)
            if arm == "A":
                events.append(event(wallet, trader, amount, at, "deposit"))
            else:
                events.append(event(wallet, "custody", amount, at, "deposit"))
                backing[wallet] += amount
                if arm == "B0":
                    allocations = [(amount, at + 60)]
                else:
                    # Only this user's credited balance is allocated. Splitting and delay
                    # are evaluated in simulation; production fund flow is untouched.
                    first = amount * (35 + delay_rng.randrange(30)) // 100
                    allocations = [
                        (first, at + delay_rng.randrange(6 * 3600, 24 * 3600)),
                        (amount - first, at + delay_rng.randrange(24 * 3600, 48 * 3600)),
                    ]
                for allocated, alloc_at in allocations:
                    assert backing[wallet] >= allocated
                    backing[wallet] -= allocated
                    events.append(event("custody", trader, allocated, alloc_at, "allocation"))
        if label in ("partial_exit", "full_exit", "pnl"):
            at = 28 * DAY + user_rng.randrange(DAY)
            principal = deposits * base + (0 if label == "repeated" else sum(j * 1_234_567 for j in range(deposits)))
            withdrawn = principal // 2 if label == "partial_exit" else principal
            if label == "pnl":
                withdrawn += 3_250_000
            if arm == "A":
                events.append(event(trader, wallet, withdrawn, at, "withdrawal"))
            elif arm == "B1Exit":
                # Comparison only: different recovery and payout sizes, spaced apart.
                # Each payout uses only this wallet's own recovered balance.
                recovered_first = withdrawn * 43 // 100
                paid_first = withdrawn * 29 // 100
                assert 0 < paid_first <= recovered_first < withdrawn
                schedule = [
                    event(trader, "custody", recovered_first, at, "recovery"),
                    event("custody", wallet, paid_first, at + 3 * 3600, "withdrawal"),
                    event(trader, "custody", withdrawn - recovered_first,
                          at + 6 * 3600, "recovery"),
                    event("custody", wallet, withdrawn - paid_first,
                          at + 18 * 3600, "withdrawal"),
                ]
                held = 0
                for item in schedule:
                    held += item["usdc_micro"] if item["kind"] == "recovery" else -item["usdc_micro"]
                    assert held >= 0
                assert held == 0
                events.extend(schedule)
            else:
                events.append(event(trader, "custody", withdrawn, at, "recovery"))
                events.append(event("custody", wallet, withdrawn, at + (90 if arm == "B0" else delay_rng.randrange(2 * 3600, DAY)), "withdrawal"))
    events.sort(key=lambda row: (row["at"], row["from"], row["to"]))
    public = {"arm": arm, "users": count, "duration_days": 30, "wallets": wallets,
              "trading_accounts": sorted(trading), "events": events}
    private = {"arm": arm, "users": count, "seed": seed, "truth": truth, "scenario": scenario,
               "backing_after_allocation": backing}
    return public, private


def attack(public: dict) -> dict:
    """Use graph, amount, and time only; no answer key or seed enters this function."""
    events = public["events"]
    predictions: dict[str, str] = {}
    directly_unique: dict[str, bool] = {}
    for wallet in public["wallets"]:
        direct = [row["to"] for row in events if row["from"] == wallet
                  and row["to"] in public["trading_accounts"]]
        directly_unique[wallet] = len(set(direct)) == 1 and bool(direct)
        if direct:
            predictions[wallet] = direct[0]
            continue
        incoming = [row for row in events if row["from"] == wallet and row["to"] == "custody"]
        outgoing = [row for row in events if row["from"] == "custody" and row["to"] == wallet]
        scores = {}
        for trader in public["trading_accounts"]:
            allocations = [row for row in events if row["from"] == "custody" and row["to"] == trader]
            recoveries = [row for row in events if row["from"] == trader and row["to"] == "custody"]
            score = 0.0
            for deposit in incoming:
                candidates = [row for row in allocations if 0 <= row["at"] - deposit["at"] <= 3 * DAY]
                if candidates:
                    best = max(candidates, key=lambda row:
                               math.exp(-abs(row["usdc_micro"] - deposit["usdc_micro"]) / max(deposit["usdc_micro"] * .05, 1))
                               * math.exp(-(row["at"] - deposit["at"]) / (12 * 3600)))
                    score += math.exp(-abs(best["usdc_micro"] - deposit["usdc_micro"]) / max(deposit["usdc_micro"] * .05, 1))
                    score += .1 * math.exp(-(best["at"] - deposit["at"]) / (12 * 3600))
            for withdrawal in outgoing:
                score += sum(2.0 for recovery in recoveries
                             if recovery["usdc_micro"] == withdrawal["usdc_micro"]
                             and 0 <= withdrawal["at"] - recovery["at"] <= DAY)
            # Splitting exact payments is insufficient when the whole exit can be
            # aggregated from public amounts and a short time window.
            if outgoing:
                earliest = min(row["at"] for row in outgoing)
                nearby = [row for row in recoveries if earliest - DAY <= row["at"] <= earliest + DAY]
                if sum(row["usdc_micro"] for row in nearby) == sum(row["usdc_micro"] for row in outgoing):
                    score += 3.0
            scores[trader] = score
        predictions[wallet] = max(scores, key=lambda trader: (scores[trader], trader))
    return {"arm": public["arm"], "users": public["users"], "predictions": predictions,
            "directly_unique": directly_unique}


def wilson(successes: int, count: int) -> tuple[float, float]:
    if count == 0:
        return (0.0, 0.0)
    z = 1.96
    p = successes / count
    denominator = 1 + z * z / count
    centre = (p + z * z / (2 * count)) / denominator
    radius = z * math.sqrt(p * (1 - p) / count + z * z / (4 * count * count)) / denominator
    return (round(centre - radius, 4), round(centre + radius, 4))


def evaluate(private: dict, result: dict, baseline_success: float | None) -> dict:
    if private["arm"] != result["arm"] or private["users"] != result["users"]:
        raise ValueError("mismatched evaluation inputs")
    truth = private["truth"]
    predictions = result["predictions"]
    if set(predictions) != set(truth):
        raise ValueError("missing or extra predictions")
    hits = {wallet: predictions[wallet] == trader for wallet, trader in truth.items()}
    count = len(truth)
    success = sum(hits.values()) / count
    direct = sum(bool(result["directly_unique"][wallet]) for wallet in truth) / count
    by_scenario = {}
    for label in SCENARIOS:
        members = [wallet for wallet in truth if private["scenario"][wallet] == label]
        if members:
            correct = sum(hits[wallet] for wallet in members)
            by_scenario[label] = {"users": len(members), "top1": round(correct / len(members), 4),
                                  "wilson95": wilson(correct, len(members))}
    reduction = None if baseline_success is None else 1 - success / baseline_success
    eligible = count >= 20
    passes = eligible and success <= .20 and direct <= .05 and reduction is not None and reduction >= .80
    return {"arm": private["arm"], "users": count, "top1": round(success, 4),
            "wilson95": wilson(sum(hits.values()), count), "directly_unique": round(direct, 4),
            "reduction_from_A": None if reduction is None else round(reduction, 4),
            "scenario": by_scenario, "pass": passes if eligible else None,
            "backing_nonnegative": all(value >= 0 for value in private["backing_after_allocation"].values())}


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    sub = parser.add_subparsers(dest="command", required=True)
    gen = sub.add_parser("generate")
    gen.add_argument("--users", type=int, required=True)
    gen.add_argument("--seed", type=int, required=True)
    gen.add_argument("--arm", choices=ARMS, required=True)
    gen.add_argument("--public", type=Path, required=True)
    gen.add_argument("--private", type=Path, required=True)
    atk = sub.add_parser("attack")
    atk.add_argument("--public", type=Path, required=True)
    atk.add_argument("--out", type=Path, required=True)
    ev = sub.add_parser("evaluate")
    ev.add_argument("--private", type=Path, required=True)
    ev.add_argument("--attack", type=Path, required=True)
    ev.add_argument("--baseline-top1", type=float)
    ev.add_argument("--out", type=Path, required=True)
    args = parser.parse_args()
    if args.command == "generate":
        if args.public.resolve() == args.private.resolve():
            raise ValueError("public and private paths must differ")
        public, private = generate(args.users, args.seed, args.arm)
        write_json(args.public, public)
        write_json(args.private, private)
        print(f"public_sha256={hashlib.sha256(args.public.read_bytes()).hexdigest()}")
    elif args.command == "attack":
        write_json(args.out, attack(read_json(args.public)))
    else:
        report = evaluate(read_json(args.private), read_json(args.attack), args.baseline_top1)
        write_json(args.out, report)
        print(json.dumps({key: report[key] for key in ("arm", "users", "top1", "directly_unique", "reduction_from_A", "pass")}))


if __name__ == "__main__":
    main()
