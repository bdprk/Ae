#!/usr/bin/env python3
"""Compare generated C byte for byte between two AE compiler binaries.

Build and preserve the baseline executable before changing the compiler. Build
and preserve the candidate separately, then invoke this script from any working
folder. Both binaries receive the exact same absolute source paths and run in
the repository root. A self-comparison checks the tool, not a refactor.
"""
import argparse
import hashlib
from pathlib import Path
import subprocess
import sys

ROOT = Path(__file__).resolve().parent.parent


def digest(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def emit(executable: Path, source: Path) -> subprocess.CompletedProcess:
    return subprocess.run([str(executable), "emit-c", str(source)], cwd=ROOT,
                          stdout=subprocess.PIPE, stderr=subprocess.PIPE, check=False)


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--baseline", type=Path, required=True, help="compiler built before the change")
    parser.add_argument("--candidate", type=Path, required=True, help="compiler built after the change")
    args = parser.parse_args()
    binaries = [args.baseline.resolve(), args.candidate.resolve()]
    for binary in binaries:
        if not binary.is_file():
            parser.error(f"compiler executable does not exist: {binary}")
    examples = sorted((ROOT / "examples").glob("*.ae"))
    module_main = ROOT / "examples" / "modules" / "main.ae"
    if not examples or not module_main.is_file():
        parser.error("example corpus or module entry point is missing")
    examples.append(module_main)
    print(f"baseline: {binaries[0]} sha256={digest(binaries[0].read_bytes())}")
    print(f"candidate: {binaries[1]} sha256={digest(binaries[1].read_bytes())}")
    failures = 0
    for source in examples:
        name = source.relative_to(ROOT).as_posix()
        try:
            before, after = (emit(binary, source) for binary in binaries)
        except OSError as error:
            print(f"ERROR {name}: {error}", file=sys.stderr)
            failures += 1
            continue
        if before.returncode != 0 or after.returncode != 0:
            print(f"ERROR {name}: baseline exit={before.returncode}, candidate exit={after.returncode}", file=sys.stderr)
            for label, result in (("baseline", before), ("candidate", after)):
                if result.returncode:
                    print(f"  {label}: {result.stderr.decode('utf-8', errors='replace').strip()}", file=sys.stderr)
            failures += 1
        elif before.stdout != after.stdout:
            print(f"DIFF {name}: baseline sha256={digest(before.stdout)}, candidate sha256={digest(after.stdout)}", file=sys.stderr)
            failures += 1
        else:
            print(f"OK {name} sha256={digest(before.stdout)}")
    print(f"Compared {len(examples)} example entry points; {failures} failed.")
    return 1 if failures else 0


if __name__ == "__main__":
    sys.exit(main())
