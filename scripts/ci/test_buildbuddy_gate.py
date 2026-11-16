import unittest
from unittest.mock import patch

import wait_buildbuddy as gate

SHA = "a" * 40
MERGE = "b" * 40
EVENT = {"pull_request": {"number": 381, "head": {"sha": SHA, "repo": {"full_name": "impalasys/talon"}},
                          "base": {"sha": "c" * 40}}}


def status(state="success", publisher=gate.PUBLISHER):
    return {"context": gate.CONTEXT, "state": state, "creator": {"login": publisher},
            "target_url": "https://impalasys.buildbuddy.io/invocation/12345678-1234-1234-1234-123456789abc"}


class GateTest(unittest.TestCase):
    def test_latest_pending_does_not_accept_old_success(self):
        with patch.object(gate, "api", return_value=[[status("pending"), status()]]):
            self.assertEqual(gate.latest_status("impalasys/talon", SHA)["state"], "pending")

    def test_spoofed_publisher_and_url_fail_closed(self):
        for row in (status(publisher="someone"), {**status(), "target_url": "https://example.com/invocation/x"}):
            with self.subTest(row=row), patch.object(gate, "api", return_value=[[row]]):
                with self.assertRaisesRegex(SystemExit, "Unexpected publisher"):
                    gate.latest_status("impalasys/talon", SHA)

    def test_exact_head_and_merge_tree_are_required(self):
        current = {"head": {"sha": SHA}, "base": {"sha": "c" * 40}}
        with patch.object(gate, "api", side_effect=[current, {"tree": {"sha": "x"}}, {"tree": {"sha": "y"}}]):
            with self.assertRaisesRegex(SystemExit, "differs from the GitHub merge tree"):
                gate.revision(EVENT, "impalasys/talon", MERGE)
        with patch.object(gate, "api", return_value={"head": {"sha": "d" * 40}, "base": current["base"]}):
            with self.assertRaisesRegex(SystemExit, "head/base changed"):
                gate.revision(EVENT, "impalasys/talon", MERGE)

    def test_success_rechecks_revision(self):
        with patch.object(gate, "revision", return_value=SHA) as check, \
                patch.object(gate, "latest_status", return_value=status()):
            self.assertEqual(gate.wait(EVENT, "impalasys/talon", MERGE, 0), 0)
            self.assertEqual(check.call_count, 2)

    def test_failures_and_missing_status_never_pass(self):
        for row in (None, status("pending"), status("failure"), status("error")):
            with self.subTest(row=row), patch.object(gate, "revision", return_value=SHA), \
                    patch.object(gate, "latest_status", return_value=row):
                with self.assertRaises(SystemExit):
                    gate.wait(EVENT, "impalasys/talon", MERGE, 0)

    def test_forks_never_wait_for_buildbuddy(self):
        with self.assertRaisesRegex(SystemExit, "Fork PRs"):
            gate.revision(EVENT, "another/repo", MERGE)


if __name__ == "__main__":
    unittest.main()
