"""Compile-and-run test for the ADR-0214 thread + allocator stress harness.

The harness (scripts/thread_harness.c) includes the runtime prefix directly,
spawns eight OS threads through `ae_thread_spawn`, drives every allocation
through the checked entry points under concurrency, and asserts the cumulative
budget counter lands on the exact deterministic total. A skipped compiler
(entry point missing) fails loudly rather than passing vacuously: the harness
is the evidence for the atomic-accounting decision and must run where gcc
exists. Skips only when no C compiler is installed at all.
"""
import shutil
import subprocess
import tempfile
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
HARNESS = ROOT / "scripts" / "thread_harness.c"


def find_compiler():
    for candidate in ("gcc", "cc", "clang"):
        if shutil.which(candidate):
            return candidate
    return None


class ThreadHarnessTest(unittest.TestCase):
    def test_allocator_accounting_stays_exact_under_threads(self):
        compiler = find_compiler()
        if compiler is None:
            self.fail("no C compiler found (gcc/cc/clang); the thread harness cannot run")
        with tempfile.TemporaryDirectory() as tmp:
            binary = Path(tmp) / "thread_harness"
            compile_result = subprocess.run(
                [compiler, "-std=c11", "-O2", str(HARNESS), "-o", str(binary)],
                capture_output=True, timeout=120)
            self.assertEqual(compile_result.returncode, 0,
                             f"compile failed:\n{compile_result.stderr.decode(errors='replace')}")
            run_result = subprocess.run([str(binary)], capture_output=True, timeout=120)
            self.assertEqual(run_result.returncode, 0,
                             f"run failed ({run_result.returncode}):\n"
                             f"{run_result.stdout.decode(errors='replace')}\n"
                             f"{run_result.stderr.decode(errors='replace')}")
            self.assertTrue(run_result.stdout.decode().startswith("OK threads=8 cycles=4000"),
                            run_result.stdout.decode(errors="replace"))


if __name__ == "__main__":
    unittest.main()
