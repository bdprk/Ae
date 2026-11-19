"""Tests for the emit-c comparison gate."""
import contextlib
import importlib.util
import io
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch

spec = importlib.util.spec_from_file_location("compare_emit_c", Path(__file__).with_name("compare_emit_c.py"))
compare = importlib.util.module_from_spec(spec)
spec.loader.exec_module(compare)


class ComparisonTest(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        (self.root / "examples" / "modules").mkdir(parents=True)
        (self.root / "examples" / "one.ae").write_text("example", encoding="utf-8")
        (self.root / "examples" / "modules" / "main.ae").write_text("module", encoding="utf-8")
        # Resolved here because compare_emit_c.main() resolves the paths
        # before passing them to emit: the temp directory's short-name form
        # differs from the resolved one on Windows, and the emit stubs tell
        # the two binaries apart by path equality.
        self.baseline = (self.root / "before").resolve()
        self.candidate = (self.root / "after").resolve()
        self.baseline.write_bytes(b"before")
        self.candidate.write_bytes(b"after")

    def run_comparison(self, emit):
        args = ["compare_emit_c.py", "--baseline", str(self.baseline), "--candidate", str(self.candidate)]
        with patch.object(compare, "ROOT", self.root), patch.object(compare, "emit", side_effect=emit), \
             patch.object(sys, "argv", args), contextlib.redirect_stdout(io.StringIO()), \
             contextlib.redirect_stderr(io.StringIO()):
            return compare.main()

    def test_equal_bytes_pass(self):
        result = subprocess.CompletedProcess([], 0, b"identical\r\n", b"")
        self.assertEqual(self.run_comparison(lambda *_: result), 0)

    def test_even_line_ending_difference_fails(self):
        def emit(binary, source):
            return subprocess.CompletedProcess([], 0, b"same\r\n" if binary == self.baseline else b"same\n", b"")
        self.assertEqual(self.run_comparison(emit), 1)

    def test_compiler_failure_does_not_pass_as_matching_output(self):
        def emit(binary, source):
            return subprocess.CompletedProcess([], 1, b"", b"failed")
        self.assertEqual(self.run_comparison(emit), 1)

    def test_cli_corpus_detects_exit_and_output_differences(self):
        for suite in ("ui", "ui-json", "run"):
            directory = self.root / "tests" / suite
            directory.mkdir(parents=True)
            (directory / "case.ae").write_text("fixture", encoding="utf-8")

        def invoke(command, **_options):
            release = "--release" in command
            candidate = command[0] == str(self.candidate)
            return subprocess.CompletedProcess(
                command, 1 if release and candidate else 0,
                b"changed" if release and candidate else b"expected", b"",
            )

        errors = io.StringIO()
        with patch.object(compare, "ROOT", self.root), patch.object(compare.subprocess, "run", side_effect=invoke), \
             contextlib.redirect_stderr(errors):
            self.assertEqual(compare.compare_cli_corpus([self.baseline, self.candidate]), (4, 1))
        self.assertIn("tests/run/case.ae --release: returncode, stdout", errors.getvalue())


if __name__ == "__main__":
    unittest.main()
