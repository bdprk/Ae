"""Compare the eager `split` text workload with the streaming `tokens` cursor.

`split(text, sep)` materialises every token: a pointer array plus one byte arena
holding a copy of all token bytes (ADR-49). `tokens(text, sep)` walks the same
boundaries lazily and hands out one view per iteration, so nothing is allocated
per token (ADR-53).

Both programs compute the same three numbers from the same input, and the script
asserts that their stdout is byte-identical, so a timing win cannot hide a
behaviour change. Rounds alternate which program runs first to cancel load
drift, and the peak resident set of each process is sampled while it runs.

Usage:
  python bench/tokens_timings.py --exe target/release/ae --runs 9 --json out.json
"""

import argparse
import json
import os
import pathlib
import statistics
import subprocess
import sys
import tempfile
import time

ROOT = pathlib.Path(__file__).resolve().parent.parent
DEFAULT_EXE = ROOT / "target" / "release" / ("ae.exe" if os.name == "nt" else "ae")

sys.path.insert(0, str(ROOT / "bench"))
from run import peak_bytes  # noqa: E402  (same directory, shared sampler)

WORDS = ["alpha", "beta", "gamma", "delta", "epsilon", "zeta", "eta", "theta"]
LINES = 50000

# Read-only baseline: how much of the total is file I/O rather than tokenising.
READ = """
fn main() {
    let parts = args();
    let text = read_file(parts[1]);
    print(len(text));
}
"""


def write_input(path):
    with open(path, "w", encoding="ascii", newline="\n") as handle:
        for i in range(LINES):
            row = " ".join(WORDS[(i + j) % len(WORDS)] for j in range(8))
            handle.write(f"{row} {i}\n")


def build(compiler, source, exe):
    subprocess.run([str(compiler), "build", str(source), "-o", str(exe), "--release"], check=True,
                   stdout=subprocess.DEVNULL)


def run(exe, input_path):
    """Wall time from one run, peak memory from a second one.

    Sampling peak memory inside the timed run distorts it: on Windows
    `time.sleep(0.001)` waits for the 15.6 ms timer tick, so short programs are
    reported several times slower. The two passes keep each number clean.
    """
    started = time.perf_counter()
    result = subprocess.run([str(exe), str(input_path)], stdout=subprocess.PIPE, stderr=subprocess.PIPE)
    elapsed = (time.perf_counter() - started) * 1000.0
    if result.returncode != 0:
        raise RuntimeError(f"{exe} failed: {result.stderr.decode(errors='replace')}")
    process = subprocess.Popen([str(exe), str(input_path)], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    peak = None
    while process.poll() is None:
        value = peak_bytes(process.pid)
        if value is not None and (peak is None or value > peak):
            peak = value
        time.sleep(0.001)
    process.wait()
    return elapsed, result.stdout, None if peak is None else peak / (1024 * 1024)


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--exe", default=str(DEFAULT_EXE))
    parser.add_argument("--runs", type=int, default=9)
    parser.add_argument("--json", default=None)
    options = parser.parse_args()

    data = {"runs": options.runs, "lines": LINES, "programs": {}}
    with tempfile.TemporaryDirectory(prefix="ae-tokens-") as tmp:
        tmp = pathlib.Path(tmp)
        input_path = tmp / "input.txt"
        write_input(input_path)
        data["input_bytes"] = input_path.stat().st_size

        sources = {
            "read": READ,
            "eager": (ROOT / "bench" / "text.ae").read_text(encoding="utf-8"),
            "lazy": (ROOT / "bench" / "textlazy.ae").read_text(encoding="utf-8"),
        }
        exes = {}
        for name, text in sources.items():
            source = tmp / f"{name}.ae"
            source.write_text(text, encoding="utf-8")
            exes[name] = tmp / (name + (".exe" if os.name == "nt" else ""))
            build(options.exe, source, exes[name])

        times = {name: [] for name in sources}
        peaks = {name: [] for name in sources}
        outputs = {}
        order = ("read", "eager", "lazy")
        for round_index in range(options.runs):
            sequence = order if round_index % 2 == 0 else tuple(reversed(order))
            for name in sequence:
                elapsed, stdout, peak = run(exes[name], input_path)
                times[name].append(elapsed)
                if peak is not None:
                    peaks[name].append(peak)
                outputs[name] = stdout

        if outputs["eager"] != outputs["lazy"]:
            raise RuntimeError(f"eager and lazy disagreed: {outputs['eager']!r} vs {outputs['lazy']!r}")

        for name in sources:
            median = statistics.median(times[name])
            data["programs"][name] = {
                "raw_ms": [round(x, 3) for x in times[name]],
                "median_ms": round(median, 3),
                "raw_peak_mb": [round(x, 3) for x in peaks[name]],
                "median_peak_mb": round(statistics.median(peaks[name]), 3) if peaks[name] else None,
            }
            peak = data["programs"][name]["median_peak_mb"]
            print(f"{name:6s} {median:7.2f} ms  peak={peak if peak is not None else 'n/a'} MB")

        eager = data["programs"]["eager"]["median_ms"]
        lazy = data["programs"]["lazy"]["median_ms"]
        read = data["programs"]["read"]["median_ms"]
        data["tokenizing_ms"] = {"eager": round(eager - read, 3), "lazy": round(lazy - read, 3)}
        data["ratio"] = round(eager / lazy, 3) if lazy else None
        print(f"tokenising beyond I/O: eager={eager - read:.2f} ms lazy={lazy - read:.2f} ms")
        print(f"eager/lazy = {eager / lazy:.2f}x" if lazy else "eager/lazy = n/a")
        if options.json:
            pathlib.Path(options.json).write_text(json.dumps(data, indent=2), encoding="utf-8")
            print(f"wrote {options.json}")


if __name__ == "__main__":
    main()
