"""Exploratory same-host benchmark; requires Python 3, cargo, gcc, rustc and go.

Wall-clock time per call is recorded everywhere; on platforms that expose
getrusage (POSIX) a summed direct-child CPU time is recorded as well. Peak
resident memory is sampled from the OS high-water mark of the direct child
(Windows `PeakWorkingSetSize`, Linux `/proc/<pid>/status` `VmHWM`), which is the
true peak as long as at least one sample lands before exit; a child that exits
between two 1 ms samples reports no value. For a build, only the direct child is
sampled, so a compiler driver that spawns cc1/as/ld (gcc) or rustc's codegen
children are not covered. Repetitions are interleaved across languages within
each round (not randomized). Cold/hot refers only to the AE
content-addressed cache: with --cold every AE build gets a fresh empty cache
directory; the external toolchains' own caches are uncontrolled.

Four workloads are available:

* `sum` — a million checked additions; the languages are AE, Go and Rust.
* `text` — read a generated 3 MiB text file, split it on spaces, and report the
  byte count, token count and summed token length. Python is included here
  because text processing is the case the design plan compares against Python,
  with the caveat that its runtime includes interpreter start-up and that its
  "build" is nothing at all. This workload materialises the tokens
  (`split` / `strings.Split` / `collect`).
* `textlazy` — the same statistics over the same input, but scanning the bytes
  once and never materialising the tokens: AE's `tokenize` cursor, Go's
  manual index scan, Rust's `split(' ')`. Having both entries keeps the two API
  shapes separate instead of crediting one language for the other's algorithm
  (ADR-53). Python is excluded because its `str.split` is eager.
* `map` — a fixed-seed LCG feeds a million draws into a 10000-key
  `map[string, i64]` histogram, then a count-weighted checksum reads every
  bucket and `len` reports how many keys were reached. Python is included
  because dict counting is the container workload the design plan compares
  against Python. The checksum cannot be computed from the draw or key count
  alone, so every draw and every lookup is exercised.
"""
import argparse
import hashlib
import json
import os
import platform
import shutil
import statistics
import subprocess
import sys
import tempfile
import time
from pathlib import Path

try:
    import resource as _resource  # POSIX; absent on Windows
except ImportError:
    _resource = None

ROOT = Path(__file__).resolve().parent.parent
BENCH = ROOT / "bench"
REPEATS = 5
SUM_EXPECTED = b"499999500000"
TEXT_LINES = 50000
EXTENSION = {"ae": "ae", "go": "go", "rust": "rs", "python": "py"}


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


def peak_bytes(pid):
    """Peak resident set of a live process, in bytes, or None if unavailable."""
    if os.name == "nt":
        import ctypes
        from ctypes import wintypes

        class Counters(ctypes.Structure):
            _fields_ = [
                ("cb", wintypes.DWORD), ("PageFaultCount", wintypes.DWORD),
                ("PeakWorkingSetSize", ctypes.c_size_t), ("WorkingSetSize", ctypes.c_size_t),
                ("QuotaPeakPagedPoolUsage", ctypes.c_size_t), ("QuotaPagedPoolUsage", ctypes.c_size_t),
                ("QuotaPeakNonPagedPoolUsage", ctypes.c_size_t), ("QuotaNonPagedPoolUsage", ctypes.c_size_t),
                ("PagefileUsage", ctypes.c_size_t), ("PeakPagefileUsage", ctypes.c_size_t),
            ]

        process = ctypes.windll.kernel32.OpenProcess(0x1000, False, pid)  # QUERY_LIMITED_INFORMATION
        if not process:
            return None
        counters = Counters()
        counters.cb = ctypes.sizeof(Counters)
        ok = ctypes.windll.psapi.GetProcessMemoryInfo(process, ctypes.byref(counters), counters.cb)
        ctypes.windll.kernel32.CloseHandle(process)
        return counters.PeakWorkingSetSize if ok else None
    try:
        with open(f"/proc/{pid}/status", "rb") as status:
            for line in status:
                if line.startswith(b"VmHWM:"):
                    return int(line.split()[1]) * 1024
    except OSError:
        return None
    return None


def peak_mb(args, env=None):
    """Peak resident set of one run, in MB, or None when it cannot be sampled.

    This runs the command a second time on purpose. Sampling inside the timed
    loop used to distort every measurement: on Windows ``time.sleep(0.001)``
    sleeps for the 15.6 ms timer tick, so a 4 ms program was reported as 20-45 ms
    and the bias hit the fastest program hardest (ADR-53). Wall time and peak
    memory are now separate passes.
    """
    with tempfile.TemporaryFile() as stderr:
        process = subprocess.Popen([str(x) for x in args], stdout=subprocess.DEVNULL,
                                   stderr=stderr, env=env)
        peak = None
        while process.poll() is None:
            value = peak_bytes(process.pid)
            if value is not None and (peak is None or value > peak):
                peak = value
            time.sleep(0.001)
        process.wait()
        if process.returncode != 0:
            stderr.seek(0)
            raise subprocess.CalledProcessError(process.returncode, args, stderr=stderr.read())
        return None if peak is None else peak / (1024 * 1024)


def timed(args, env=None, peak_args=None, peak_env=None):
    cpu_before = child_cpu_ms()
    start = time.perf_counter_ns()
    result = subprocess.run([str(x) for x in args], capture_output=True, env=env)
    wall_ms = (time.perf_counter_ns() - start) / 1_000_000
    cpu_after = child_cpu_ms()
    if result.returncode != 0:
        raise subprocess.CalledProcessError(result.returncode, args, result.stdout, result.stderr)
    cpu_ms = None if cpu_before is None or cpu_after is None else max(0.0, cpu_after - cpu_before)
    peak = peak_mb(peak_args if peak_args is not None else args,
                   peak_env if peak_env is not None else env)
    return {"wall_ms": wall_ms, "cpu_ms": cpu_ms, "peak_mb": peak, "stdout": result.stdout}


def _median(values):
    values = [v for v in values if v is not None]
    return statistics.median(values) if values else None


def write_text_input(path):
    """Deterministic ASCII input: a fixed word cycle plus the line number."""
    words = ["alpha", "beta", "gamma", "delta", "epsilon", "zeta", "eta", "theta"]
    with open(path, "w", encoding="ascii", newline="\n") as handle:
        for i in range(TEXT_LINES):
            row = " ".join(words[(i + j) % len(words)] for j in range(8))
            handle.write(f"{row} {i}\n")


def build_commands(workload, lang, exe, compiler, options):
    source = BENCH / f"{workload}.{EXTENSION[lang]}"
    if lang == "ae":
        return [compiler, "build", source, "-o", exe] + (["--release"] if options.release else [])
    if lang == "go":
        return ["go", "build", "-o", exe, source]
    if lang == "rust":
        return ["rustc", "-C", "opt-level=2" if options.release else "opt-level=0", source, "-o", exe]
    return None


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--release", action="store_true", help="use AE -O2 and Rust opt-level=2")
    parser.add_argument("--cold", action="store_true", help="give every build a fresh empty AE cache (measures real compiles)")
    parser.add_argument("--workload", choices=("sum", "text", "textlazy", "map", "all"), default="all")
    options = parser.parse_args()
    call(["cargo", "build", "--release", "--locked"], cwd=ROOT)
    suffix = ".exe" if os.name == "nt" else ""
    compiler = ROOT / "target" / "release" / ("ae" + suffix)
    cc = os.environ.get("AE_CC", "gcc")
    cpu_method = None if _resource is None else "resource.getrusage(RUSAGE_CHILDREN) deltas between calls, direct children only"
    workloads = ("sum", "text", "textlazy", "map") if options.workload == "all" else (options.workload,)
    report = {
        "note": ("Exploratory same-host measurements with external toolchain caches uncontrolled. AE uses GCC -O2 "
                 "and Rust opt-level=2 in release, otherwise both use no optimization; Go uses its default; Python is "
                 "not compiled and its run time includes interpreter start-up. Flags do not imply equivalent "
                 "optimization. Runtime includes process start-up; repetitions are interleaved across languages per "
                 "round, not randomized; temperature and frequency not recorded. Peak memory is the OS high-water mark "
                 "of the direct child, sampled every millisecond while it runs, so a child that exits between samples "
                 "reports nothing and a build that spawns grandchildren (gcc, rustc codegen) reports only the driver's "
                 "peak. AE builds reuse its content-addressed cache unless --cold gives every build a fresh empty "
                 "cache dir. Every timed run's output is checked against the workload expectation or its round's AE output. "
                 "Build peak memory uses a separate output path and, for cold AE builds, a separate empty cache."),
        "cpu_method": cpu_method,
        "peak_method": ("Windows GetProcessMemoryInfo PeakWorkingSetSize" if os.name == "nt"
                        else "Linux /proc/<pid>/status VmHWM") + " sampled in a separate run from the timed one",
        "mode": ("cold-" if options.cold else "") + ("release" if options.release else "development"),
        "platform": platform.platform(), "machine": platform.machine(), "processor": platform.processor(),
        "versions": {"go": version("go", "version"), "rustc": version("rustc", "--version"),
                     "cc": version(cc, "--version"), "python": platform.python_version()},
        "repetitions": REPEATS, "results": {},
    }
    with tempfile.TemporaryDirectory(prefix="ae-bench-") as tmp:
        tmp = Path(tmp)
        text_input = tmp / "input.txt"
        write_text_input(text_input)
        report["inputs"] = {"text_input_bytes": text_input.stat().st_size, "text_input_lines": TEXT_LINES,
                            "sha256": {p.name: hashlib.sha256(p.read_bytes()).hexdigest()
                                       for p in sorted(BENCH.glob("sum.*")) + sorted(BENCH.glob("text.*"))
                                       + sorted(BENCH.glob("map.*"))}}
        for workload in workloads:
            report["results"][workload] = run_workload(workload, tmp, compiler, options)
    print(json.dumps(report, indent=2))


def run_workload(workload, tmp, compiler, options):
    langs = ("ae", "go", "rust") if workload in ("sum", "textlazy") else ("ae", "go", "rust", "python")
    expected = SUM_EXPECTED if workload == "sum" else None
    suffix = ".exe" if os.name == "nt" else ""
    exe = {lang: tmp / (workload + "-" + lang + suffix) for lang in langs}
    build_args = {lang: build_commands(workload, lang, exe[lang], compiler, options) for lang in langs}
    run_args = {}
    for lang in langs:
        if lang == "python":
            run_args[lang] = [sys.executable, BENCH / f"{workload}.py"]
        else:
            run_args[lang] = [exe[lang]]
        if workload in ("text", "textlazy"):
            run_args[lang] = run_args[lang] + [tmp / "input.txt"]
    builds = {lang: [] for lang in langs}
    runs = {lang: [] for lang in langs}
    for _ in range(REPEATS):
        for lang in langs:
            if build_args[lang] is None:
                continue
            env = None
            if options.cold and lang == "ae":
                cold = tmp / f"cold-{workload}-{lang}"
                shutil.rmtree(cold, ignore_errors=True)
                env = {**os.environ, "AE_CACHE": str(cold)}
            if exe[lang].exists():
                exe[lang].unlink()
            peak_exe = tmp / (workload + "-" + lang + "-peak" + suffix)
            if peak_exe.exists():
                peak_exe.unlink()
            peak_env = env
            if options.cold and lang == "ae":
                peak_cold = tmp / f"cold-{workload}-{lang}-peak"
                shutil.rmtree(peak_cold, ignore_errors=True)
                peak_env = {**os.environ, "AE_CACHE": str(peak_cold)}
            peak_build = build_commands(workload, lang, peak_exe, compiler, options)
            builds[lang].append(timed(build_args[lang], env=env, peak_args=peak_build, peak_env=peak_env))
        round_expected = expected
        for lang in langs:
            sample = timed(run_args[lang])
            output = sample["stdout"].replace(b"\r\n", b"\n").strip()
            if round_expected is None:
                round_expected = output
            if output != round_expected:
                raise RuntimeError(f"{lang} timed output disagreed: {output!r} vs {round_expected!r}")
            runs[lang].append(sample)
    outputs = {}
    for lang in langs:
        # Normalize line endings: AE's stdout is opened in text mode on
        # Windows, the others write raw bytes.
        outputs[lang] = call(run_args[lang], stdout=subprocess.PIPE, stderr=subprocess.PIPE).stdout.replace(b"\r\n", b"\n").strip()
        if expected is None:
            if outputs[lang] != outputs["ae"]:
                raise RuntimeError(f"{lang} disagreed with ae: {outputs[lang]!r} vs {outputs['ae']!r}")
        elif outputs[lang] != expected:
            raise RuntimeError(f"{lang} incorrect output: {outputs[lang]!r}")
    results = {}
    for lang in langs:
        b, r = builds[lang], runs[lang]
        shipped = exe[lang] if lang != "python" else BENCH / f"{workload}.py"
        results[lang] = {
            "build_ms": [x["wall_ms"] for x in b] or None,
            "build_cpu_ms": [x["cpu_ms"] for x in b] or None,
            "build_peak_mb": [x["peak_mb"] for x in b] or None,
            "build_median_ms": _median(x["wall_ms"] for x in b),
            "build_cpu_median_ms": _median(x["cpu_ms"] for x in b),
            "build_peak_median_mb": _median(x["peak_mb"] for x in b),
            "run_ms": [x["wall_ms"] for x in r],
            "run_cpu_ms": [x["cpu_ms"] for x in r],
            "run_peak_mb": [x["peak_mb"] for x in r],
            "run_median_ms": statistics.median(x["wall_ms"] for x in r),
            "run_cpu_median_ms": _median(x["cpu_ms"] for x in r),
            "run_peak_median_mb": _median(x["peak_mb"] for x in r),
            "shipped_bytes": shipped.stat().st_size,
            "output": outputs[lang].decode(errors="replace"),
        }
    return results


if __name__ == "__main__":
    main()
