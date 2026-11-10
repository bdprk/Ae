"""Exploratory same-host benchmark; requires Python 3, cargo, gcc, go and rustc.

Wall-clock time per call is recorded everywhere; on platforms that expose
getrusage (POSIX) a summed direct-child CPU time is recorded as well. Peak
resident memory is not measured. Repetitions are interleaved across languages
within each round (not randomized). Cold/hot refers only to the AE
content-addressed cache: with --cold every AE build gets a fresh empty
cache directory; the external toolchains' own caches are uncontrolled.
"""
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

try:
    import resource as _resource  # POSIX; absent on Windows
except ImportError:
    _resource = None

ROOT = Path(__file__).resolve().parent.parent
BENCH = ROOT / "bench"
EXPECTED = b"499999500000"
REPEATS = 5


def call(args, **kwargs):
    return subprocess.run([str(x) for x in args], check=True, **kwargs)


def version(*args):
    return call(args, stdout=subprocess.PIPE, stderr=subprocess.STDOUT).stdout.decode(errors="replace").splitlines()[0]


def child_cpu_ms():
    """Summed (user+system) CPU of terminated direct children, in ms.

    Returns None where the platform has no getrusage (Windows). Scope: direct
    children only, so grandchildren that e.g. gcc spawns (cc1, as, ld) are
    not included; this is not the child's CPU time in an OS-profiler sense.
    """
    if _resource is None:
        return None
    r = _resource.getrusage(_resource.RUSAGE_CHILDREN)
    return (r.ru_utime + r.ru_stime) * 1000.0


def timed(args, env=None):
    cpu_before = child_cpu_ms()
    start = time.perf_counter_ns()
    call(args, stdout=subprocess.PIPE, stderr=subprocess.PIPE, env=env)
    wall_ms = (time.perf_counter_ns() - start) / 1_000_000
    cpu_after = child_cpu_ms()
    cpu_ms = None if cpu_before is None or cpu_after is None else max(0.0, cpu_after - cpu_before)
    return {"wall_ms": wall_ms, "cpu_ms": cpu_ms}


def _median(values):
    values = [v for v in values if v is not None]
    return statistics.median(values) if values else None


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--release", action="store_true", help="use AE -O2 and Rust opt-level=2")
    parser.add_argument("--cold", action="store_true", help="give every build a fresh empty AE cache (measures real compiles)")
    options = parser.parse_args()
    call(["cargo", "build", "--release", "--locked"], cwd=ROOT)
    suffix = ".exe" if os.name == "nt" else ""
    compiler = ROOT / "target" / "release" / ("ae" + suffix)
    cc = os.environ.get("AE_CC", "gcc")
    cpu_method = None if _resource is None else "resource.getrusage(RUSAGE_CHILDREN) deltas between calls, direct children only"
    report = {
        "note": ("Exploratory same-host measurements with external toolchain caches uncontrolled. AE uses GCC -O2 "
                 "and Rust opt-level=2 in release, otherwise both use no optimization; Go uses its default. Flags do "
                 "not imply equivalent optimization. Runtime includes startup; repetitions are interleaved across "
                 "languages per round, not randomized; temperature and frequency not recorded. Peak memory is not "
                 "measured. AE builds reuse its content-addressed cache unless --cold gives every build a fresh "
                 "empty cache dir."),
        "cpu_method": cpu_method,
        "mode": ("cold-" if options.cold else "") + ("release" if options.release else "development"),
        "platform": platform.platform(), "machine": platform.machine(), "processor": platform.processor(),
        "versions": {"go": version("go", "version"), "rustc": version("rustc", "--version"), "cc": version(cc, "--version")},
        "inputs_sha256": {p.name: hashlib.sha256(p.read_bytes()).hexdigest() for p in (BENCH / "sum.ae", BENCH / "sum.go", BENCH / "sum.rs")},
        "repetitions": REPEATS, "results": {},
    }
    with tempfile.TemporaryDirectory(prefix="ae-bench-") as tmp:
        tmp = Path(tmp)
        langs = ("ae", "go", "rust")
        exe = {lang: tmp / (lang + suffix) for lang in langs}
        build_args = {}
        for lang in langs:
            if lang == "ae":
                build_args[lang] = [compiler, "build", BENCH / "sum.ae", "-o", exe[lang]] + (["--release"] if options.release else [])
            elif lang == "go":
                build_args[lang] = ["go", "build", "-o", exe[lang], BENCH / "sum.go"]
            else:
                build_args[lang] = ["rustc", "-C", "opt-level=2" if options.release else "opt-level=0", BENCH / "sum.rs", "-o", exe[lang]]
        builds = {lang: [] for lang in langs}
        runs = {lang: [] for lang in langs}
        for _ in range(REPEATS):
            for lang in langs:
                env = None
                if options.cold:
                    cold = tmp / f"cold-{lang}"
                    shutil.rmtree(cold, ignore_errors=True)
                    env = {**os.environ, "AE_CACHE": str(cold)}
                if exe[lang].exists():
                    exe[lang].unlink()
                builds[lang].append(timed(build_args[lang], env=env))
            for lang in langs:
                runs[lang].append(timed([exe[lang]]))
        for lang in langs:
            checked = call([exe[lang]], stdout=subprocess.PIPE, stderr=subprocess.PIPE)
            if checked.stdout.strip() != EXPECTED:
                raise RuntimeError(f"{lang} incorrect output: {checked.stdout!r}")
            b, r = builds[lang], runs[lang]
            report["results"][lang] = {
                "build_ms": [x["wall_ms"] for x in b],
                "build_cpu_ms": [x["cpu_ms"] for x in b],
                "build_median_ms": statistics.median(x["wall_ms"] for x in b),
                "build_cpu_median_ms": _median(x["cpu_ms"] for x in b),
                "run_ms": [x["wall_ms"] for x in r],
                "run_cpu_ms": [x["cpu_ms"] for x in r],
                "run_median_ms": statistics.median(x["wall_ms"] for x in r),
                "run_cpu_median_ms": _median(x["cpu_ms"] for x in r),
                "executable_bytes": exe[lang].stat().st_size,
            }
    print(json.dumps(report, indent=2))


if __name__ == "__main__":
    main()
