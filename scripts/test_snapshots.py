"""Tests for the snapshot runner's selection and update safeguards."""
import importlib.util
import os
from pathlib import Path
import subprocess
import unittest
from unittest.mock import patch

spec = importlib.util.spec_from_file_location("snapshot_runner", Path(__file__).with_name("snapshots.py"))
runner = importlib.util.module_from_spec(spec)
spec.loader.exec_module(runner)


class SnapshotRunnerTest(unittest.TestCase):
    def test_default_never_inherits_bless(self):
        with patch.dict(os.environ, {"AE_BLESS": "1"}), patch.object(runner.subprocess, "run", return_value=subprocess.CompletedProcess([], 0)) as run:
            self.assertEqual(runner.main([]), 0)
        self.assertNotIn("AE_BLESS", run.call_args.kwargs["env"])
        self.assertEqual(run.call_args.args[0], ["cargo", "test", "--locked", "--test", "snapshots"])

    def test_bless_selects_exact_suite_and_propagates_failure(self):
        with patch.object(runner.subprocess, "run", return_value=subprocess.CompletedProcess([], 7)) as run:
            self.assertEqual(runner.main(["--suite", "ui", "--bless"]), 7)
        self.assertEqual(run.call_args.kwargs["env"]["AE_BLESS"], "1")
        self.assertEqual(run.call_args.args[0][-3:], ["ui_snapshots", "--", "--exact"])


if __name__ == "__main__":
    unittest.main()
