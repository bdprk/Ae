# ADR-0189: Function Types — Closures as Values

## Status

Implemented (Windows-verified: full `cargo test --workspace --locked` suite, the `ui`/`json`/`run` snapshot suites, and `scripts/verify_generated_c.py` including the new `examples/function_types.ae` case built with `-pedantic-errors -Werror=return-type -O1`; Linux coverage rides the CI matrix on the next push).

## Context

ADR-0188's closures could be bound and called locally, but not *handed to anything*: a parameter had no way to spell "a function", so `map`/`filter`-shaped code — the actual point of M1.8 — stayed out of reach. The missing piece was a type for the closure value itself, and a calling convention for it.

The C backend made this cheaper than it sounds: every generated closure function is already a static function in one translation unit, so a non-capturing closure's value *is* its function pointer, and an indirect call is a plain C call through the pointer.

## Decision

**`fn(T, U) -> R` is a parameter type.** The signature is interned (`fn_sigs` registry) so two closures of the same shape share one type — the same interned-signature trick the C type system does implicitly. `Type::Fn(sig)` is not owned, has no storage as a local, and cannot be a return type or a field (diagnostics say so at the annotation).

**Only non-capturing closures can be passed.** The argument must be a closure literal whose parameter list and return type match the signature exactly; a literal with captures is rejected with the workaround named (bind its inputs as arguments). A capturing closure has no representation without its environment, and building one — a context struct plus function pointer — is M2-scale work that this ADR deliberately does not start.

**The pointer travels inline in the argument list.** `Argument::FnPtr(cid)` emits `ae_fn_N` directly into the call's argument text — no temporary, because a temporary of function type would need C's parenthesized declarator form (`int64_t (*t0)(int64_t)`), which the `{type}t{id} = value;` temp template cannot print. Inside the callee, `f(x)` is `Expr::FnCall`: an indirect C call through the parameter slot, typed by the interned signature.

**Named functions are not values yet.** `apply(double, 5)` is rejected ("must be a closure literal") — referencing a function without calling it needs its own syntax and a decision about overloading, so it stays out of this ADR.

## Evidence

Re-runnable:

- `cargo test --workspace --locked` — 200 passing; `tests/run/function_types.ae` exercises one parameter called with two different closures, and a two-call composition.
- `python scripts/snapshots.py` — three new `ui` diagnostics:
  - `fn_capture_rejected`: `a closure with captures cannot be passed as a function value`;
  - `fn_shape_mismatch`: `argument 1 to 'apply' must be a closure of type fn(i64) -> i64, found fn(i64, i64) -> i64`;
  - `fn_non_closure_arg`: `argument 1 to 'apply' must be a closure literal of type fn(i64) -> i64, found i64`.
- `python scripts/verify_generated_c.py` — 41 cases, `function_types.ae` compiled and output-checked.

Shape check, `cargo run -- emit-c examples/function_types.ae`:

```
static int64_t ae_fn_0(int64_t (*v0)(int64_t), int64_t v1) {
    int64_t t0 = v0(v1);                    // indirect call through the parameter
...
    int64_t t1 = ae_fn_0(ae_fn_4, INT64_C(21));   // closure pointer, inline
```

The parameter is a C function pointer; the callee calls through it; the call site passes the generated closure function by name. Nothing else changes.

## Consequences

- M1.8's adapters are unblocked: `map`/`filter`-shaped functions can now take the transforming closure as a parameter.
- The closure story is coherent end to end: bind and call locally (ADR-0188), pass parameter-only closures to functions (this ADR), capture-carrying values wait for a context representation.
- Known gaps, stated rather than implied: no named-function references as values; no function-typed return types or fields; no generic function parameters (`fn(T) -> T` as a parameter shape — the interning is monomorphic per signature); the `fn` keyword is now reserved in parameter-type position.
- Next per the roadmap: the `Iterator` trait and its adapters, or M2 (MIR construction and the borrow checker).
