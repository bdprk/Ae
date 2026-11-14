#!/usr/bin/env python3
"""Corpus sweep for M2.4 drop elaboration (ADR-0204).

Counts release call sites (`_release(&...`) in the generated C for every
compilable tests/run program, through the MIR backend (drop elaboration on,
the default) and the tree backend (AE_CODEGEN=tree, conservative lists).
The delta is the shipped-C share drop elaboration removes.

Usage: python bench/drop_elision_corpus.py [--suite tests/run]
"""

import argparse
import glob
import json
import os
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
EXE = ROOT / "target" / "debug" / ("ae.exe" if os.name == "nt" else "ae")


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--suite", default="tests/run")
    options = parser.parse_args()

    paths = sorted(glob.glob(f"{options.suite}/*.ae"))
    rows = []
    for path in paths:
        try:
            mir = subprocess.run([str(EXE), "emit-c", path], capture_output=True, text=True, timeout=60)
            env = {**os.environ, "AE_CODEGEN": "tree"}
            tree = subprocess.run([str(EXE), "emit-c", path], capture_output=True, text=True, timeout=60, env=env)
        except subprocess.TimeoutExpired:
            rows.append({"file": path, "skipped": "timeout"})
            continue
        if mir.returncode != 0 or tree.returncode != 0:
            rows.append({"file": path, "skipped": "compile error"})
            continue
        rows.append({
            "file": path,
            "mir_release_sites": mir.stdout.count("_release(&"),
            "tree_release_sites": tree.stdout.count("_release(&"),
        })
    compiled = [row for row in rows if "mir_release_sites" in row]
    total_mir = sum(row["mir_release_sites"] for row in compiled)
    total_tree = sum(row["tree_release_sites"] for row in compiled)
    report = {
        "suite": options.suite,
        "files": len(paths),
        "compiled": len(compiled),
        "total_mir_release_sites": total_mir,
        "total_tree_release_sites": total_tree,
        "elided": total_tree - total_mir,
        "elided_share": round((total_tree - total_mir) / max(total_tree, 1), 4),
        "rows": rows,
    }
    print(json.dumps({k: v for k, v in report.items() if k != "rows"}, indent=2))
    out = ROOT / "bench" / "results" / "2026-11-14-windows-drop-elision-corpus.json"
    out.write_text(json.dumps(report, indent=2), encoding="utf-8")
    print(f"raw results: {out}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
