"""Measure separate GCC compile/link phases; exploratory, not the normal build path."""
import json
import os
import platform
import statistics
import subprocess
import tempfile
import time
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent

def command(args, **kwargs):
    return subprocess.run([str(a) for a in args], check=True, stdout=subprocess.PIPE,
                          stderr=subprocess.PIPE, **kwargs)

def elapsed(args):
    start = time.perf_counter_ns()
    command(args)
    return (time.perf_counter_ns() - start) / 1_000_000

def main():
    command(["cargo", "build", "--release", "--locked"], cwd=ROOT)
    suffix = ".exe" if os.name == "nt" else ""
    compiler = ROOT / "target" / "release" / ("ae" + suffix)
    cc = os.environ.get("AE_CC", "gcc")
    generated = command([compiler, "emit-c", ROOT / "bench" / "sum.ae"]).stdout
    samples = []
    with tempfile.TemporaryDirectory(prefix="ae-phases-") as tmp:
        directory = Path(tmp)
        source = directory / "program.c"
        obj = directory / "program.o"
        executable = directory / ("program" + suffix)
        source.write_bytes(generated)
        for iteration in range(5):
            compile_ms = elapsed([cc, "-std=c11", "-O2", "-c", source, "-o", obj])
            link_ms = elapsed([cc, obj, "-o", executable])
            output = command([executable]).stdout.strip()
            if output != b"499999500000":
                raise RuntimeError(f"unexpected output at iteration {iteration}: {output!r}")
            samples.append({"compile_ms": compile_ms, "link_ms": link_ms})
    print(json.dumps({"platform": platform.platform(), "compiler": cc,
                      "note": "Two separate GCC invocations, not the normal combined build; cache state uncontrolled, wall time includes process startup.",
                      "samples": samples,
                      "compile_median_ms": statistics.median(x["compile_ms"] for x in samples),
                      "link_median_ms": statistics.median(x["link_ms"] for x in samples)}, indent=2))

if __name__ == "__main__":
    main()
