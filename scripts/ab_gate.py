"""Run the M0.2.08 byte-parity gate: build a baseline from a git revision in
a scratch worktree, build the current tree, and compare every corpus entry.

This is the frontend-switchover acceptance gate (ADR-0229 slice 5): the
shared-tree consumption must change no generated C and no diagnostic. The
comparison itself lives in `compare_emit_c.py` (which normalizes line
endings, since a baseline built in a CRLF checkout and a candidate in an
LF checkout differ on every line without a semantic change).

Usage:
    python scripts/ab_gate.py --baseline <rev> [--skip-build]

Exits 0 only when every example entry point and CLI fixture invocation
matches byte for byte.
"""

import argparse
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
EXE = "ae.exe" if sys.platform == "win32" else "ae"


def run(args: list[str], cwd: Path, check: bool = True) -> subprocess.CompletedProcess:
    return subprocess.run(args, cwd=cwd, check=check, capture_output=True, text=True)


def build(where: Path, label: str) -> Path:
    """Build the release binary in `where` and return its path."""
    result = run(["cargo", "build", "--workspace", "--locked", "--release"], where, check=False)
    if result.returncode != 0:
        print(f"{label} build failed:\n{result.stderr[-2000:]}", file=sys.stderr)
        raise SystemExit(1)
    binary = where / "target" / "release" / EXE
    if not binary.exists():
        # The repository pins target-dir outside the checkout.
        metadata = run(["cargo", "metadata", "--no-deps", "--format-version", "1"], where)
        import json
        target = Path(json.loads(metadata.stdout)["target_directory"])
        binary = target / "release" / EXE
    if not binary.exists():
        print(f"{label} binary missing after build: {binary}", file=sys.stderr)
        raise SystemExit(1)
    print(f"{label}: {binary}")
    return binary


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--baseline", required=True, help="git revision to build the baseline from")
    parser.add_argument("--skip-build", action="store_true", help="reuse existing binaries")
    options = parser.parse_args()

    scratch = Path(tempfile.mkdtemp(prefix="ae-ab-"))
    baseline_tree = scratch / "baseline"
    print(f"baseline tree: {baseline_tree}")
    result = run(["git", "worktree", "add", "--detach", str(baseline_tree), options.baseline], ROOT, check=False)
    if result.returncode != 0:
        print(f"worktree add failed:\n{result.stderr}", file=sys.stderr)
        return 1

    try:
        if options.skip_build:
            baseline = baseline_tree / "target" / "release" / EXE
            candidate = ROOT / "target" / "release" / EXE
        else:
            baseline = build(baseline_tree, "baseline")
            candidate = build(ROOT, "candidate")

        gate = run(
            [sys.executable, "scripts/compare_emit_c.py",
             "--baseline", str(baseline), "--candidate", str(candidate), "--frontend-corpus"],
            ROOT, check=False,
        )
        sys.stdout.write(gate.stdout)
        sys.stderr.write(gate.stderr)
        return gate.returncode
    finally:
        run(["git", "worktree", "remove", "--force", str(baseline_tree)], ROOT, check=False)
        shutil.rmtree(scratch, ignore_errors=True)


if __name__ == "__main__":
    raise SystemExit(main())
