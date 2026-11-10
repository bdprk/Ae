#!/usr/bin/env python3
"""Measure the four build scenarios required by W3 and record C/object sizes.

    python3 bench/build_timings.py --json bench/results/<dir>/timings.json

Every scenario uses its own temporary cache and output path, so a "hit" is
never confused with a "miss". Timings are wall-clock milliseconds of the whole
`ae build` process (frontend + C compiler + link), which is what a user
actually waits for; `AE_TIMINGS=1` gives the per-stage breakdown.

The script does not compare against a "full runtime" baseline by itself. Pass
`--full-c PATH` with an `emit-c` dump from a build without runtime pruning to
also measure the C compiler's isolated cost on both texts.
"""
import argparse
import json
import os
import pathlib
import shutil
import statistics
import subprocess
import sys
import tempfile
import time

ROOT = pathlib.Path(__file__).resolve().parent.parent
EXE = ROOT / "target" / "debug" / ("ae.exe" if os.name == "nt" else "ae")
PLACEHOLDER = "fn main() { print(1); }"


def elapsed_ms(fn):
    start = time.perf_counter()
    result = fn()
    return (time.perf_counter() - start) * 1000.0, result


def median_ms(fn, runs):
    return statistics.median(elapsed_ms(fn)[0] for _ in range(runs))


def ae(args, cache, timings=False):
    env = dict(os.environ)
    env["AE_CACHE"] = str(cache)
    if timings:
        env["AE_TIMINGS"] = "1"
    else:
        env.pop("AE_TIMINGS", None)
    return subprocess.run([str(EXE), *args], env=env, capture_output=True)


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--json", dest="json_path", help="write the measurements here")
    ap.add_argument("--runs", type=int, default=3, help="repeats per timed scenario")
    ap.add_argument("--full-c", dest="full_c", help="unpruned emit-c dump for a C-compiler comparison")
    args = ap.parse_args()
    if not EXE.is_file():
        sys.exit(f"missing compiler: {EXE} (run cargo build --locked first)")

    tmp = pathlib.Path(tempfile.mkdtemp(prefix="ae-timings-"))
    data = {"tool": "bench/build_timings.py", "runs": args.runs, "scenarios": {}}
    source = tmp / "prog.ae"
    source.write_text(PLACEHOLDER, encoding="utf-8")
    cache = tmp / "cache"
    cc = os.environ.get("AE_CC", "gcc")

    def build(name, out_name, cold=False, timings=False):
        if cold and cache.exists():
            shutil.rmtree(cache)
        out = tmp / out_name
        if out.exists():
            out.unlink()

        def once():
            result = ae(["build", str(source), "-o", str(out)], cache, timings)
            if not result.returncode == 0:
                sys.exit(f"build failed: {result.stderr.decode(errors='replace')}")

        value, _ = elapsed_ms(once)
        data["scenarios"][name] = round(value, 3)
        return out

    try:
        # 1. Cold: nothing cached, runtime prefix compiled for the first time.
        build("cold_build_ms", "cold.exe", cold=True)
        # 2. No change: the whole-program content-addressed cache must hit.
        build("unchanged_build_ms", "same.exe")
        # 3. Warm but new program: cache has other entries, this C text is new.
        edited = source.read_text(encoding="utf-8").replace("print(1)", "print(2)")
        source.write_text(edited, encoding="utf-8")
        build("implementation_edit_ms", "edit.exe")
        # 4. Interface change: a public signature changes, so no entry matches.
        source.write_text(
            "pub fn answer(v: i64) -> i64 { return v + 1; }\n"
            "fn main() { print(answer(41)); }\n",
            encoding="utf-8",
        )
        build("interface_edit_ms", "iface.exe")

        # C text and executable sizes for the resource budget.
        source.write_text(PLACEHOLDER, encoding="utf-8")
        emitted = ae(["emit-c", str(source)], cache)
        if emitted.returncode == 0:
            data["generated_c_bytes"] = len(emitted.stdout)
            cfile = tmp / "program.c"
            cfile.write_bytes(emitted.stdout)
            binary = tmp / "emit.exe"
            result = subprocess.run([cc, "-std=c11", "-O0", str(cfile), "-o", str(binary)], capture_output=True)
            if result.returncode == 0:
                data["c_compile_ms"] = round(median_ms(lambda: subprocess.run(
                    [cc, "-std=c11", "-O0", str(cfile), "-o", str(binary)], capture_output=True), args.runs), 3)
                data["executable_bytes"] = binary.stat().st_size
            else:
                data["c_compile_error"] = result.stderr.decode(errors="replace")[:400]
            if args.full_c:
                full = pathlib.Path(args.full_c)
                if full.is_file():
                    data["full_runtime_c_bytes"] = full.stat().st_size
                    data["full_runtime_c_compile_ms"] = round(median_ms(lambda: subprocess.run(
                        [cc, "-std=c11", "-O0", str(full), "-o", str(binary)], capture_output=True), args.runs), 3)
                else:
                    data["full_runtime_c_error"] = f"not found: {full}"

        out = json.dumps(data, indent=2, sort_keys=True)
        print(out)
        if args.json_path:
            dest = pathlib.Path(args.json_path)
            dest.parent.mkdir(parents=True, exist_ok=True)
            dest.write_text(out + "\n", encoding="utf-8")
    finally:
        shutil.rmtree(tmp, ignore_errors=True)


if __name__ == "__main__":
    main()
