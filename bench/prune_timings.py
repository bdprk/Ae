"""Measure the ADR-50 comment-pruning change against the previous compiler.

Before ADR-50 `runtime_units` kept every `//` comment in the always-retained
"non-function" text, and `ae_symbols` scans that text for `ae_*` names —
including names that only appear in a comment. A comment mentioning a runtime
function therefore rooted it, so every generated program carried it. Pruning now
takes the comment with the function below it.

The script compares two real compilers in one session:

* generated C bytes for a minimal program and a string program,
* `gcc -c` time on both generated files (alternating, medians),
* end-to-end `ae build --release` time with a fresh cache each run.

Build the baseline binary from the commit before ADR-50, for example:
  mkdir -p target/probe/old-prune
  git archive <pre-adr50-commit> src Cargo.toml Cargo.lock | tar -x -C target/probe/old-prune
  cd target/probe/old-prune
  CARGO_TARGET_DIR=$PWD/../../old-prune-target cargo build --locked

Usage:
  python bench/prune_timings.py --old-exe <path> --runs 7 --json out.json
"""

import argparse
import json
import os
import pathlib
import shutil
import statistics
import subprocess
import tempfile
import time

ROOT = pathlib.Path(__file__).resolve().parent.parent
DEFAULT_NEW = ROOT / "target" / "debug" / ("ae.exe" if os.name == "nt" else "ae")

PROGRAMS = {
    "print": "fn main() { print(1); }\n",
    "strings": 'fn main() { let s = "hi"; print(s); print(len(s)); }\n',
    "map": None,  # filled from bench/map.ae
}


def run(command, **kwargs):
    result = subprocess.run(command, stdout=subprocess.PIPE, stderr=subprocess.PIPE, **kwargs)
    if result.returncode != 0:
        raise RuntimeError(f"{command[0]} failed: {result.stderr.decode('utf-8', 'replace')[:400]}")
    return result.stdout


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--old-exe", required=True)
    parser.add_argument("--new-exe", default=str(DEFAULT_NEW))
    parser.add_argument("--runs", type=int, default=7)
    parser.add_argument("--json", default=None)
    options = parser.parse_args()

    programs = dict(PROGRAMS)
    programs["map"] = (ROOT / "bench" / "map.ae").read_text(encoding="utf-8")
    data = {"runs": options.runs, "programs": {}}

    with tempfile.TemporaryDirectory(prefix="ae-prune-") as tmp_dir:
        tmp = pathlib.Path(tmp_dir)
        for name, source in programs.items():
            src = tmp / f"{name}.ae"
            src.write_text(source, encoding="utf-8")
            sizes = {}
            c_files = {}
            for tag, exe in (("old", options.old_exe), ("new", options.new_exe)):
                ctext = run([exe, "emit-c", str(src)])
                c_path = tmp / f"{name}-{tag}.c"
                c_path.write_bytes(ctext)
                sizes[tag] = len(ctext)
                c_files[tag] = c_path
            # Alternate the order so load drift cancels out.
            gcc_times = {"old": [], "new": []}
            for round_index in range(options.runs):
                order = ("old", "new") if round_index % 2 == 0 else ("new", "old")
                for tag in order:
                    started = time.perf_counter()
                    subprocess.run(["gcc", "-std=c11", "-O0", "-c", str(c_files[tag]),
                                    "-o", str(tmp / f"{name}-{tag}.o")], check=True)
                    gcc_times[tag].append((time.perf_counter() - started) * 1000.0)
            data["programs"][name] = {
                "generated_c_bytes": sizes,
                "generated_c_delta": sizes["new"] - sizes["old"],
                "generated_c_ratio": round(sizes["new"] / sizes["old"], 4),
                "gcc_ms": {tag: round(statistics.median(v), 3) for tag, v in gcc_times.items()},
                "gcc_raw": {tag: [round(x, 2) for x in v] for tag, v in gcc_times.items()},
            }
            saved = data["programs"][name]["gcc_ms"]["old"] - data["programs"][name]["gcc_ms"]["new"]
            print(f"{name:8s} C bytes {sizes['old']:6d} -> {sizes['new']:6d} "
                  f"({data['programs'][name]['generated_c_ratio']:.3f}x)  "
                  f"gcc {data['programs'][name]['gcc_ms']['old']:7.2f} -> "
                  f"{data['programs'][name]['gcc_ms']['new']:7.2f} ms (saved {saved:6.2f})")

        # End-to-end cold build: a fresh cache forces the front end and gcc.
        hello = ROOT / "examples" / "hello.ae"
        build_times = {"old": [], "new": []}
        for round_index in range(options.runs):
            order = ("old", "new") if round_index % 2 == 0 else ("new", "old")
            for tag in order:
                exe = options.old_exe if tag == "old" else options.new_exe
                cache = tmp / f"cache-{tag}-{round_index}"
                shutil.rmtree(cache, ignore_errors=True)
                cache.mkdir(parents=True)
                out = tmp / f"hello-{tag}-{round_index}.exe"
                env = {**os.environ, "AE_CACHE": str(cache)}
                started = time.perf_counter()
                subprocess.run([exe, "build", str(hello), "-o", str(out), "--release"],
                               env=env, check=True, stdout=subprocess.DEVNULL)
                build_times[tag].append((time.perf_counter() - started) * 1000.0)
        data["cold_build_ms"] = {tag: round(statistics.median(v), 3) for tag, v in build_times.items()}
        data["cold_build_raw"] = {tag: [round(x, 2) for x in v] for tag, v in build_times.items()}
        saved = data["cold_build_ms"]["old"] - data["cold_build_ms"]["new"]
        print(f"cold build --release: {data['cold_build_ms']['old']:.2f} -> "
              f"{data['cold_build_ms']['new']:.2f} ms (saved {saved:.2f}, "
              f"{saved * 100 / data['cold_build_ms']['old']:.1f}%)")

    if options.json:
        pathlib.Path(options.json).write_text(json.dumps(data, indent=2), encoding="utf-8")
        print(f"wrote {options.json}")


if __name__ == "__main__":
    main()
