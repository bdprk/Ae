# ADR-0195: Differential Debug/Release Fuzzing

## Status

Implemented (Windows-verified: 160 seeds across two runs, zero mismatches; the harness is committed as `scripts/diff_fuzz.py` and re-runnable with `python -I scripts/diff_fuzz.py --seeds N [--start S]`).

## Context

Plan M2.10 asks for differential testing: random programs, checked across execution modes. The C-backend version of that is debug vs. release — the same generated C compiled at `-O0` and `-O2` — because §2.2's rule 7 ("development and release semantics are identical") is exactly the invariant that optimization bugs would break. The suite's fixed tests cannot cover the space of operator/branch combinations; a generator can.

## Decision

**The generator produces *well-typed* programs, and the compiler's rejection of one is a harness failure.** This inverts the usual fuzzer contract: instead of accepting compile errors as outcomes, the generator is restricted to a grammar the parser accepts (i64 arithmetic, comparisons, boolean short-circuit, `if`/`else`, prints, fresh variables), so every seed also tests determinism of the accept/reject boundary. A compile failure means the generator wandered outside its grammar — a harness bug, fixed as such (the first run caught exactly one: `main` cannot `return` in this language).

**Division and modulo wrap their divisor in a provably nonzero guard**: `(b * b) + 1` is positive for every integer `b`, so generated programs are well-defined at runtime and runtime failures are also harness bugs. The first guard (`b / (b + 1) + 1`) divided by zero when `b == -1` — the fuzzer found its own generator bug on the first extended run, which is the mechanism working: any mismatch or failure names a seed, prints the program, and the cause is either a harness fix or a compiler bug with a reproducer.

**The oracle is output equality between the debug and release binaries** (stdout, exit status asserted zero). Both compile from the same generated C, so a mismatch names the optimization pipeline, and the seed reproduces it.

## Evidence

- 160 seeds (40 + 120 from a fresh start offset): all identical output, zero failures.
- The harness caught one generator bug (`main` return; divisor guard) — both fixed, both now permanent generator invariants.
- Each seed runs two compiles and two executions; 160 seeds ≈ 320 compiles, ~4 minutes wall on the dev machine.

## Consequences

- Debug/release semantic identity now has an automated, extensible oracle; new optimization work can run the fuzzer as a gate.
- Known gaps, stated rather than implied: the grammar excludes strings, vectors, structs, enums, and closures (moves make generated programs easier to make ill-typed than well-typed; the generator grows toward them); there is no MIR-interpreter oracle yet (the C backend is the only execution mode); output comparison is exact-text, so programs that print floating point must print deterministically (the generator only prints i64).
- Next: grow the generator toward owned types and more statement shapes, or add the MIR interpreter as a third execution mode for three-way differencing.
