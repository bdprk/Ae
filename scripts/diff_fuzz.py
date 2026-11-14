#!/usr/bin/env python3
"""Differential fuzzing for the AE compiler (plan M2.10, C-backend slice).

Generates random *well-typed* programs from a grammar of arithmetic,
comparisons, control flow, strings (including one a branch moves into a
by-value call, the mixed drop shape M2.4 keeps unproven), vectors with
their mutation surface (push, indexed store, sort) and string vectors
(push, element store releasing the old element, join), structs, enums,
matches with payload bindings, counted loops with `continue`, and the
borrow surface (M2.5): a Doc struct with a `&mut self` method and a free
`&mut` function, field views born and read inside one statement group,
the L2 field-granularity write across a live view, and the ordered
read-then-release shape left-to-right evaluation keeps legal; compiles each source
with every backend and optimization level — the MIR block-graph backend (the
-O0 default, M2.3) and the structured tree emitter (`AE_CODEGEN=tree`),
both at -O0, plus the optimized build; runs every binary; and asserts that
all runs produce identical output. A runtime trap is agreement too (the
generator can produce overflowing arithmetic; seed 1762): every mode must
exit with the same code, print the same stdout up to the trap, and trap at
the same position with the same message. The parser's own move/borrow/type
rules gate generation (a generated program the compiler rejects is a
generator bug and fails the run), so each seed doubles as a regression test
of compile-or-die determinism.

Usage: python scripts/diff_fuzz.py [--seeds N] [--start SEED] [--jobs N]
                                   [--save-corpus DIR]

Seeds run through a worker pool (--jobs, default min(cpu, 8)); each worker
owns its files and its own content cache, so parallel seeds never share
mutable state. Results print as they complete and the summary repeats the
failed seeds in order. --save-corpus writes every generated program as
DIR/seed_N.ae — the regression corpus for future compiler changes.
"""

import argparse
import os
import re
import time
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
        # Bindings are collision-free: every declaration draws its name from
        # one per-program counter instead of a random suffix, so two
        # statements can never declare the same variable twice.
        self.name_seq = 0

    def rnd(self) -> int:
        self.state ^= (self.state << 13) & 0xFFFFFFFF
        self.state ^= self.state >> 7
        self.state ^= (self.state << 17) & 0xFFFFFFFF
        return self.state & 0xFFFFFFFF

    def pick(self, choices):
        return choices[self.rnd() % len(choices)]

    def fresh(self, prefix: str) -> str:
        self.name_seq += 1
        return f"{prefix}{self.name_seq}"

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

    def statement(self, vars: list[str], depth: int, docs: list[str], pointers: list[tuple[str, str, bool]]) -> list[str]:
        """One statement, possibly introducing new variables. Returns lines.

        `docs` and `pointers` carry the borrow and raw-pointer surfaces
        (M2.5, M2.12), threading through the same subtree discipline `vars`
        follows: a binding created inside a branch stays in that branch's
        subtree, so no generated statement references a variable the parser
        would reject as undefined on some path. Pointer targets are scalars,
        which never move, so no generated shape can dangle.
        """
        if depth >= 3:
            return [f"print({self.int_expr(vars)});"]
        kind = self.rnd() % 21
        if 14 <= kind <= 17 and not docs:
            kind = 14
        if kind == 18 and not vars:
            kind = 3
        if kind in (19, 20) and (not vars or not pointers):
            kind = 3
        if kind == 13:
            # A string vector with the full mutation surface: pushes, an
            # indexed store (releasing the old element), and reads. The
            # differential exercises the string-ownership paths — the push
            # consumes, the store releases — against all four modes.
            name = self.fresh("sv")
            return [
                f"var {name} = strvec_new();",
                f'push({name}, "w{self.rnd() % 100}");',
                f'push({name}, "x{self.rnd() % 100}");',
                f'{name}[0] = "y{self.rnd() % 100}";',
                f"print(len({name}));",
                f'print({name}[0]);',
                f'print(join({name}, "-"));',
                *self.statement(vars, depth + 1, docs, pointers),
            ]
        if kind == 12:
            # A vector with the full mutation surface: push, indexed store,
            # sort, and reads. The MIR renders all of these as named
            # intrinsics now, so the four-way differential exercises their
            # lowering (index checks, sort determinism) on every seed.
            name = self.fresh("vv")
            return [
                f"var {name} = vec[{self.int_expr(vars)}, {self.int_expr(vars)}];",
                f"push({name}, {self.int_expr(vars)});",
                f"{name}[0] = {self.int_expr(vars)};",
                f"sort({name});",
                f"print(len({name}));",
                f"print({name}[0]);",
                *self.statement(vars, depth + 1, docs, pointers),
            ]
        if kind == 11:
            # A string one branch consumes: the taken path moves it into a
            # by-value call, the untaken path leaves it live, so its scope-end
            # release is a mixed drop site the MIR backend must keep without
            # proof (M2.4). The name is retired after the branch — it is never
            # read again, which is what keeps the program accepted.
            name = self.fresh("str")
            a = '"m' + str(self.rnd() % 1000) + '"'
            return [
                f"var {name} = {a};",
                f"print(len({name}) > 0);",
                f"if {self.bool_expr(vars)} {{ print(eat({name})); }}",
                *self.statement(vars, depth + 1, docs, pointers),
            ]
        if kind == 9:
            # A match over a generated enum value: every variant is covered,
            # arms carry payload bindings, and each arm prints one line.
            name = self.fresh("op")
            value = self.int_expr(vars)
            shape = self.rnd() % 3
            if shape == 0:
                init = f"Op::Wrap({value})"
            elif shape == 1:
                init = f"Op::Pair({value}, {self.int_expr(vars)})"
            else:
                init = "Op::Flat"
            return [
                f"var {name} = {init};",
                f"match {name} {{",
                f"    Op::Wrap(w) => {{ print(w); }}",
                f"    Op::Pair(a, b) => {{ print(a + b); }}",
                f"    Op::Flat => {{ print(42); }}",
                f"}}",
                *self.statement(vars, depth + 1, docs, pointers),
            ]
        if kind == 10:
            # A counted loop with a continue landing on the hidden increment.
            # The bound is a constant, so the iteration count is fixed and the
            # output is deterministic whichever backend emits the loop.
            bound = (self.rnd() % 4) + 1
            skip = self.rnd() % bound
            name = self.fresh("i")
            return [
                f"for {name} in 0..{bound} {{",
                f"    if {name} == {skip} {{ continue; }}",
                f"    print({name});",
                f"}}",
                *self.statement(vars, depth + 1, docs, pointers),
            ]
        if kind == 6:
            # A string built from two literals; reads are borrows.
            name = self.fresh("str")
            a = '"t' + str(self.rnd() % 1000) + '"'
            b = '"u' + str(self.rnd() % 1000) + '"'
            return [
                f"var {name} = {a} + {b};",
                f"print(len({name}));",
                *self.statement(vars, depth + 1, docs, pointers),
            ]
        if kind == 7:
            # A vector with pushes and a length read.
            name = self.fresh("vec")
            return [
                f"var {name} = vec[{self.int_expr(vars)}, {self.int_expr(vars)}];",
                f"push({name}, {self.int_expr(vars)});",
                f"print(len({name}));",
                *self.statement(vars, depth + 1, docs, pointers),
            ]
        if kind == 8:
            # A struct with two scalar fields, read field-wise.
            name = self.fresh("pt")
            return [
                f"var {name} = Pt {{ a: {self.int_expr(vars)}, b: {self.int_expr(vars)} }};",
                f"print({name}.a + {name}.b);",
                *self.statement(vars, depth + 1, docs, pointers),
            ]
        if kind == 0:
            name = self.fresh("x")
            return [f"var {name} = {self.int_expr(vars)};"] + self.statement(vars + [name], depth + 1, docs, pointers)
        if kind == 1:
            return [
                f"if {self.bool_expr(vars)} {{",
                *self.statement(vars, depth + 1, docs, pointers),
                "} else {",
                *self.statement(vars, depth + 1, docs, pointers),
                "}",
            ]
        if kind == 2:
            name = self.fresh("n")
            bound = self.int_expr(vars)
            return [
                f"var {name} = {bound};",
                f"var {self.fresh('i')} = 0;",
                *self.statement(vars, depth + 1, docs, pointers),
            ]
        if kind == 3:
            return [f"print({self.int_expr(vars)});"]
        if kind == 4:
            return [
                f"if {self.bool_expr(vars)} {{",
                f"    print({self.int_expr(vars)});",
                "}",
                *self.statement(vars, depth + 1, docs, pointers),
            ]
        if kind == 14:
            # A Doc with two string fields opens the borrow surface: the
            # `&mut self` method, the free `&mut` function, the field view,
            # and the L2 field-granularity write. Views are born and die
            # inside one statement group, so no borrow state outlives a
            # statement and every emitted shape is one the parser accepts.
            name = self.fresh("d")
            return [
                f'var {name} = Doc {{ name: "alpha{self.rnd() % 100}", note: "keep" }};',
                *self.statement(vars, depth + 1, docs + [name], pointers),
            ]
        if kind == 15:
            # The L2 field-granularity face (ADR-0205): a view of `name`
            # stays readable across a write to the *other* field, then dies
            # at its read. The four modes must agree on both printed lines.
            d = self.pick(docs)
            h = self.fresh("h")
            return [
                f"let {h} = slice_view({d}.name, 0, 2);",
                f'{d}.note = "m{self.rnd() % 100}";',
                f"print({h});",
                f"print({d}.note);",
                *self.statement(vars, depth + 1, docs, pointers),
            ]
        if kind == 16:
            # A `&mut` lending call (M2.5 slice 3): no views exist anywhere,
            # so the call's whole-variable release site is uncovered and it
            # parses; the modes must agree on the write landing, the return
            # value, and the renamed field.
            d = self.pick(docs)
            n = self.fresh("n")
            if self.rnd() % 2 == 0:
                call = f'{d}.rename("r{self.rnd() % 100}")'
            else:
                call = f'doc_bump(&mut {d}, "r{self.rnd() % 100}")'
            return [
                f"let {n} = {call};",
                f"print({n});",
                f"print({d}.name);",
                *self.statement(vars, depth + 1, docs, pointers),
            ]
        if kind == 17:
            # The ordered face (M2.5 slice 3): the view is read by the first
            # argument — its last use — and the mut call releases after it;
            # left-to-right evaluation is what keeps this accepted.
            d = self.pick(docs)
            h = self.fresh("h")
            n = self.fresh("n")
            return [
                f"let {h} = slice_view({d}.name, 0, 2);",
                f'let {n} = doc_combine(len({h}), {d}.rename("r{self.rnd() % 100}"));',
                f"print({n});",
                *self.statement(vars, depth + 1, docs, pointers),
            ]
        if kind == 18:
            # A raw pointer to a scalar (M2.12): the target never moves, so
            # the pointer stays valid for its whole scope. Every generated
            # scalar is a `var`, so the mutable form is always legal.
            target = self.pick(vars)
            name = self.fresh("ptr")
            if self.rnd() % 2 == 0:
                return [
                    f"let {name} = &{target} as *const i64;",
                    *self.statement(vars, depth + 1, docs, pointers + [(name, target, False)]),
                ]
            return [
                f"let {name} = &mut {target} as *mut i64;",
                *self.statement(vars, depth + 1, docs, pointers + [(name, target, True)]),
            ]
        if kind == 19:
            # A deref read inside unsafe (M2.12 slice 2): the pointer and the
            # variable agree on every backend.
            name, target, _ = self.pick(pointers)
            return [
                f"unsafe {{ print({name}.deref()); }}",
                f"print({target});",
                *self.statement(vars, depth + 1, docs, pointers),
            ]
        if kind == 20:
            # A write through a mutable pointer (M2.12 slice 3): the variable,
            # the pointer, and any later pointer all observe the write.
            name, target, mutable = self.pick(pointers)
            if not mutable:
                name, target, mutable = next(((p, t, m) for (p, t, m) in pointers if m), (None, None, False))
            if name is None:
                return [f"print({self.int_expr(vars)});"]
            return [
                f"unsafe {{ *{name} = {self.int_expr(vars)}; }}",
                f"print({target});",
                f"unsafe {{ print({name}.deref()); }}",
                *self.statement(vars, depth + 1, docs, pointers),
            ]
        name = self.fresh("s")
        return [
            f"var {name} = {self.int_expr(vars)};",
            f"print({name} > 0 || {name} <= 0);",
            *self.statement(vars, depth + 1, docs, pointers),
        ]

    def program(self) -> str:
        # `main` cannot `return` in this language; its exit is implicit. The
        # struct Pt and its two scalar fields are fixed: field reads borrow.
        # The Op enum exercises match dispatch and payload bindings.
        lines = [
            "struct Pt { a: i64, b: i64 }",
            "struct Doc { name: string, note: string }",
            "enum Op { Wrap(i64), Pair(i64, i64), Flat }",
            "fn eat(t: string) -> i64 { return len(t); }",
            "impl Doc {",
            "    fn rename(&mut self, to: string) -> i64 {",
            "        self.name = to;",
            "        return len(self.name);",
            "    }",
            "}",
            "fn doc_bump(d: &mut Doc, to: string) -> i64 {",
            "    d.name = to;",
            "    return len(d.name);",
            "}",
            "fn doc_combine(a: i64, b: i64) -> i64 {",
            "    return a + b;",
            "}",
            "fn main() {",
        ]
        vars = ["seed0"]
        lines.append("    var seed0 = 7;")
        body = self.statement(vars, 0, [], [])
        lines.extend("    " + line for line in body)
        lines.append("}")
        return "\n".join(lines)


def compile_at(source: str, workdir: Path, tag: str, opt: bool, tree: bool = False,
               cache_dir: str = "", timeout: int = 120) -> Path:
    src = workdir / f"prog_{tag}.ae"
    src.write_text(source, encoding="utf-8")
    # `build` optimizes by default; --debug compiles the same source at -O0.
    # AE_CODEGEN=tree switches that one compile to the structured tree
    # emitter, so the MIR backend's C is differentiated against it (M2.3).
    # AE_CACHE points the worker at its own content cache, so parallel
    # workers never share mutable cache state with each other or with the
    # user's cache.
    env = {**os.environ, "AE_CODEGEN": "tree"} if tree else None
    if cache_dir:
        env = {**(env or os.environ), "AE_CACHE": cache_dir}
    result = subprocess.run(
        [str(EXE), "build", str(src), "-o", str(workdir / f"prog_{tag}")] + ([] if opt else ["--debug"]),
        capture_output=True,
        text=True,
        env=env,
        timeout=timeout,
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


def _run_dump(runs: list[tuple[str, subprocess.CompletedProcess]]) -> str:
    """Every mode's exit code, stdout, and stderr, for a mismatch report."""
    blocks = []
    for name, run in runs:
        blocks.append(f"--- {name} (exit {run.returncode}) ---{NL}{run.stdout}{NL}[stderr]{NL}{run.stderr}")
    return NL.join(blocks)


def run_seed(seed: int, workdir: Path, cache_dir: Path, compile_timeout: int) -> dict:
    """One seed's four-way differential, fully isolated.

    The seed's files live in their own directory and the compiles run with
    their own content cache, so parallel workers never share mutable state
    with each other or with the user's cache. Returns a result dict; never
    raises (every failure becomes a result).
    """
    started = time.perf_counter()
    result = {"seed": seed, "ok": None, "detail": "", "lines": 0, "source": ""}
    gen = Gen(seed)
    source = gen.program()
    result["source"] = source
    seed_dir = workdir / f"s{seed}"
    seed_dir.mkdir(parents=True, exist_ok=True)
    cache = str(cache_dir)
    try:
        debug_bin = compile_at(source, seed_dir, "d", opt=False, cache_dir=cache, timeout=compile_timeout)
        tree_bin = compile_at(source, seed_dir, "t", opt=False, tree=True, cache_dir=cache, timeout=compile_timeout)
        release_bin = compile_at(source, seed_dir, "r", opt=True, cache_dir=cache, timeout=compile_timeout)
        runs = []
        for bin_path in (debug_bin, tree_bin, release_bin):
            runs.append((bin_path.name, subprocess.run([str(bin_path)], capture_output=True, text=True, timeout=30)))
        interp_run = subprocess.run(
            [str(EXE), "run", str(seed_dir / "prog_d.ae"), "--interp"],
            capture_output=True,
            text=True,
            timeout=30,
        )
        runs.append(("interp", interp_run))
        # A runtime trap is part of the language's observable behavior, so
        # agreement includes traps (seed 1762's overflow): every mode must
        # exit with the same code, print the same stdout up to the trap, and
        # trap at the same position with the same message. ADR-55 exempts
        # diagnostic wording for compile and file errors, not runtime traps;
        # the interp's `ae: ` prefix spacing is normalized away.
        codes = {run.returncode for _, run in runs}
        outs = [run.stdout for _, run in runs]
        traps = [re.match(r"ae:\s*(\d+:\d+): (.+)", run.stderr.strip()) for _, run in runs] if codes != {0} else []
        if codes != {0} and (None in traps or len(set(t.groups() for t in traps if t)) != 1):
            result["ok"] = False
            result["detail"] = "TRAP MISMATCH" + NL + _run_dump(runs)
        elif len(codes) != 1 or len(set(outs)) != 1:
            result["ok"] = False
            result["detail"] = "OUTPUT MISMATCH" + NL + _run_dump(runs)
        else:
            result["ok"] = True
            result["lines"] = len(outs[0].splitlines())
    except (RuntimeError, subprocess.TimeoutExpired) as error:
        result["ok"] = False
        result["detail"] = str(error)
    result["seconds"] = time.perf_counter() - started
    return result


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--seeds", type=int, default=40)
    parser.add_argument("--start", type=int, default=0)
    parser.add_argument("--jobs", type=int, default=min(os.cpu_count() or 2, 8),
                        help="parallel workers; each gets its own files and content cache")
    parser.add_argument("--compile-timeout", type=int, default=120,
                        help="seconds before one compile is abandoned (and fails that seed)")
    parser.add_argument("--save-corpus", metavar="DIR",
                        help="write every generated program as DIR/seed_N.ae")
    args = parser.parse_args()

    if not EXE.exists():
        print(f"ae binary missing: {EXE}\nbuild it first: cargo build --workspace", file=sys.stderr)
        return 2

    jobs = max(1, args.jobs)
    started = time.perf_counter()
    results: dict[int, dict] = {}
    failures = 0

    with tempfile.TemporaryDirectory() as tmp:
        workdir = Path(tmp)
        cache_dir = workdir / "cache"
        corpus: dict[int, str] = {}
        seeds = list(range(args.start, args.start + args.seeds))

        if jobs == 1:
            for seed in seeds:
                result = run_seed(seed, workdir, cache_dir, args.compile_timeout)
                results[seed] = result
                failures += 0 if result["ok"] else 1
                mark = "ok" if result["ok"] else "FAILURE"
                print(f"seed {seed}: {mark} ({result['lines']} lines)", flush=True)
        else:
            from concurrent.futures import ThreadPoolExecutor
            with ThreadPoolExecutor(max_workers=jobs) as pool:
                futures = {pool.submit(run_seed, seed, workdir, cache_dir, args.compile_timeout): seed for seed in seeds}
                for future in futures:
                    pass
                from concurrent.futures import as_completed
                for future in as_completed(futures):
                    result = future.result()
                    seed = result["seed"]
                    results[seed] = result
                    failures += 0 if result["ok"] else 1
                    mark = "ok" if result["ok"] else "FAILURE"
                    print(f"seed {seed}: {mark} ({result['lines']} lines)", flush=True)

        if args.save_corpus:
            corpus_dir = Path(args.save_corpus)
            corpus_dir.mkdir(parents=True, exist_ok=True)
            for seed in seeds:
                gen = Gen(seed)
                (corpus_dir / f"seed_{seed}.ae").write_text(gen.program(), encoding="utf-8")
            print(f"corpus saved: {corpus_dir} ({len(seeds)} programs)")

    elapsed = time.perf_counter() - started
    order = sorted(results)
    failed_seeds = [seed for seed in order if not results[seed]["ok"]]
    for seed in failed_seeds:
        print("")
        print(f"seed {seed}: FAILURE")
        print(results[seed]["source"])
        print(results[seed]["detail"])
    rate = len(results) / elapsed if elapsed > 0 else 0.0
    print(f"\n{len(results)} seeds, {failures} failures, {elapsed:.1f}s ({rate:.2f} seeds/s, {jobs} jobs)")
    if failed_seeds:
        print(f"failed seeds: {failed_seeds}")
    return 1 if failures else 0


if __name__ == "__main__":
    sys.exit(main())
