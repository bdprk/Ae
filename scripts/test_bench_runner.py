"""Regression tests for benchmark output and separate peak-memory passes."""
import importlib.util
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch

ROOT = Path(__file__).resolve().parent.parent
spec = importlib.util.spec_from_file_location("benchmark_runner", ROOT / "bench" / "run.py")
bench = importlib.util.module_from_spec(spec)
spec.loader.exec_module(bench)


class BenchmarkRunnerTest(unittest.TestCase):
    def test_timed_returns_actual_output(self):
        with patch.object(bench, "peak_mb", return_value=None):
            sample = bench.timed([sys.executable, "-c", "print('actual output')"])
        self.assertEqual(sample["stdout"].replace(b"\r\n", b"\n"), b"actual output\n")

    def test_incorrect_timed_output_aborts_the_workload(self):
        sample = {"wall_ms": 1.0, "cpu_ms": None, "peak_mb": None, "stdout": b"incorrect\n"}
        with tempfile.TemporaryDirectory() as tmp, patch.object(bench, "build_commands", return_value=None), \
             patch.object(bench, "timed", return_value=sample):
            with self.assertRaisesRegex(RuntimeError, "timed output disagreed"):
                bench.run_workload("sum", Path(tmp), Path("compiler"), object())

    def test_peak_pass_uses_separate_output_and_propagates_failure(self):
        with tempfile.TemporaryDirectory() as tmp:
            first, second = Path(tmp) / "first", Path(tmp) / "second"
            code = "import pathlib,sys; pathlib.Path(sys.argv[1]).open('x').close()"
            bench.timed([sys.executable, "-c", code, first], peak_args=[sys.executable, "-c", code, second])
            self.assertTrue(first.is_file() and second.is_file())
            with self.assertRaises(subprocess.CalledProcessError):
                bench.peak_mb([sys.executable, "-c", "import sys; sys.exit(7)"])


if __name__ == "__main__":
    unittest.main()
