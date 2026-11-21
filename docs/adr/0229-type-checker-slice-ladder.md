# ADR-0229: R3c step 2 — the type checker as an ordered slice ladder (proposed)

- Status: Proposed
- Date: 2026-11-21
- Associated Milestone: R3c of the ADR-0224 pipeline split

## Context

R3b (ADR-0227) and R3c step 1 (ADR-0228) delivered the resolver: def table,
occurrences, classified spellings, writability, and import following — all
differential-gated. What remains of R3c is the largest single arc in the
ADR-0224 split: a type checker that consumes the resolved tree and emits the
existing typed `Expr`, so MIR and codegen keep working. The frontend that
does this today in one pass is about 5.9k lines
(`parser.rs` 1274 + `expression.rs` 1866 + `declarations.rs` 1412 +
`types.rs` 812 + the `Expr` definitions in `tree.rs` 560).

This ADR decomposes the arc into an ordered ladder of slices, each complete
and gated on its own, so the mega-arc becomes executable without a long
red stretch. It is a planning ADR: implementation starts from slice 1 and
updates this ADR as slices land.

## The ladder (each slice ships green or does not ship)

1. **Binding types** (was: a standalone Type model — folded in here, since
   a model mirroring the classifier alone would be its duplicate without a
   consumer; the model earns its variants exactly where binding and
   expression types need them). Parameters type from their spellings
   (receivers: `&self` borrows, `&mut self` mutates, by-value `self` owns);
   `let`/`var` bindings infer from initializers in a bounded subset —
   literals, copies of typed bindings, calls to module functions (return
   spellings are already classified), vector literals, struct literals and
   closure literals (an `fn(params) -> ret` shape); `for`-range bindings
   are i64. Everything else is `deferred`, honestly marked. **Landed:**
   `DefEntry.inferred` carries the shape, `locals … = <shape>` /
   `= deferred` render in the dump (snapshots re-blessed), the corpus gate
   runs unchanged, and `infer_initializer` is the subset's home.
2. **Expression inference, bounded subset first:** literals, arithmetic and
   comparisons on primitives, local reads, vector and map literals,
   field/index reads, struct literals. Gate: expression-type dump snapshots
   pinned; differential on accepted corpus (no inferred contradiction).
   **Landed:** `infer_expression` covers literals, copies, module-fn calls,
   vector/struct/closure literals, same-primitive arithmetic (the language
   never mixes), comparisons and logic (bool), unary minus/not, field reads
   through struct defs and receiver borrows, index element types, pointer
   casts, and generic instantiations — everything else defers.
3. **Calls and builtins.** Argument-count and element checks against the
   def table; builtin/prelude surface classified per `RESERVED`/allowlist.
   Gate: same-differential as slice 2 over the call corpus. **Landed:** the
   two arity rules — value arity (`function '{name}' expects {n} arguments,
   got {m}`) for calls resolving to module functions, and type arity
   (`'{name}' takes {n} type argument(s), found {m}`) for specializations
   of module items — plus the unsafe gates (`ptr_write` / `ptr_drop_in_place`
   statement calls, `unsafe fn` and `extern "C"` calls outside an unsafe
   block → E0701) and struct-literal field validation (unknown field,
   field specified twice, missing field — the compiler's texts, probes
   u1-u3). Receiver-method calls stay out (Field callees; the
   arity is type-checker material), prelude types stay out (no module def),
   imported functions keep their arity checked but never collide with
   local names.
4. **Statement and control-flow checking** (conditions must be bool, match
   exhaustiveness, return type agreement) — the checker starts REJECTING.
   Gate: `tests/ui` accepted/rejected verdicts reproduced by the checker on
   the resolved tree, independent of the frontend. **Landed:** condition
   checks for `if`/`while` (`condition must be a boolean expression`),
   return agreement against the declared spelling (`return type must be
   string`), and match exhaustiveness with instantiated variant names
   (`match is not exhaustive: 'Opt[i64]::None' is not handled`); deferred
   shapes stay silent. Remainder landed: `function may exit without
   returning a value` — a per-block path analysis (`return` anywhere
   decides; `if` needs both branches; a `match` needs every arm; `unsafe`
   recurses; a `while` never guarantees — the compiler rejects even
   `while true`, probe r2) with the diagnostic on the body's closing
   brace, exactly where the compiler points (probes r1-r5).
5. **Frontend switchover.** The compiler's Parser consumes the resolved
   tree instead of re-parsing tokens; `emit-c`/`emit-mir`/`run` output must
   be byte-identical on the whole corpus (the strongest gate of the arc, and
   the one that finally closes M0.2.08).
   Run `python scripts/compare_emit_c.py --baseline <before> --candidate
   <after> --frontend-corpus` with independently built binaries. The gate
   compares example C and MIR, run-fixture C and MIR, debug/release execution,
   and diagnostic fixtures.
6. **LSP rebase** (R3d) after 5: hover/definition/rename/completion read
   the def table and types instead of matching text.

## What stays with the compiler (for now)

* Instantiation of generics (M1.7's template machinery) until slice 6.
* Visibility (`pub`), move/borrow checking (M2.5), unsafe gating (M2.12) —
  separate passes that already sit behind the parser; they move last.
* The error-text catalogue: the checker must reproduce the compiler's
  diagnostics verbatim on `tests/ui` before slice 6 may land.

## Risks and honest notes

* The single-pass frontend interleaves moves, borrows and types; slicing
  types out first means the intermediate state has duplicated verdicts —
  the gates are what keep the two copies honest until switchover.
* Slices 3-4 may reveal grammar shapes the classifier cannot type (as the
  import-following gate revealed two grammar bugs); each one is a recorded
  fix or a recorded gap, never a silent pass.
* Trait dispatch (`resolve_trait_call`) is generic-instantiation territory:
  it stays compiler-side until slice 6, and slice 5's checker treats it as
  deferred verdicts.

## Evidence

- Frontend sizing: `wc -l` over the five frontend modules (this ADR's
  Context), taken 2026-11-21 at `0083348` (plus uncommitted R3b/R3c work).
- Dual-platform verification: the slices landed and were re-run green on
  the WSL2 Linux leg of 2026-11-21 (same working state).
- Existing gates this ladder plugs into:
  `resolver_agrees_with_the_compiler_on_accepted_programs`
  (`tests/execution.rs`), `tests/resolve` snapshots, `tests/ui` /
  `tests/run` snapshot families for slice 5's verdict reproduction.
