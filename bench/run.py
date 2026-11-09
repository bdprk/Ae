"""Exploratory same-host benchmark; requires Python 3, cargo, gcc, go and rustc."""
import argparse
import hashlib
import json
import os
import platform
import shutil
import statistics
import subprocess
import tempfile
import time
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
BENCH = ROOT / "bench"
EXPECTED = b"499999500000"
REPEATS = 5

def call(args, **kwargs):
    return subprocess.run([str(x) for x in args], check=True, **kwargs)

def version(*args):
    return call(args, stdout=subprocess.PIPE, stderr=subprocess.STDOUT).stdout.decode(errors="replace").splitlines()[0]

def timed(args, env=None):
    start = time.perf_counter_ns()
    call(args, stdout=subprocess.PIPE, stderr=subprocess.PIPE, env=env)
    return (time.perf_counter_ns() - start) / 1_000_000

def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--release", action="store_true", help="use AE -O2 and Rust opt-level=2")
    parser.add_argument("--cold", action="store_true", help="give every build a fresh empty AE cache (measures real compiles)")
    options = parser.parse_args()
    call(["cargo", "build", "--release", "--locked"], cwd=ROOT)
    suffix = ".exe" if os.name == "nt" else ""
    compiler = ROOT / "target" / "release" / ("ae" + suffix)
    cc = os.environ.get("AE_CC", "gcc")
    report = {
        "note": "Exploratory builds with uncontrolled caches. AE uses GCC -O2 and Rust opt-level=2 in release, otherwise both use no optimization; Go uses its default. Flags do not imply equivalent optimization. Runtime includes startup, runs are grouped, CPU time and peak memory are not measured. AE builds reuse its content-addressed cache unless --cold gives every build a fresh empty cache.",
        "mode": ("cold-" if options.cold else "") + ("release" if options.release else "development"),
        "platform": platform.platform(), "machine": platform.machine(), "processor": platform.processor(),
        "versions": {"go": version("go", "version"), "rustc": version("rustc", "--version"), "cc": version(cc, "--version")},
        "inputs_sha256": {p.name: hashlib.sha256(p.read_bytes()).hexdigest() for p in (BENCH / "sum.ae", BENCH / "sum.go", BENCH / "sum.rs")},
        "repetitions": REPEATS, "results": {},
    }
    with tempfile.TemporaryDirectory(prefix="ae-bench-") as tmp:
        tmp = Path(tmp)
        for lang in ("ae", "go", "rust"):
            exe = tmp / (lang + suffix)
            if lang == "ae": args = [compiler, "build", BENCH / "sum.ae", "-o", exe] + (["--release"] if options.release else [])
            elif lang == "go": args = ["go", "build", "-o", exe, BENCH / "sum.go"]
            else: args = ["rustc", "-C", "opt-level=2" if options.release else "opt-level=0", BENCH / "sum.rs", "-o", exe]
            builds = []
            for _ in range(REPEATS):
                env = None
                if options.cold:
                    cold = tmp / f"cold-{lang}"
                    shutil.rmtree(cold, ignore_errors=True)
                    env = {**os.environ, "AE_CACHE": str(cold)}
                if exe.exists(): exe.unlink()
                builds.append(timed(args, env=env))
            checked = call([exe], stdout=subprocess.PIPE, stderr=subprocess.PIPE)
            if checked.stdout.strip() != EXPECTED:
                raise RuntimeError(f"{lang} incorrect output: {checked.stdout!r}")
            runs = [timed([exe]) for _ in range(REPEATS)]
            report["results"][lang] = {"build_ms": builds, "build_median_ms": statistics.median(builds), "run_ms": runs, "run_median_ms": statistics.median(runs), "executable_bytes": exe.stat().st_size}
    print(json.dumps(report, indent=2))

if __name__ == "__main__":
    main()
