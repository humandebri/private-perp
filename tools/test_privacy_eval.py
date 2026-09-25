import unittest

from privacy_eval import attack, evaluate, generate


class PrivacyEvalTests(unittest.TestCase):
    def test_same_inputs_and_nonnegative_backing_across_arms(self):
        generated = {arm: generate(20, 20261001, arm) for arm in ("A", "B0", "B1")}
        truths = [item[1]["truth"] for item in generated.values()]
        self.assertEqual(truths[0], truths[1])
        self.assertEqual(truths[1], truths[2])
        deposits = [
            [(row["from"], row["usdc_micro"], row["at"]) for row in public["events"] if row["kind"] == "deposit"]
            for public, _ in generated.values()
        ]
        self.assertEqual(deposits[0], deposits[1])
        self.assertEqual(deposits[1], deposits[2])
        self.assertTrue(all(amount >= 0 for amount in generated["B1"][1]["backing_after_allocation"].values()))

    def test_failure_control_is_detected(self):
        a_public, a_private = generate(20, 20261001, "A")
        baseline = evaluate(a_private, attack(a_public), None)["top1"]
        b0_public, b0_private = generate(20, 20261001, "B0")
        report = evaluate(b0_private, attack(b0_public), baseline)
        self.assertFalse(report["pass"])
        self.assertEqual(report["top1"], 1.0)

    def test_exit_split_keeps_per_user_backing_and_splits_both_sides(self):
        public, private = generate(20, 20261001, "B1Exit")
        for wallet, trader in private["truth"].items():
            recoveries = [row for row in public["events"] if row["from"] == trader and row["kind"] == "recovery"]
            payouts = [row for row in public["events"] if row["to"] == wallet and row["kind"] == "withdrawal"]
            if private["scenario"][wallet] not in ("partial_exit", "full_exit", "pnl"):
                self.assertFalse(recoveries)
                self.assertFalse(payouts)
                continue
            self.assertEqual(len(recoveries), 2)
            self.assertEqual(len(payouts), 2)
            self.assertEqual(sum(row["usdc_micro"] for row in recoveries),
                             sum(row["usdc_micro"] for row in payouts))
            self.assertTrue(all(r["usdc_micro"] != p["usdc_micro"] for r in recoveries for p in payouts))


if __name__ == "__main__":
    unittest.main()
