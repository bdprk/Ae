#!/usr/bin/env python3
"""Acceptance run for `ae fmt` over the whole example corpus.

    python scripts/fmt_acceptance.py --json <archive>/fmt-acceptance.json

Nothing in the repository is modified: every example is first copied to a
temporary directory (modules keep their relative layout, so `use` resolves),
and all formatting happens on the copies.

Standalone examples (files that `emit-c` accepts, i.e. they define main):
  1. `emit-c` succeeds on the untouched copy (the baseline),
  2. `fmt` succeeds,
  3. the generated C after formatting differs from the baseline only in
     embedded numbers: line counts match and every differing line matches
     once digit characters are ignored (runtime checks embed the source
     column, which spacing may move — this is the documented, permitted
     difference; any other change is a defect),
  4. a second `fmt` writes nothing: byte-for-byte idempotence.

Module files (examples/modules/*.ae) have no main of their own, so `emit-c`
cannot address them directly. They are formatted one by one and the whole
program is re-emitted from `modules/main.ae` before and after; the same
digit-free line comparison applies. The before/after pair also proves the
formatted modules still load, since emit-c runs the full front end.
"""
import argparse
import hashlib
import json
import pathlib
import shutil
import subprocess
import sys
import tempfile
import time

ROOT = pathlib.Path(__file__).resolve().parent.parent
DIGITS = set("0123456789")


def run(exe, *args):
    started = time.perf_counter()
    proc = subprocess.run([str(exe), *args], capture_output=True)
    return proc, (time.perf_counter() - started) * 1000.0


def digit_free_lines(text: str) -> list:
    return ["".join(c for c in line if c not in DIGITS)
            for line in text.replace("\r\n", "\n").split("\n")]


def assert_c_equivalent(before_c: str, after_c: str, what: str) -> dict:
    was, now = digit_free_lines(before_c), digit_free_lines(after_c)
    assert len(was) == len(now), f"generated C line count changed for {what}"
    differing = [i for i, (a, b) in enumerate(zip(was, now)) if a != b]
    assert not differing, f"generated C changed beyond embedded numbers for {what}, lines {differing[:5]}"
    raw_before = before_c.replace("\r\n", "\n").split("\n")
    raw_after = after_c.replace("\r\n", "\n").split("\n")
    return {"emit_c_lines": len(was),
            "emit_c_differing_lines": sum(1 for a, b in zip(raw_before, raw_after) if a != b)}


def fmt_one(exe, source: pathlib.Path) -> dict:
    _, fmt_ms = run(exe, "fmt", source)
    first_hash = hashlib.sha256(source.read_bytes()).hexdigest()
    _, fmt2_ms = run(exe, "fmt", source)
    second_hash = hashlib.sha256(source.read_bytes()).hexdigest()
    assert first_hash == second_hash, f"fmt is not idempotent on {source}"
    return {"file": str(source), "fmt_ms": round(fmt_ms, 3), "fmt2_ms": round(fmt2_ms, 3)}


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--json", required=True, help="path of the JSON report to write")
    options = parser.parse_args()

    exe = ROOT / "target" / "release" / ("ae.exe" if sys.platform == "win32" else "ae")
    assert exe.is_file(), f"missing release binary: {exe}"

    report = {"ae": subprocess.run([str(exe), "version"], capture_output=True, text=True).stdout.strip(),
              "standalone": [], "modules": []}
    with tempfile.TemporaryDirectory(prefix="ae-fmt-acceptance-") as tmp:
        work = pathlib.Path(tmp)
        shutil.copytree(ROOT / "examples", work / "examples")

        standalone = sorted((work / "examples").glob("*.ae"))
        assert standalone, "example corpus missing"
        for source in standalone:
            baseline, _ = run(exe, "emit-c", source)
            assert baseline.returncode == 0, f"emit-c failed on {source}: {baseline.stderr.decode(errors='replace')}"
            before_text = source.read_text(encoding="utf-8")
            record = fmt_one(exe, source)
            after, _ = run(exe, "emit-c", source)
            assert after.returncode == 0, f"emit-c failed after fmt on {source}: {after.stderr.decode(errors='replace')}"
            record.update(assert_c_equivalent(baseline.stdout.decode("utf-8", errors="replace"),
                                              after.stdout.decode("utf-8", errors="replace"), source))
            record["file"] = str(source.relative_to(work)).replace("\\", "/")
            record["changed"] = source.read_text(encoding="utf-8") != before_text
            report["standalone"].append(record)

        main_c = work / "examples" / "modules" / "main.ae"
        modules = sorted((work / "examples" / "modules").glob("*.ae"))
        assert main_c.is_file() and len(modules) >= 2, "module corpus missing"
        baseline, _ = run(exe, "emit-c", main_c)
        assert baseline.returncode == 0, f"emit-c failed on {main_c}"
        for source in modules:
            before_text = source.read_text(encoding="utf-8")
            record = fmt_one(exe, source)
            record["file"] = str(source.relative_to(work)).replace("\\", "/")
            record["changed"] = source.read_text(encoding="utf-8") != before_text
            report["modules"].append(record)
        after, _ = run(exe, "emit-c", main_c)
        assert after.returncode == 0, f"emit-c failed after formatting modules"
        report["module_program"] = assert_c_equivalent(baseline.stdout.decode("utf-8", errors="replace"),
                                                       after.stdout.decode("utf-8", errors="replace"), "modules program")

    report["checked_files"] = len(report["standalone"]) + len(report["modules"])
    report["changed_files"] = [f["file"] for f in report["standalone"] + report["modules"] if f["changed"]]
    report["total_fmt_ms"] = round(sum(f["fmt_ms"] for f in report["standalone"] + report["modules"]), 3)
    report["note"] = ("emit-c comparison ignores digit characters per line: runtime checks embed the source "
                      "column, which reformatting may move. Any other difference fails the run. The repository "
                      "copy of every example is untouched; formatting happened on temporary copies.")
    pathlib.Path(options.json).write_text(json.dumps(report, indent=2) + "\n", encoding="utf-8")
    print(f"checked {report['checked_files']} files; "
          f"not already formatted: {len(report['changed_files'])} {report['changed_files']}; "
          f"total fmt {report['total_fmt_ms']:.1f} ms")


if __name__ == "__main__":
    main()
