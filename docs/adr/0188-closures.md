# ADR-0188: Closure Literals with Capture Inference

## Status

Implemented (Windows-verified: full `cargo test --workspace --locked` suite, the `ui`/`json`/`run` snapshot suites, and `scripts/verify_generated_c.py` including the new `examples/closures.ae` case built with `-pedantic-errors -Werror=return-type -O1`; Linux coverage rides the CI matrix on the next push).

## Context

The gap matrix's "no closures or iterators" row is the expressiveness line between this language and Go/Rust. Method syntax (ADR-0182–0187) made types carry behaviour; what remained was behaviour as a *value* — a function body written where its data lives, referencing the surrounding scope directly instead of threading parameters through. The plan's M1.8 names exactly this: closures with capture inference, then the `Iterator` trait built on them.

The C backend offered a shortcut that shaped the whole design: every generated function is already a static function in one translation unit, and `ae_add`-style calls pass everything by position. A closure needs no method table, no context struct, no function pointer — only *hidden leading parameters*.

## Decision

**A closure compiles to an ordinary function; its captures are hidden leading parameters.** `|a: i64, b: i64| -> i64 { return a + b; }` emits `static int64_t ae_fn_N(int64_t v0, int64_t v1)` and the call `add(3, 4)` emits `ae_fn_N(3, 4)` — with the capture arguments spliced in before the visible ones. There is no closure object: the value has no storage, so it cannot be stored in a struct, returned (a dedicated diagnostic says why), or reassigned. A closure name is a name for its function id.

**Capture classification is Rust's Fn/FnOnce split, inferred from two probes.** The literal parses its body twice:

1. The *borrow probe* binds every borrowable capture in its read-only form (`string`→view, `vec`→`&vec`, `struct`→`&Struct`) and the rest as-is. If the body parses, every capture it moved would have been a type error — so all captures are borrows, and the closure is callable any number of times.
2. If that body fails — some capture is used where an owned value is required (`return note;` returns the capture itself) — the *owned probe* binds everything by value, and every used capture becomes a value capture: **each call moves the outer variable**, exactly like a plain call, which is what makes a second call a compile-time use-after-move instead of a double free.

Both probes are throwaway; only the last parse is emitted. An AST walk over the probe decides *which* names are captures (used parameter slots, in capture order) and rejects the one shape that cannot be honored: mutating a captured container or field, which would silently write a copy — the diagnostic says to write the loop in the enclosing scope.

**The enclosing scope keeps what the body borrowed.** Every borrow capture registers a persistent borrow on the outer variable (the same register a string view uses), so the variable cannot move or be written while the closure name is live; the loan ends at scope exit, and borrows taken *inside* the body end with each call because the callee owns nothing the caller depends on. Scalars always copy (no borrow form, no move), and maps — which cannot be borrowed — travel by value.

**`||` is the empty parameter list, disambiguated in the parser.** The lexer keeps `||` as `Or`; the expression grammar knows an or-expression is never followed by `->`, so `Or Arrow` can only be a closure with no parameters. One token of context, no lexer change, and the formatter's lexer-drift test stays intact.

**Parameter and return annotations are required.** The language has no inference for parameter types anywhere else, and the return type is what makes the call site checkable; a closure body must return on every path like any function, and may not return a borrow.

## Evidence

Re-runnable:

- `cargo test --workspace --locked` — 200 passing; `tests/run/closures.ae` covers multi-parameter closures, reusable scalar captures, borrow captures of strings and structs called twice, a value capture whose first call moves, and nested closures (the inner body captures from the middle scope).
- `python scripts/snapshots.py` — four new `ui` diagnostics:
  - `closure_mutate_capture`: `a closure cannot mutate captured variable 'items' in this version`;
  - `closure_return`: `a closure cannot be returned: it has no storage, and its captures live in this frame`;
  - `closure_capture_moved`: `cannot capture 'note' because it was moved`;
  - `closure_write_borrowed`: `cannot assign to 'greeting' while it is borrowed` (the borrow-capture loan).
- `python scripts/verify_generated_c.py` — 40 cases, `closures.ae` compiled and output-checked.

Shape check, `cargo run -- emit-c examples/closures.ae`:

```
static int64_t ae_fn_3(int64_t v0, int64_t v1);      // add: two visible params
static int64_t ae_fn_7(int64_t v0);                  // offset: `base` captured as hidden param 0
    int64_t t4 = ae_fn_7(v3, INT64_C(1));            // call reads the capture at the call site
```

The capture is an ordinary parameter; the call passes the caller's current value. Nothing else in the generated program changes.

## Consequences

- The `|` character is now a token (`Kind::Pipe`, formatter class `Pipe`); `a || b` remains logical-or.
- Closures are the substrate the `Iterator` trait needs: adapters can take closures once function-typed parameters exist.
- Known gaps, stated rather than implied: no `fn(T) -> R` parameter types yet, so closures cannot be passed to functions — `map`/`filter` adapters wait on that; no `FnMut` (a body that mutates its environment is rejected with the enclosing-scope workaround); closures cannot be stored in structs or returned; map captures are by value (one call).
- Next per the roadmap: M1.8 continues with function-typed parameters and the `Iterator` trait, or M2 begins (MIR construction and the borrow checker).
