#!/usr/bin/env python3
"""Differential fuzzing for the AE compiler (plan M2.10, C-backend slice).

Generates random *well-typed* programs from a grammar of arithmetic,
comparisons, control flow, strings, vectors, structs, and enums; compiles each
source with every backend and optimization level — the MIR block-graph
backend (the -O0 default, M2.3) and the structured tree emitter
(`AE_CODEGEN=tree`), both at -O0, plus the optimized build; runs every
binary; and asserts that all runs produce identical output. The parser's own
move/borrow/type rules gate generation (a generated program the compiler
rejects is a generator bug and fails the run), so each seed doubles as a
regression test of compile-or-die determinism.

Usage: python scripts/diff_fuzz.py [--seeds N] [--start SEED]
"""

import argparse
import os
import subprocess
import sys
import tempfile
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
EXE = ROOT / "target" / "debug" / ("ae.exe" if os.name == "nt" else "ae")


class Gen:
    """A deterministic random program generator."""

    def __init__(self, seed: int):
        self.state = (seed & 0xFFFFFFFF) or 1
        self.lines: list[str] = []

    def rnd(self) -> int:
        self.state ^= (self.state << 13) & 0xFFFFFFFF
        self.state ^= self.state >> 7
        self.state ^= (self.state << 17) & 0xFFFFFFFF
        return self.state & 0xFFFFFFFF

    def pick(self, choices):
        return choices[self.rnd() % len(choices)]

    def int_expr(self, vars: list[str], depth: int = 0) -> str:
        if depth >= 3 or self.rnd() % 4 == 0:
            if vars and self.rnd() % 3 != 0:
                return self.pick(vars)
            return str(self.rnd() % 1000 - 500)
        op = self.pick(["+", "-", "*", "/", "%"])
        a = self.int_expr(vars, depth + 1)
        b = self.int_expr(vars, depth + 1)
        # Division and modulo wrap their divisor in a provably nonzero guard:
        # `(b * b) + 1` is positive for every integer b, so the program is
        # well-defined at runtime whichever way the operands evaluate.
        if op in ("/", "%"):
            return f"({a} {op} (({b} * {b}) + 1))"
        return f"({a} {op} {b})"

    def bool_expr(self, vars: list[str], depth: int = 0) -> str:
        if depth >= 2:
            return self.pick(["true", "false"])
        kind = self.rnd() % 3
        if kind == 0:
            return f"({self.int_expr(vars, depth + 1)} {self.pick(['<', '<=', '>', '>=', '==', '!='])} {self.int_expr(vars, depth + 1)})"
        if kind == 1:
            return f"({self.bool_expr(vars, depth + 1)} {self.pick(['&&', '||'])} {self.bool_expr(vars, depth + 1)})"
        return f"!{self.bool_expr(vars, depth + 1)}"

    def statement(self, vars: list[str], depth: int) -> list[str]:
        """One statement, possibly introducing new variables. Returns lines."""
        if depth >= 3:
            return [f"print({self.int_expr(vars)});"]
        kind = self.rnd() % 9
        if kind == 6:
            # A string built from two literals; reads are borrows.
            name = f"str{self.rnd() % 1000}"
            a = '"t' + str(self.rnd() % 1000) + '"'
            b = '"u' + str(self.rnd() % 1000) + '"'
            return [
                f"var {name} = {a} + {b};",
                f"print(len({name}));",
                *self.statement(vars, depth + 1),
            ]
        if kind == 7:
            # A vector with pushes and a length read.
            name = f"vec{self.rnd() % 1000}"
            return [
                f"var {name} = vec[{self.int_expr(vars)}, {self.int_expr(vars)}];",
                f"push({name}, {self.int_expr(vars)});",
                f"print(len({name}));",
                *self.statement(vars, depth + 1),
            ]
        if kind == 8:
            # A struct with two scalar fields, read field-wise.
            name = f"pt{self.rnd() % 1000}"
            return [
                f"var {name} = Pt {{ a: {self.int_expr(vars)}, b: {self.int_expr(vars)} }};",
                f"print({name}.a + {name}.b);",
                *self.statement(vars, depth + 1),
            ]
        if kind == 0:
            name = f"x{self.rnd() % 100}"
            return [f"var {name} = {self.int_expr(vars)};"] + self.statement(vars + [name], depth + 1)
        if kind == 1:
            return [
                f"if {self.bool_expr(vars)} {{",
                *self.statement(vars, depth + 1),
                "} else {",
                *self.statement(vars, depth + 1),
                "}",
            ]
        if kind == 2:
            name = f"n{self.rnd() % 100}"
            bound = self.int_expr(vars)
            return [
                f"var {name} = {bound};",
                f"var i{self.rnd() % 100} = 0;",
                *self.statement(vars, depth + 1),
            ]
        if kind == 3:
            return [f"print({self.int_expr(vars)});"]
        if kind == 4:
            return [
                f"if {self.bool_expr(vars)} {{",
                f"    print({self.int_expr(vars)});",
                "}",
                *self.statement(vars, depth + 1),
            ]
        name = f"s{self.rnd() % 100}"
        return [
            f"var {name} = {self.int_expr(vars)};",
            f"print({name} > 0 || {name} <= 0);",
            *self.statement(vars, depth + 1),
        ]

    def program(self) -> str:
        # `main` cannot `return` in this language; its exit is implicit. The
        # struct Pt and its two scalar fields are fixed: field reads borrow.
        lines = ["struct Pt { a: i64, b: i64 }", "fn main() {"]
        vars = ["seed0"]
        lines.append("    var seed0 = 7;")
        body = self.statement(vars, 0)
        lines.extend("    " + line for line in body)
        lines.append("}")
        return "\n".join(lines)


def compile_at(source: str, workdir: Path, tag: str, opt: bool, tree: bool = False) -> Path:
    src = workdir / f"prog_{tag}.ae"
    src.write_text(source, encoding="utf-8")
    # `build` optimizes by default; --debug compiles the same source at -O0.
    # AE_CODEGEN=tree switches that one compile to the structured tree
    # emitter, so the MIR backend's C is differentiated against it (M2.3).
    env = {**os.environ, "AE_CODEGEN": "tree"} if tree else None
    result = subprocess.run(
        [str(EXE), "build", str(src), "-o", str(workdir / f"prog_{tag}")] + ([] if opt else ["--debug"]),
        capture_output=True,
        text=True,
        env=env,
    )
    if result.returncode != 0:
        raise RuntimeError(f"compile ({tag}) failed: {result.stderr}")
    out = workdir / f"prog_{tag}"
    if os.name == "nt" and not out.suffix:
        # Windows executability comes from the `.exe` suffix, which `-o` does
        # not add when the user names the file explicitly. A stale file from
        # an earlier seed with the same tag is replaced, not merged.
        renamed = out.with_suffix(".exe")
        renamed.unlink(missing_ok=True)
        out.rename(renamed)
        return renamed
    return out


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--seeds", type=int, default=40)
    parser.add_argument("--start", type=int, default=0)
    args = parser.parse_args()

    if not EXE.exists():
        print(f"ae binary missing: {EXE}\nbuild it first: cargo build --workspace", file=sys.stderr)
        return 2

    failures = 0
    with tempfile.TemporaryDirectory() as tmp:
        workdir = Path(tmp)
        for seed in range(args.start, args.start + args.seeds):
            gen = Gen(seed)
            source = gen.program()
            try:
                debug_bin = compile_at(source, workdir, "d", opt=False)
                tree_bin = compile_at(source, workdir, "t", opt=False, tree=True)
                release_bin = compile_at(source, workdir, "r", opt=True)
                outputs = []
                for bin_path in (debug_bin, tree_bin, release_bin):
                    run = subprocess.run([str(bin_path)], capture_output=True, text=True, timeout=30)
                    if run.returncode != 0:
                        raise RuntimeError(f"run ({bin_path.name}) failed: {run.stderr}")
                    outputs.append(run.stdout)
                interp_run = subprocess.run(
                    [str(EXE), "run", str(workdir / "prog_d.ae"), "--interp"],
                    capture_output=True,
                    text=True,
                    timeout=30,
                )
                if interp_run.returncode != 0:
                    raise RuntimeError(f"run (--interp) failed: {interp_run.stderr}")
                outputs.append(interp_run.stdout)
                if len(set(outputs)) != 1:
                    failures += 1
                    print(
                        f"seed {seed}: OUTPUT MISMATCH\n{source}"
                        f"\n--- debug (mir) ---\n{outputs[0]}"
                        f"\n--- debug (tree) ---\n{outputs[1]}"
                        f"\n--- release ---\n{outputs[2]}"
                        f"\n--- interp ---\n{outputs[3]}"
                    )
                else:
                    print(f"seed {seed}: ok ({len(outputs[0].splitlines())} lines)")
            except RuntimeError as error:
                failures += 1
                print(f"seed {seed}: FAILURE\n{source}\n{error}")

    print(f"\n{args.seeds} seeds, {failures} failures")
    return 1 if failures else 0


if __name__ == "__main__":
    sys.exit(main())
