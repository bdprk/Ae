"""Measure the ADR-49 fused `split`/`lines` arena against the previous runtime.

Before ADR-49 `ae_split` allocated one buffer per token (`ae_alloc` in
`ae_str_from_len`) on top of the pointer array's doubling growth. Text
workloads on 2.5 MB paid ~450,000 allocations. ADR-49 counts tokens first, then
allocates one exact-size pointer array and one byte arena, and points every
element into the arena.

The script compares two real binaries in the same session on the same input and
the same sources, alternating the order each round to neutralise load drift. It
also asserts identical stdout, so a timing win cannot hide a behaviour change.

Build the baseline binary before ADR-49, for example:
  mkdir -p target/probe/old-arena
  git archive <pre-adr49-commit> src Cargo.toml Cargo.lock | tar -x -C target/probe/old-arena
  cp src/runtime.c src/runtime_ex.c target/probe/old-arena/src/   # workspace line endings
  cd target/probe/old-arena
  CARGO_TARGET_DIR=$PWD/../../old-arena-target cargo build --release --locked

Usage:
  python bench/split_timings.py --old-exe <path> --runs 7 --json out.json
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
DEFAULT_NEW = ROOT / "target" / "release" / ("ae.exe" if os.name == "nt" else "ae")

WORDS = ["alpha", "beta", "gamma", "delta", "epsilon", "zeta", "eta", "theta"]
LINES = 50000

# Three shapes over the same input: read only, split without iterating, and the
# benchmark's own text workload. The differences attribute cost to the split.
SOURCES = {
    "read": """
fn main() {
    let parts = args();
    let text = read_file(parts[1]);
    print(len(text));
}
""",
    "count": """
fn main() {
    let parts = args();
    let text = read_file(parts[1]);
    let tokens = split(text, " ");
    print(len(text));
    print(len(tokens));
}
""",
    "full": """
fn main() {
    let parts = args();
    let text = read_file(parts[1]);
    let tokens = split(text, " ");
    var inside = 0;
    for token in tokens { inside = inside + len(token); }
    print(len(text));
    print(len(tokens));
    print(inside);
}
""",
}


def write_input(path):
    with open(path, "w", encoding="ascii", newline="\n") as handle:
        for i in range(LINES):
            row = " ".join(WORDS[(i + j) % len(WORDS)] for j in range(8))
            handle.write(f"{row} {i}\n")


def build(compiler, source, exe):
    subprocess.run([str(compiler), "build", str(source), "-o", str(exe), "--release"], check=True)


def run(exe, input_path):
    started = time.perf_counter()
    result = subprocess.run([str(exe), str(input_path)], stdout=subprocess.PIPE, stderr=subprocess.PIPE)
    elapsed = (time.perf_counter() - started) * 1000.0
    if result.returncode != 0:
        raise RuntimeError(f"{exe} failed: {result.stderr.decode(errors='replace')}")
    return elapsed, result.stdout


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--old-exe", required=True)
    parser.add_argument("--new-exe", default=str(DEFAULT_NEW))
    parser.add_argument("--runs", type=int, default=7)
    parser.add_argument("--json", default=None)
    options = parser.parse_args()

    data = {"runs": options.runs, "lines": LINES, "workloads": {}}
    with tempfile.TemporaryDirectory(prefix="ae-split-") as tmp:
        tmp = pathlib.Path(tmp)
        input_path = tmp / "input.txt"
        write_input(input_path)
        data["input_bytes"] = input_path.stat().st_size
        for name, text in SOURCES.items():
            source = tmp / f"{name}.ae"
            source.write_text(text, encoding="utf-8")
            exes = {"old": tmp / f"{name}-old.exe", "new": tmp / f"{name}-new.exe"}
            build(options.old_exe, source, exes["old"])
            build(options.new_exe, source, exes["new"])
            times = {"old": [], "new": []}
            outputs = {}
            for round_index in range(options.runs):
                # Alternate which binary goes first so drift cancels out.
                order = ("old", "new") if round_index % 2 == 0 else ("new", "old")
                for which in order:
                    elapsed, stdout = run(exes[which], input_path)
                    times[which].append(elapsed)
                    outputs[which] = stdout
            if outputs["old"] != outputs["new"]:
                raise RuntimeError(f"{name}: old and new disagreed: {outputs['old']!r} vs {outputs['new']!r}")
            old_ms = statistics.median(times["old"])
            new_ms = statistics.median(times["new"])
            data["workloads"][name] = {
                "raw_old_ms": times["old"],
                "raw_new_ms": times["new"],
                "median_old_ms": round(old_ms, 3),
                "median_new_ms": round(new_ms, 3),
                "saved_ms": round(old_ms - new_ms, 3),
                "ratio": round(old_ms / new_ms, 3) if new_ms > 0 else None,
            }
            print(f"{name:6s} old={old_ms:7.2f} ms new={new_ms:7.2f} ms "
                  f"saved={old_ms - new_ms:6.2f} ms ratio={old_ms / new_ms:5.2f}x")
        read = data["workloads"]["read"]["median_new_ms"]
        count = data["workloads"]["count"]["median_new_ms"]
        full = data["workloads"]["full"]["median_new_ms"]
        data["new_split_cost_ms"] = round(count - read, 3)
        data["new_iteration_cost_ms"] = round(full - count, 3)
        print(f"new split cost (count-read) = {count - read:.2f} ms; iteration (full-count) = {full - count:.2f} ms")
        if options.json:
            pathlib.Path(options.json).write_text(json.dumps(data, indent=2), encoding="utf-8")
            print(f"wrote {options.json}")


if __name__ == "__main__":
    main()
