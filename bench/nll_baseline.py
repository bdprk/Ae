"""Differential baseline for the borrow-checker roadmap (ADR-63/67).

Every case records the CURRENT verdict of `ae check` and the roadmap level
at which that verdict is expected to flip:

* ``L0-keep``  — rejected today and at every level (soundness, not expressiveness);
* ``L0-allow`` — accepted today; a regression pin (L1+ must keep accepting);
* ``L1-flip``  — rejected today, NLL (loan dies at last use) allows it;
* ``L2-flip``  — rejected today and under NLL, field-granularity loans allow it;
* ``L3-flip``  — rejected or inexpressible today, escaping borrows allow it.

The script fails if today's verdict disagrees with ``expect_now`` — so the
baseline cannot drift silently while the front end evolves. The JSON report is
the acceptance list for the L1 (NLL) implementation: flipping exactly the
``L1-flip`` set, keeping everything else, is the definition of done.

Usage:
  python bench/nll_baseline.py --json out.json
"""

import argparse
import json
import os
import pathlib
import subprocess
import tempfile

ROOT = pathlib.Path(__file__).resolve().parent.parent
EXE = ROOT / "target" / "debug" / ("ae.exe" if os.name == "nt" else "ae")

# name, expect_now ("accept"|"reject"), flip level, source. Every case states
# in a comment what it pins and when it is allowed to change.
CASES = [
    # ---- L0-allow: accepted today, regression pins -------------------------
    ("scope_exit_then_write", "accept", "L0-allow", """fn touch() -> i64 {
    var s = "hello";
    if true {
        let v = slice_view(s, 0, 4);
        print(v);
    }
    s = "x";
    return len(s);
}
fn main() { print(touch()); }
"""),
    ("push_after_for_loop", "accept", "L0-allow", """fn main() {
    var v = vec[1, 2, 3];
    for x in v { print(x); }
    push(v, 4);
    print(len(v));
}
"""),
    ("string_view_stored_in_let", "accept", "L0-allow", """// A string view is a pointer plus a length into immutable bytes, so copying
// it into a let is sound; the storage rejection is vec-specific (a vec borrow
// would dangle through a realloc).
fn hold(s: &string) -> i64 {
    let q = s;
    return len(q);
}
fn main() { print(hold("hello")); }
"""),
    ("other_base_unaffected", "accept", "L0-allow", """// Two loans on two bases: writing the untouched base is fine today.
fn pick(s: &string) -> string { return "got " + s; }
fn main() {
    var a = "alpha";
    var b = "beta";
    let va = slice_view(a, 0, 3);
    print(pick(b));
    b = "gamma";
    print(va);
    print(b);
}
"""),
    # ---- L0-keep: rejected today and at every level ------------------------
    ("write_between_creation_and_use", "reject", "L0-keep", """fn main() {
    var s = "hello";
    let v = slice_view(s, 0, 4);
    s = "x";
    print(v);
}
"""),
    ("move_between_creation_and_use", "reject", "L0-keep", """fn consume(s: string) -> i64 { return len(s); }
fn main() {
    var s = "hello";
    let v = slice_view(s, 0, 4);
    print(consume(s));
    print(v);
}
"""),
    ("push_during_for_loop", "reject", "L0-keep", """fn main() {
    var v = vec[1, 2, 3];
    for x in v { push(v, x); }
    print(len(v));
}
"""),
    ("vec_view_stored_in_let", "reject", "L0-keep", """// A vec borrow points into a buffer that push may reallocate, so storing it
// is unsound at every level of the roadmap.
fn hold(v: &vec) -> i64 {
    let q = v;
    return len(q);
}
fn main() { print(hold(vec[1])); }
"""),
    # ---- L1-flip: NLL (loan dies at last use) allows these -----------------
    ("write_after_last_use", "reject", "L1-flip", """fn main() {
    var s = "hello world";
    let v = slice_view(s, 0, 5);
    print(v);
    s = "goodbye";
    print(s);
}
"""),
    ("move_after_last_use", "reject", "L1-flip", """fn consume(s: string) -> i64 { return len(s); }
fn main() {
    var s = "hello";
    let v = slice_view(s, 0, 4);
    print(v);
    print(consume(s));
}
"""),
    ("field_write_after_field_view_last_use", "reject", "L1-flip", """// The field view borrows the whole variable (ADR-52); NLL kills that loan at
// the view's last use, so the field write passes without field granularity.
struct P { name: string, age: i64 }
fn main() {
    var p = P { name: "ada", age: 1 };
    let v = slice_view(p.name, 0, 2);
    print(v);
    p.age = 2;
    print(p.age);
}
"""),
    ("dead_loan_does_not_block_its_own_base", "reject", "L1-flip", """// Two loans: a's view is dead after its last use, b's is still live. NLL
// must free exactly a — writing a passes while b stays frozen.
fn pick(s: &string) -> string { return "got " + s; }
fn main() {
    var a = "alpha";
    var b = "beta";
    let va = slice_view(a, 0, 3);
    print(pick(b));
    print(va);
    a = "gamma";
    print(pick(b));
    print(a);
}
"""),
    # ---- L2-flip: field-granularity loans allow these ----------------------
    ("field_write_while_field_view_alive", "reject", "L2-flip", """// The view is still used after the write, so even NLL keeps the whole-variable
// loan alive; only a loan that names the place (p.name, not p) allows this.
struct P { name: string, age: i64 }
fn main() {
    var p = P { name: "ada", age: 1 };
    let v = slice_view(p.name, 0, 2);
    p.age = 2;
    print(v);
    print(p.age);
}
"""),
    # ---- L3-flip: escaping borrows allow these -----------------------------
    ("return_view_from_function", "reject", "L3-flip", """// Inexpressible today: view types are unnameable, so the function signature
// itself is a parse error. L3 must first introduce (or infer) escape-capable
// view types; the case pins that no accidental escape path exists.
fn first(s: &string) -> string view { return slice_view(s, 0, 1); }
fn main() { var s = "hello"; print(first(s)); }
"""),
]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--json", type=pathlib.Path, required=True)
    options = parser.parse_args()
    if not EXE.exists():
        raise SystemExit(f"missing compiler binary: {EXE} (cargo build first)")

    report = {"compiler": str(EXE), "cases": []}
    disagreements = 0
    with tempfile.TemporaryDirectory(prefix="ae-nll-") as tmp:
        for name, expect_now, level, source in CASES:
            path = pathlib.Path(tmp) / f"{name}.ae"
            path.write_text(source, encoding="utf-8")
            proc = subprocess.run([str(EXE), "check", str(path)], capture_output=True)
            verdict = "accept" if proc.returncode == 0 else "reject"
            message = (proc.stderr.decode("utf-8", errors="replace").strip().splitlines() or [""])[0]
            ok = verdict == expect_now
            if not ok:
                disagreements += 1
            report["cases"].append({
                "name": name, "expect_now": expect_now, "verdict": verdict,
                "matches": ok, "flip_level": level, "message": message,
            })
            print(f"{'OK ' if ok else 'BAD'} {name:42s} {verdict:6s} ({level})")
    report["disagreements"] = disagreements
    report["total"] = len(CASES)
    options.json.parent.mkdir(parents=True, exist_ok=True)
    options.json.write_text(json.dumps(report, indent=2), encoding="utf-8")
    if disagreements:
        raise SystemExit(f"{disagreements} case(s) disagree with the recorded baseline")
    print(f"baseline holds: {len(CASES)} cases, 0 disagreements")


if __name__ == "__main__":
    main()
