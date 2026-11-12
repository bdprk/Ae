# ADR-0190: `emit-tree` — the Inspectable Layer Above the Parser

## Status

Implemented (Windows-verified: the full `cargo test --workspace --locked` suite including a whole-suite sweep of 136 run-suite and example files, plus an output-shape regression test; Linux coverage rides the CI matrix on the next push).

## Context

Plan M2.2 asks for a MIR with a validator and a text printer (`emit --mir`). Before that construction can be reviewed or tested, one layer is missing entirely: nothing can show what the front end *currently* lowers. The compiler emits C straight from its statement tree, and the only way to see that tree was to read the generated C and mentally invert the runtime calls. A borrow checker built on top of an uninspectable tree cannot have its diagnostics reviewed.

The statement tree is also the object every later M2 phase consumes: the borrow checker checks it, MIR construction lowers it. Whatever shape those phases want to see, they want to see *this* first.

## Decision

**`ae emit-tree <source.ae>` prints the lowered statement tree as text.** Declarations first (structs with field types, enums with variant payloads), then each function: `fn #id(params) -> ret` with its statements indented one level per block shape (`if`/`while`/`for`/`match` nest their bodies). Every statement prints with the data the later phases need: variable ids, types, source positions, bounds-check lengths, release annotations, map kinds, loop labels.

**Every variant has a form, enforced by the compiler.** The printer matches over `Statement`, `Expr`, `Argument`, `MapPlace`, and `Printed` exhaustively — a new variant without a printed form fails the build instead of silently printing a placeholder. (The generic `<expr ...>` discriminant fallback exists only as a build-time signal, not an accepted output.)

**Two print-time judgments are deliberate**: an empty release list prints nothing (the lowering attaches them on paths with nothing owned), and a `Frees` statement prints its list bare (`release v0: struct #0`) while the same list as an annotation on `return`/`exit` prints parenthesized — the distinction is statement vs. annotation, which the borrow checker treats differently.

**The command bypasses the frontend reuse cache**, like `emit-c`: its output *is* the frontend's answer, so a cache hit standing in for it would be a lie about the current source.

## Evidence

Re-runnable:

- `cargo test --workspace --locked` — 202 passing, including:
  - `emit_tree_dumps_the_lowered_statement_tree` — output shape on a small program (struct declaration, main's signature, a struct-literal assignment, a field read);
  - `emit_tree_runs_clean_on_the_run_suite` — `emit-tree` succeeds over every `.ae` in `tests/run` (currently 87 files), asserting the printer has no missing arms and no panics on any construct the suite uses.
- Manual sweep: all 136 `tests/run` + `examples` files print successfully.

## Consequences

- M2.2's MIR construction now has its input visible; the borrow checker's future diagnostics can cite tree nodes by variable id and position.
- The printer is a stable fixture format: tooling (the future LSP, formatter tests) can snapshot trees instead of C.
- Known gaps, stated rather than implied: expression printing favors completeness over beauty (positions inline everywhere); generic-instantiation bodies print where they were parsed, not in file order; the tree is per-program (imports inline), which makes large-program dumps large.
- Next: M2.2 continues with the MIR definition and the tree→MIR lowering.
