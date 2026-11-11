"""Measure the two build levels (-O0 via --debug, -O2 the build default, ADR-59).

`build` writes the artifact a user ships, so it defaults to -O2; `run` is the
development loop and stays at -O0. This script quantifies both sides of that
trade on the four bench workloads in one session, alternating the order of the
levels each round so load drift does not favour either side:

* build time — end-to-end `ae build <src> -o <exe>` against a fresh cache
  directory every round, so the C compiler always runs;
* run time — the two produced binaries alternate per round on the same input;
* artifact size and generated-C size for context.

stdout must be identical between the levels, so a timing win cannot hide a
behaviour change. Wall-clock medians are reported alongside every raw value.

Usage:
  python bench/build_levels_timings.py --runs 5 --run-runs 9 --json out.json
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
AE = ROOT / "target" / "release" / ("ae.exe" if os.name == "nt" else "ae")
PROBE = ROOT / "target" / "probe" / "buildlevels"
WORKLOADS = ["sum", "text", "textlazy", "map"]
LEVELS = {"o0": ["--debug"], "o2": []}
ORDER = [("o0", "o2"), ("o2", "o0")]


def timed(command, env=None):
    start = time.perf_counter()
    result = subprocess.run(command, env=env, capture_output=True)
    elapsed = (time.perf_counter() - start) * 1000.0
    if result.returncode != 0:
        raise SystemExit(f"command failed ({result.returncode}): {command}\n{result.stderr.decode(errors='replace')}")
    return elapsed, result.stdout


def build_once(compiler, source, out, flag, env=None):
    """`build` refuses to overwrite, so remove the artifact outside the clock."""
    if out.exists():
        out.unlink()
    return timed([str(compiler), "build", str(source), "-o", str(out), *flag], env=env)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--runs", type=int, default=5, help="build rounds per level")
    parser.add_argument("--run-runs", type=int, default=9, help="execution rounds per level")
    parser.add_argument("--json", type=pathlib.Path, required=True)
    args = parser.parse_args()

    if not AE.exists():
        raise SystemExit(f"missing compiler binary: {AE} (cargo build --release first)")

    sys.path.insert(0, str(ROOT / "bench"))
    from run import TEXT_LINES, write_text_input

    text_input = PROBE / "input.txt"
    PROBE.mkdir(parents=True, exist_ok=True)
    if not text_input.exists():
        write_text_input(text_input)
    RUN_ARGS = {"sum": [], "text": [text_input], "textlazy": [text_input], "map": []}

    report = {"compiler": str(AE), "text_input_bytes": text_input.stat().st_size,
              "text_input_lines": TEXT_LINES, "runs": args.runs, "run_runs": args.run_runs,
              "workloads": {}}
    for name in WORKLOADS:
        source = ROOT / "bench" / f"{name}.ae"
        _, c_text = timed([str(AE), "emit-c", str(source)])
        entry = {"generated_c_bytes": len(c_text), "build": {}, "run": {}}

        exes = {}
        for level, flag in LEVELS.items():
            out = PROBE / f"{name}-{level}" / ("prog.exe" if os.name == "nt" else "prog")
            out.parent.mkdir(parents=True, exist_ok=True)
            build_once(AE, source, out, flag)
            entry["build"][f"exe_bytes_{level}"] = out.stat().st_size
            exes[level] = out

        # Build time against a fresh cache every round; alternate levels so
        # load drift lands on both sides equally.
        raw = {"o0": [], "o2": []}
        for round_index in range(args.runs):
            for level in ORDER[round_index % 2]:
                with tempfile.TemporaryDirectory() as cache:
                    env = dict(os.environ, AE_CACHE=cache)
                    elapsed, _ = build_once(AE, source, exes[level], LEVELS[level], env=env)
                    raw[level].append(round(elapsed, 3))
        entry["build"]["raw_ms"] = raw
        entry["build"]["median_ms"] = {level: statistics.median(v) for level, v in raw.items()}
        entry["build"]["o2_cost_ms"] = round(entry["build"]["median_ms"]["o2"] - entry["build"]["median_ms"]["o0"], 3)

        # Run time: assert identical stdout first, then alternate binaries.
        program_args = [str(a) for a in RUN_ARGS[name]]
        stdout = {}
        for level, exe in exes.items():
            _, stdout[level] = timed([str(exe), *program_args])
        if stdout["o0"] != stdout["o2"]:
            raise SystemExit(f"{name}: output differs between -O0 and -O2")
        entry["run"]["stdout_bytes"] = len(stdout["o0"])

        raw = {"o0": [], "o2": []}
        for round_index in range(args.run_runs):
            for level in ORDER[round_index % 2]:
                elapsed, _ = timed([str(exes[level]), *program_args])
                raw[level].append(round(elapsed, 3))
        entry["run"]["raw_ms"] = raw
        entry["run"]["median_ms"] = {level: statistics.median(v) for level, v in raw.items()}
        entry["run"]["speedup_o2"] = round(entry["run"]["median_ms"]["o0"] / entry["run"]["median_ms"]["o2"], 3) if entry["run"]["median_ms"]["o2"] else None
        report["workloads"][name] = entry

    args.json.parent.mkdir(parents=True, exist_ok=True)
    args.json.write_text(json.dumps(report, indent=2), encoding="utf-8")
    print(json.dumps(report, indent=2))


if __name__ == "__main__":
    main()
