# ADR-0206: Constant Array Indices Skip the Bounds Check (M2.10)

- **Date**: 2026-11-14
- **Status**: Implemented (Windows-verified; Linux coverage rides the CI matrix on the next push)
- **Milestone**: M2.10 — "bounds check elimination (proof required)"

## Context

Fixed arrays carry their length in the type (`Type::Array(len)`), and every
index read/write emits a runtime `ae_index(index, len, line, col)` check.
When the index expression is a compile-time constant, the check's outcome is
decided at compile time: the proof is the constant's range against the
type-carried length — `0 <= n < len` needs no runtime work, and its negation
is a guaranteed trap.

The plan's M2.10 marks bounds check elimination "proof required". The proof
here is the trivial one: a literal's value is known, the array's length is
known from the type, and the comparison is total. The precedent for the
out-of-range case is ADR-0198's constant folding: `1 / 0` stays unfolded so
the runtime trap fires where the source points — an out-of-range constant
keeps its check and traps the same way.

Vectors are **not** covered: a `vec`'s length is a runtime value that `push`
and `pop` change, so no compile-time proof exists. The check stays.

## Options

| Option | Pros | Cons | Resource Cost |
|---|---|---|---|
| Keep every check | Simplest | Provably-dead checks execute on every array access | Runtime: a call + branch per access |
| Elide for in-range constants; keep otherwise | Zero-cost proven-safe accesses; traps unchanged | One more rule to audit | Compile: one range comparison on a literal |
| Compile-time rejection of out-of-range constants | Fails fast | Changes runtime-trap semantics the const-prop precedent fixed (ADR-0198) | — |

## Decision

1. **In-range constants elide the check.** An `Expr::Number(n)` index with
   `0 <= n < len` emits the direct element access `v{id}[{n}]`; no
   `ae_index` call, no temporary.
2. **Out-of-range constants keep the check.** The trap fires at runtime with
   the same message and position the dynamic check produces — the semantics
   never change, only the provably-dead checks disappear.
3. **Vectors keep their checks.** A runtime length has no compile-time
   proof; the elision applies to `Type::Array` only.

## Evidence

- `constant_array_indices_in_range_skip_the_bounds_check`: the C for a
  three-element array program with three constant reads and one constant
  store contains zero `ae_index` calls; the program runs `10/99/30`; an
  out-of-range constant traps with `array index out of bounds`.
- `cargo test --workspace --locked` — 280 tests, 0 failures.
- `python scripts/diff_fuzz.py --seeds 40` — 0 failures (the fuzzer does not
  generate fixed arrays; the elision is exercised by the pinned test).
- `python scripts/verify_generated_c.py` — 41 PASS / 0 FAIL.
- Single-platform verification (Windows).

## Consequences

- Fixed-array access with constant indices is free of runtime checks; the
  generated C for array-heavy programs shrinks accordingly.
- The audit surface is one comparison on a literal — the proof is the
  constant's range against the type-carried length, re-checkable by reading
  the two arms in `codegen.rs`.
- Revisit criteria: if a check elision is ever demonstrated unsound, the
  suspect is the `len` carried by the statement versus the value's real
  type — the pinned test's OOB half guards the trap path.
