"""Measure the ADR-41 front-end change against the previous compiler.

ADR-40's `GenericFn` cloned the whole token stream and source text once per
generic declaration, so a file with N declarations paid O(N^2). ADR-41 shares
both per file with `Rc`. This script compares two real binaries in the same
session, on the same source, alternating the order to neutralise load drift.

It also asserts that both binaries emit byte-identical C and produce the same
declaration-site diagnostic, so a timing win cannot hide a semantic change.

Build the baseline binary from the commit before ADR-41, for example:
  mkdir -p target/probe/old-tree
  git archive <pre-adr41-commit> src Cargo.toml Cargo.lock | tar -x -C target/probe/old-tree
  cd target/probe/old-tree
  CARGO_TARGET_DIR=$PWD/../../old-target cargo build --locked

Usage:
  python bench/shared_tokens_timings.py --old-exe <path> --runs 5 \
      --functions 800 --json out.json
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
DEFAULT_NEW = ROOT / "target" / "debug" / ("ae.exe" if os.name == "nt" else "ae")


def make_source(count):
    lines = [f"fn f{i}[T: Add](a: T, b: T) -> T {{ return a + b; }}" for i in range(count)]
    lines.append("fn main() { print(1); }")
    return ("\n".join(lines) + "\n").encode("utf-8")


WEAK = b"fn f[T: Copy](a: T, b: T) -> T { return a + b; }\nfn main() { print(1); }\n"


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--old-exe", required=True)
    ap.add_argument("--new-exe", default=str(DEFAULT_NEW))
    ap.add_argument("--runs", type=int, default=5)
    ap.add_argument("--functions", type=int, default=800)
    ap.add_argument("--json", dest="json_path")
    args = ap.parse_args()

    old, new = pathlib.Path(args.old_exe), pathlib.Path(args.new_exe)
    assert old.is_file(), old
    assert new.is_file(), new

    with tempfile.TemporaryDirectory(prefix="ae-shared-") as tmp:
        tmp = pathlib.Path(tmp)
        source = tmp / "many.ae"
        weak = tmp / "weak.ae"
        source.write_bytes(make_source(args.functions))
        weak.write_bytes(WEAK)

        def check(exe, path):
            started = time.perf_counter()
            result = subprocess.run([str(exe), "check", str(path)], capture_output=True)
            return (time.perf_counter() - started) * 1000, result

        def emit(exe, path):
            result = subprocess.run([str(exe), "emit-c", str(path)], capture_output=True, check=True)
            return result.stdout

        # Semantic equivalence: same generated C (CRLF/LF normalised, since a
        # baseline tree exported with `git archive` can carry different line
        # endings) and the same declaration-site diagnostic.
        c_old, c_new = emit(old, source), emit(new, source)
        normalize = lambda data: data.replace(b"\r\n", b"\n")
        identical_c = normalize(c_old) == normalize(c_new)
        identical_raw = c_old == c_new
        d_old, d_new = check(old, weak)[1], check(new, weak)[1]
        same_diagnostic = (
            d_old.returncode == d_new.returncode
            and d_old.stderr.replace(b"\r\n", b"\n") == d_new.stderr.replace(b"\r\n", b"\n")
        )
        assert d_old.returncode != 0 and d_new.returncode != 0, "weak constraint must be rejected"

        pairs = []
        for run in range(args.runs):
            if run % 2 == 0:
                o, n = check(old, source)[0], check(new, source)[0]
            else:
                n, o = check(new, source)[0], check(old, source)[0]
            pairs.append((round(o, 3), round(n, 3)))

    old_ms = [p[0] for p in pairs]
    new_ms = [p[1] for p in pairs]
    med_old, med_new = statistics.median(old_ms), statistics.median(new_ms)
    data = {
        "tool": "bench/shared_tokens_timings.py",
        "functions": args.functions,
        "source_bytes": len(make_source(args.functions)),
        "runs": args.runs,
        "old_exe": str(old),
        "new_exe": str(new),
        "old_check_ms": old_ms,
        "new_check_ms": new_ms,
        "median_old_ms": round(med_old, 3),
        "median_new_ms": round(med_new, 3),
        "median_saved_ms": round(med_old - med_new, 3),
        "median_ratio": round(med_old / med_new, 3) if med_new else None,
        "generated_c_bytes": len(c_new),
        "generated_c_identical": identical_c,
        "generated_c_identical_raw": identical_raw,
        "declaration_diagnostic_identical": same_diagnostic,
    }
    print(f"functions={args.functions} source_bytes={data['source_bytes']} runs={args.runs}")
    print(f"old (per-declaration clone) ms: {old_ms} median={data['median_old_ms']}")
    print(f"new (shared Rc)            ms: {new_ms} median={data['median_new_ms']}")
    print(f"saved={data['median_saved_ms']} ms ratio={data['median_ratio']}")
    print(f"generated C bytes={len(c_new)} identical (normalised)={identical_c} identical (raw)={identical_raw} diagnostic identical={same_diagnostic}")
    assert identical_c and same_diagnostic, "semantic output changed"
    if args.json_path:
        out = pathlib.Path(args.json_path)
        out.parent.mkdir(parents=True, exist_ok=True)
        out.write_text(json.dumps(data, indent=2, sort_keys=True) + "\n", encoding="utf-8")
        print(f"wrote {out}")


if __name__ == "__main__":
    main()
