# ADR-0187: Generic Impl Blocks

## Status

Implemented (Windows-verified: full `cargo test --workspace --locked` suite, the `ui`/`json`/`run` snapshot suites, and `scripts/verify_generated_c.py` including the new `examples/generic_impl.ae` case built with `-pedantic-errors -Werror=return-type -O1`; Linux coverage rides the CI matrix on the next push).

## Context

Generic structs could hold data but not behaviour: `impl Pair[K, V]` was rejected, so every instantiation's operations lived in free functions while `Option`-shaped enums got methods for free (ADR-0186). The freeze list (§5.3) and the gap matrix both called this out — it is the last M1.5 item, and the machinery it needs already exists twice over: generic functions instantiate bodies per argument list (ADR-31), and inherent impls resolve methods through a (type, method) name space (ADR-0182).

## Decision

**`impl Pair[K, V] { ... }` declares a template, not signatures.** The signature pass stores only what the call site needs: the method's name, how its receiver was spelled (`self`/`&self`/`&mut self` → `ReceiverKind`), and the token range where its parameter list starts. Signatures and bodies are parsed **once per (method, argument list) pair** with the impl's parameters substituted — exactly how generic functions instantiate (ADR-31). This costs one design decision that a payload-template would have duplicated: type parameters may appear anywhere a type can appear in a signature, including nested (`Option[V]`) — the substitution map does the work, there is no second type grammar.

**Instantiation is cached and re-entrant-safe.** `generic_impl_done` caches by (declaration, method, argument list) so a method called twice on the same instantiation parses once; `impl_in_progress` rejects a recursive expansion with a message that names the pair, the same guard generic functions have. Each instantiation gets a function id from the one shared id space, so C names never collide.

**The call site cannot tell generic from concrete.** A method call on an instantiation falls back from the concrete inherent map to the struct's origin (`struct_origin: sid → (declaration, arguments)`, recorded at instantiation) and resolves through the declaration's impl template; `Pair[string, i64]::new(...)` resolves the same way after the type arguments are parsed. `Self` inside the body is the instantiated struct. Associated functions (no receiver) work and return through `Self`. The postfix loop still owns chaining — `Pair[string, i64]::new(..).count()` is a method call on the call's result.

**Coherence carries over unchanged**: one impl block per generic struct, in the file that declares it; method names may not collide with field names (checked against the declaration's fields, which every instantiation shares); method names may not be `main` or built-ins. Diagnostics list the methods the template has when a name misses.

**Receiver rules are the concrete ones, applied per instantiation**: by-value receivers move, `&self` auto-borrows (ADR-0184), `&mut self` borrows exclusively and writes through the pointer (ADR-0185). The receiver's concrete type is built from the kind and the instantiation — it is never spelled in the template.

## Evidence

Re-runnable:

- `cargo test --workspace --locked` — 200 passing; `tests/run/generic_impl.ae` exercises an associated constructor with `-> Self`, a `&self` method returning the instantiation's `V`, and two argument lists instantiating separate bodies.
- `python scripts/snapshots.py` — three new `ui` diagnostics:
  - `generic_impl_unknown_method`: `the impl for 'Pair' has no method 'nope'; it has count, new` (sorted list);
  - `generic_impl_duplicate`: `duplicate generic impl for 'Pair'`;
  - `generic_impl_field_collision`: the field-collision rule, checked against the declaration.
- `python scripts/verify_generated_c.py` — 39 cases, `generic_impl.ae` compiled and output-checked.

## Consequences

- M1.5 is complete: every row of the plan's method-syntax item is implemented (`self`, `&self`, `&mut self`, auto-borrow, methods and associated functions on structs and enums, generic impl blocks, `Self`).
- `std/core` can now express container behaviour generically — the first step toward `Vec[T]`-style standard containers replacing the hand-written per-element families (M1.10).
- Known gaps, stated rather than implied: no bounds/constraints on impl parameters (a method that needs `+` on `K` cannot say so — a constrained generic function is the workaround); no trait impls on generic types (`impl Show for Pair[K, V]`); `Pair[V, K]`-shaped field moves out of an owned receiver remain rejected by the blanket field rule (pre-existing, applies to concrete impls too); one impl block per generic struct (a second block over the same parameters is a duplicate, not an extension).
- Next per the roadmap: M1.8 — closures with capture inference, then the `Iterator` trait and its adapters.
