# ADR-0227: Resolver pass over the shared grammar (R3b)

- Status: Implemented (Single platform)
- Date: 2026-11-21
- Associated Milestone: R3b of the ADR-0224 pipeline split (see `docs/implementation-ledger.md`)

## Context

ADR-0224 froze the intrinsic surface and ordered the frontend split: (a) route
the CLI through the `ae-syntax` AST (R3a — the statement layer and its two
differential gates landed 2026-11-21), (b) a resolver pass producing DefIds
over that AST, (c) a type checker over the resolved tree. Until (b) exists,
the only name map lives inside the compiler's single-pass frontend, so the
LSP's text matching (M6.2) has nothing to stand on and the future type
checker (R3c) has no input.

The grammar-only layer keeps its promise — `ae_syntax` accepts anything the
compiler rejects for semantic reasons — but two declaration shapes turned out
to be missing entirely from the shared grammar while the differential corpus
uses them: impl methods (impl bodies are opaque ranges, so `fn` methods inside
them were unreachable) and the receiver spellings `self` / `&self` /
`&mut self`, which carry no type annotation.

## Options

| Option | Pros | Cons | Resource Cost (Compile/Runtime/Memory/Disk) |
|---|---|---|---|
| Resolver inside `ae-syntax` | one crate less | breaks the crate's grammar-only contract; diagnostics would live beside the grammar | none |
| Resolver in the compiler frontend | reuses existing name maps | re-couples the pipeline; R3c could not consume it without the compiler | none |
| **New crate `ae-resolve`** over `ae-syntax` + `ae-span` | clean layering (grammar → resolve → typecheck), dumpable, LSP-usable | a new crate to maintain | negligible: one extra pass over tokens |

## Decision

- **New workspace crate `crates/ae-resolve`.** `parse_module` assembles one
  source file into top-level declarations plus per-function statement trees.
  Impl bodies are expanded by re-parsing the body's token span with the same
  declaration parser (token offsets are absolute, so method ranges land where
  the source has them). Trait bodies stay opaque: trait methods are generic
  dispatch surface for the type checker.
- **Flat def table.** Every item (function, struct, enum, variant, trait,
  impl) and every local (parameter, binding, for-binding, pattern-binding)
  gets one `DefId` in `Resolution::defs`; occurrences carry `role` and the
  `DefId` they resolve to. Locals number after items, in creation order.
- **Scope rules mirror the compiler's frontend.** Live names are a flat map
  with per-block removal: a name is reusable after its block ends, never
  while another binding of it is live (`E0104 duplicate variable`). Lookup is
  local first, then module item. A write target or borrow argument naming no
  live local is `E0101 unknown variable`; value positions that reach neither
  are `target: None` — in an accepted program that is builtin/prelude/import
  surface, which this single-file pass does not own yet.
- **Grammar additions in `ae-syntax` (additive):** `typed_names` accepts the
  receiver spellings `self`, `&self`, `&mut self` as a first parameter with an
  empty type range; `self: Type` rewinds into the plain typed-parameter path
  where the compiler owns the rejection. `Declaration` and friends derive
  `Clone`.
- **CLI:** `ae emit-resolve <source|project>` prints the deterministic dump
  (`resolve …`, items, per-function locals and occurrences, diagnostics).
  Snapshot family `tests/resolve/*.ae` → `*.resolve` pins it, blessed with
  `AE_BLESS=1`. A case may carry diagnostics in its dump; stderr stays empty.
- **Closure bodies resolve** (same ADR, extended same day): the resolver
  parses a closure's brace-balanced body with the shared statement grammar
  inside a nested scope; the parameters bind as `ClosureParameter` locals
  that die with the body. Capture semantics stay with the type checker.
- **Qualified members resolve** (same ADR, extended same day): a
  `Type::member` in expression position resolves to the variant of a known
  enum or to an inherent impl's method (trait impls dispatch through types
  and stay out). The grammar keeps the member as a bare string, so its range
  is scanned out of the tokens between the qualifier and the expression end.
- **Not done here (honest gaps, next slices):** imports are listed, not
  followed; method calls resolve their receiver, not the method (that needs
  the receiver's type); the prelude/builtin surface is `external`.

## Evidence

- `cargo test --workspace --locked` on Windows: 474 passing (14 bin + 322
  execution + 2 intrinsic-freeze + 11 lsp + 4 snapshot + 44 compiler + 8
  resolver + 2 span + 67 syntax), including the new gates below (474: 7
  resolver unit tests + 1 closure test).
  Dual-platform verification (Windows + the WSL2 Linux leg of 2026-11-21, same working state).
- R3b differential gate `resolver_agrees_with_the_compiler_on_accepted_programs`
  (`tests/execution.rs`): every compiler-accepted program in `tests/run` +
  `examples` parses through `parse_module`, resolves with zero diagnostics,
  and has every write/borrow target on a live local.
- Snapshot family `tests/resolve/` (locals, methods, calls, duplicates) pins
  the dump format and the diagnostic texts.
- `cargo test -p ae-resolve`: 7 unit tests on the def table, receiver
  spelling, pattern bindings, scope model, diagnostics and dump stability.

## Consequences

- `docs/claims.md`: no public claim changes; `emit-resolve` is a new tool
  surface, not a language claim.
- R3c (type checker over the resolved tree) starts from `Resolution`; M6.2
  LSP features re-base on the def table instead of text matching (R3d).
- Rollback: delete the crate, the CLI branch and the two test families; the
  compiler frontend is untouched and remains the enforcement authority.
