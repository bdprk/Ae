# ADR-0228: R3c step 1 — type-spelling resolution (proposed)

- Status: Implemented (Single platform)
- Date: 2026-11-21
- Associated Milestone: R3c step 1 of the ADR-0224 pipeline split

## Context

R3b (ADR-0227) shipped the resolver: a flat def table over the shared grammar
plus per-function occurrences, gated by a differential corpus. The remaining
R3b gaps (receiver method names, import following) and R3c itself both need
the same next primitive: **type spellings must resolve**. Today a spelling
lives as a byte range on `TypedName.ty` / `DeclarationKind::Function.returns`
/ `ClosureParam.ty` / struct fields; only the compiler's single-pass frontend
can read it (`crates/ae-compiler/src/frontend/types.rs`).

This ADR mapped the spelling surface and proposed the slice; the slice
landed the same day (sections below updated from Proposed to what shipped).
Every entry cites the compiler site it must stay in sync with.

## The type-spelling surface (surveyed 2026-11-21)

Entry points in `crates/ae-compiler/src/frontend/types.rs`:

* `parameter_type` (types.rs:634) — the annotation dispatcher:
  - `*const T` / `*mut T` → `pointer_type` (types.rs:610): star + pointer
    mutability (`ae_syntax::declaration::pointer_mutability`) + pointee,
    interned (`Ptr`/`PtrMut`).
  - `fn(T, …) -> R` → fn values (types.rs:634 `Kind::Fn` arm): ≤ 8
    parameters, each via `callable_parameter_type` (borrowed read-only:
    `StrView`/`VecRef`/`StrVecRef`/`VecFRef`/`StructRef`/`StructMut`/`Array`),
    return via `return_annotation`; a fn type cannot return a borrow or
    another fn type.
  - `&T` / `&mut T` → borrowed parameter forms (same fn).
  - otherwise `named_type`.
* `named_type` (types.rs:23) — the named forms:
  - Primitives: `i64`, `f64`, `bool`, `string` (types.rs:30-33).
  - Copy-semantics handles: `atomic`, `mutex`, `guard`, `chan`, `exec`,
    `task_scope`, `pool`.
  - `shared[T]` — element restricted to `i64|bool|f64|atomic`; no handle
    towers (types.rs:51).
  - `vec` (= i64 vector), `vec[string]`, `vec[f64]` (types.rs:63).
  - `map[string,i64]` / `map[string,f64]` — keys must be `string` (types.rs:80).
  - Generic instantiations: `Name[T1, …]` where Name is a generic enum or
    struct; arguments via `type_argument` (types.rs:401) restricted to
    `i64|bool|f64|string|vec|vec[string]|vec[f64]`.
  - `Self` — only inside an impl block (ADR-0186).
  - Plain struct/enum names; anything else fails `unknown type '{name}'`.
* Generic declaration fields (`field_type`, types.rs:413) additionally allow
  **type parameters** (`Payload::Param`) and **nested instantiations**
  (`Payload::Nested`) whose arguments may themselves be parameters —
  `inner: Pair[string, V]` inside `struct Boxed[V]`.
* Array types (`Type::Array(n)`) have no annotation spelling today; they are
  born from literals, and parameter spellings borrow them (`&arr`).
* `StrView` and `Tokens` have no spelling either; they are born from
  builtins (`slice_view`, `split`, token cursors).

## What shipped

1. **Name-level classifier in `crates/ae-resolve/src/types.rs`.** Every
   declarative spelling (parameters, returns, struct fields, variant
   payloads) classifies into: primitive / handle / composite (`shared`,
   `vec`, `map`) / pointer / borrow / fn value / named-item (a def id) /
   type-parameter / generic-instance / prelude (`PRELUDE_TYPES`) / unknown.
   No `Type` ids, no instantiation, no visibility — the compiler stays the
   semantic authority. Bare receivers keep an empty range and are skipped.
2. **Import following** (`crates/ae-resolve/src/imports.rs`): `use` headers
   resolve against the file system transitively (canonical-path cycle guard,
   64-file cap); `resolve_with_imports` exposes the imports' `pub` items
   under free names (`DefEntry.imported` marks them), so multi-file programs
   — and R3c after them — see the same surface the compiler does. The
   grammar records `pub` on `Declaration` now.
3. **Dump extension**: a `types:` section grouped by owner, pinned by the
   re-blessed `tests/resolve` snapshots.
4. **Differential gate**: the corpus gate now also asserts no unknown
   spelling on any compiler-accepted program — which is exactly what caught
   the two grammar bugs below — and the resolver enforces the compiler's
   two-level writability model on accepted programs: rebinding a binding
   needs `var`, writes through it need `var` or a `&mut T` / `*mut T`
   parameter or `&mut self` receiver (probes w1-w9), producing the
   compiler's `cannot assign to immutable variable` text. `*p = v` through
   a pointer is a `deref-write` and never checks the pointer binding's own
   mutability.
5. **Honest gaps recorded**: instantiation, visibility, and borrow-flavor
   checking stay with the compiler; trait bodies stay opaque. (Closure
   parameter and return spellings were classified in the same-day follow-up:
   the walker classifies them against the enclosing function's type
   parameters and impl target.)
   Generic-type bounds (`[T: Pair[string, i64]]`) are NOT a gap: the language
   allows trait-name bounds only (`frontend/types.rs:306-310` rejects
   anything else), so the grammar stopping on them breaks no parity.

## Grammar bugs the gate caught (same day)

* `Name[expr] {` — a bracket list followed by a brace was always a generic
  instantiation, so `if arr[i + 1] {` tried to parse `i + 1` as type
  arguments. The specialization arm now falls back to indexing when the
  bracket contents are not a type spelling (the BracketedCallTarget arm
  already had that fallback).
* `expression()` clobbered `no_struct` to `false` on return, so a sibling
  operator after an infix right operand (`b > 0 && a > x - b {`) lost the
  condition position and `b {` became a struct literal. The tail now
  restores the entry value.

Both bugs lived outside the R3a gate corpus (tests/run + examples had no
`&&`-with-trailing-arithmetic condition and no indexed-then-brace shape);
std/core — which the import-following gate now walks — had both.

## Evidence

- `cargo test --workspace --locked` on Windows: 476 passing (14 bin + 322
  execution + 2 intrinsic-freeze + 11 lsp + 5 snapshot + 44 compiler + 10
  resolver + 2 span + 67 syntax), including the extended gate. 0 warnings.
  Dual-platform verification (Windows + the WSL2 Linux leg of 2026-11-21, same working state).
- `verify_generated_c.py` 41 PASS / 0 FAIL; `diff_fuzz.py` 40 seeds /
  0 failures (both after the grammar fixes).
- Survey sources: `crates/ae-compiler/src/frontend/types.rs` lines 23-159
  (`named_type`), 401-470 (`type_argument`, `field_type`), 610-665
  (`pointer_type`, `callable_parameter_type`, `parameter_type`); prelude
  surface `crates/ae-compiler/src/frontend/mod.rs:263-300`.

## Consequences

- `docs/claims.md`: no public claims; internal pipeline work.
- The compiler's frontend is untouched; the classifier fails safe (unknown
  classifications become diagnostics in the dump, never compile errors).
- Revisit when M5.3 (query engine) or R3d (LSP) needs types on hover: the
  def-table extension in step 2 is the seam.
