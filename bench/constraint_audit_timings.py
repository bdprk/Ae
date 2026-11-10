"""Measure the front-end cost of the ADR-40 capability audit.

The audit runs once per constrained generic declaration, before any call site.
This script compares two otherwise identical programs in the same session:

  with constraints     fn fN[T: Add](a: T, b: T) -> T { return a + b; }
  without constraints  fn fN[T](a: T, b: T) -> T { return a + b; } //<pad>

Byte counts are equalised with a trailing comment, so the lexer sees the same
number of characters. The remaining difference is two extra tokens plus the
audit itself, so the reported delta is an upper bound on the audit cost, not a
precise measurement. Both programs must emit byte-identical C: no generic is
instantiated, so the audit cannot change code generation.

Usage:
  python bench/constraint_audit_timings.py --runs 5 --functions 200 --json out.json
"""

import argparse
import json
import os
import pathlib
import statistics
import subprocess
import tempfile
import time

ROOT = pathlib.Path(__file__).resolve().parent.parent
EXE = ROOT / "target" / "debug" / ("ae.exe" if os.name == "nt" else "ae")


def make_source(count, constrained):
    lines = []
    for i in range(count):
        if constrained:
            lines.append(f"fn f{i}[T: Add](a: T, b: T) -> T {{ return a + b; }}")
        else:
            # ': Add' is five characters; pad the unconstrained line with a
            # five-character comment so both files have identical byte counts.
            lines.append(f"fn f{i}[T](a: T, b: T) -> T {{ return a + b; }} //  ")
    lines.append("fn main() { print(1); }")
    return ("\n".join(lines) + "\n").encode("utf-8")


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--runs", type=int, default=5)
    ap.add_argument("--functions", type=int, default=200)
    ap.add_argument("--json", dest="json_path")
    args = ap.parse_args()

    with tempfile.TemporaryDirectory(prefix="ae-audit-") as tmp:
        tmp = pathlib.Path(tmp)
        with_constraints = tmp / "with_constraints.ae"
        without_constraints = tmp / "without_constraints.ae"
        a = make_source(args.functions, True)
        b = make_source(args.functions, False)
        with_constraints.write_bytes(a)
        without_constraints.write_bytes(b)
        assert len(a) == len(b), (len(a), len(b))

        def once(path):
            started = time.perf_counter()
            result = subprocess.run([str(EXE), "check", str(path)], capture_output=True)
            elapsed = (time.perf_counter() - started) * 1000
            if result.returncode:
                raise RuntimeError(result.stderr.decode(errors="replace"))
            return elapsed

        pairs = []
        for run in range(args.runs):
            # Alternate the order so a warming process or a loaded machine does
            # not systematically favour one variant.
            if run % 2 == 0:
                x, y = once(with_constraints), once(without_constraints)
            else:
                y, x = once(without_constraints), once(with_constraints)
            pairs.append((round(x, 3), round(y, 3)))

        ca = subprocess.run([str(EXE), "emit-c", str(with_constraints)], capture_output=True, check=True).stdout
        cb = subprocess.run([str(EXE), "emit-c", str(without_constraints)], capture_output=True, check=True).stdout

    with_ms = [p[0] for p in pairs]
    without_ms = [p[1] for p in pairs]
    data = {
        "tool": "bench/constraint_audit_timings.py",
        "functions": args.functions,
        "source_bytes_each": len(a),
        "runs": args.runs,
        "with_constraints_ms": with_ms,
        "without_constraints_ms": without_ms,
        "median_with_ms": round(statistics.median(with_ms), 3),
        "median_without_ms": round(statistics.median(without_ms), 3),
        "median_delta_ms": round(statistics.median(with_ms) - statistics.median(without_ms), 3),
        "generated_c_bytes": len(ca),
        "generated_c_identical": ca == cb,
    }
    print(f"functions={args.functions} source_bytes={len(a)} runs={args.runs}")
    print(f"with constraints   ms: {with_ms} median={data['median_with_ms']}")
    print(f"without constraints ms: {without_ms} median={data['median_without_ms']}")
    print(f"delta (upper bound) ms: {data['median_delta_ms']}")
    print(f"generated C bytes={len(ca)} identical={ca == cb}")
    if args.json_path:
        out = pathlib.Path(args.json_path)
        out.parent.mkdir(parents=True, exist_ok=True)
        out.write_text(json.dumps(data, indent=2, sort_keys=True) + "\n", encoding="utf-8")
        print(f"wrote {out}")


if __name__ == "__main__":
    main()
