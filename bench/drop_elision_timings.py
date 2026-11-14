#!/usr/bin/env python3
"""Exploratory same-host timings for M2.4 drop elaboration (ADR-0204).

Two questions, measured separately:

1. Runtime: does eliding provably-dead releases show up in wall-clock time?
   Two workloads: `alloc` allocates three owned strings per loop iteration
   and moves two (allocation dominates); `straight` is a generated,
   allocation-light function of N declare-then-move pairs, where the only
   work besides the moves is the dead releases themselves.

2. Structure: how many release call sites does the generated C carry for
   the same source through the MIR backend (elision on, the default) versus
   the tree backend (AE_CODEGEN=tree, conservative lists shipped)?

The same source runs through both backends; binaries are built once and run
interleaved over R rounds; every run's stdout is validated against the first.
The report states medians and ranges, not superiority.

Usage: python bench/drop_elision_timings.py [--mode alloc|straight] [--rounds 15] [--iters 1000000] [--pairs 20000]
"""

import argparse
import json
import os
import statistics
import subprocess
import sys
import tempfile
import time
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
EXE = ROOT / "target" / "debug" / ("ae.exe" if os.name == "nt" else "ae")

ALLOC_SOURCE = """fn sink(x: string) -> i64 { return len(x); }
fn hold(y: &string) -> i64 { return len(y); }
fn main() {
    var total = 0;
    var i = 0;
    while i < AE_BENCH_ITERS {
        let a = "alpha-" + to_string(i);
        let b = "beta-" + to_string(i);
        let c = "gamma-" + to_string(i);
        total = total + sink(a) + sink(b) + hold(c);
        i = i + 1;
    }
    print(total);
}
"""


def straight_source(pairs: int) -> str:
    # Allocation-light: each binding holds a short literal; the move hands it
    # to a by-value call. The tree backend ships a dead release per moved
    # binding; the MIR backend ships none. N bindings, no loop, no allocation
    # beyond the N one-time string constructions.
    lines = ["fn sink(x: string) -> i64 { return len(x); }", "fn main() {", "    var total = 0;"]
    for index in range(pairs):
        lines.append(f'    let s{index} = "s{index}";')
        lines.append(f"    total = total + sink(s{index});")
    lines.append("    print(total);")
    lines.append("}")
    return "\n".join(lines)


def run(args, **kwargs):
    result = subprocess.run(args, capture_output=True, text=True, **kwargs)
    if result.returncode != 0:
        raise RuntimeError(f"command failed: {args}\n{result.stderr}")
    return result


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--mode", choices=["alloc", "straight"], default="alloc")
    parser.add_argument("--rounds", type=int, default=15)
    parser.add_argument("--iters", type=int, default=1_000_000)
    parser.add_argument("--pairs", type=int, default=20_000)
    options = parser.parse_args()

    if not EXE.exists():
        print(f"missing {EXE}; run cargo build -p ae first", file=sys.stderr)
        return 1

    workdir = Path(tempfile.mkdtemp(prefix="ae-drop-bench-"))
    source = workdir / "dropload.ae"
    if options.mode == "alloc":
        source.write_text(ALLOC_SOURCE.replace("AE_BENCH_ITERS", str(options.iters)), encoding="utf-8")
    else:
        source.write_text(straight_source(options.pairs), encoding="utf-8")

    binaries = {}
    release_sites = {}
    for tag, tree in (("mir", False), ("tree", True)):
        env = {**os.environ, "AE_CODEGEN": "tree"} if tree else None
        emitted = run([str(EXE), "emit-c", str(source)], env=env)
        main_body = emitted.stdout.split("int main(", 1)[-1]
        release_sites[tag] = main_body.count("_release(&")
        run([str(EXE), "build", str(source), "-o", str(workdir / f"dropload_{tag}"), "--debug"], env=env)
        binaries[tag] = workdir / f"dropload_{tag}"

    expected = None
    samples = {"mir": [], "tree": []}
    for _ in range(options.rounds):
        for tag in ("mir", "tree"):
            started = time.perf_counter()
            result = subprocess.run([str(binaries[tag])], capture_output=True, text=True)
            elapsed = time.perf_counter() - started
            if result.returncode != 0:
                raise RuntimeError(f"run failed: {result.stderr}")
            if expected is None:
                expected = result.stdout
            elif result.stdout != expected:
                raise RuntimeError("backends disagree on stdout")
            samples[tag].append(elapsed)

    report = {
        "mode": options.mode,
        "iters": options.iters,
        "pairs": options.pairs,
        "rounds": options.rounds,
        "stdout": expected.strip(),
        "release_sites_in_main": release_sites,
        "median_seconds": {tag: statistics.median(values) for tag, values in samples.items()},
        "min_seconds": {tag: min(values) for tag, values in samples.items()},
        "max_seconds": {tag: max(values) for tag, values in samples.items()},
        "all_samples": samples,
    }
    print(report["stdout"])
    for tag in ("mir", "tree"):
        print(f"{tag}: median {report['median_seconds'][tag]:.4f}s "
              f"(min {report['min_seconds'][tag]:.4f}, max {report['max_seconds'][tag]:.4f}) "
              f"release_sites_in_main {release_sites[tag]}")
    (workdir / "results.json").write_text(json.dumps(report, indent=2), encoding="utf-8")
    print(f"raw results: {(workdir / 'results.json')}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
