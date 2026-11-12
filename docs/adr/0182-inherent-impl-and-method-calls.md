# ADR-0182: Inherent `impl` Blocks and Method Call Syntax

## Status

Implemented (dual platform, as far as the test suite goes: `tests/run/methods.ae` runs on both, and the diagnostics are pinned in `tests/ui`).

## Context

`docs/plan.md` §5.2 lists "no method syntax, closures, or iterators" as the language-surface gap that keeps expressiveness below Go and Rust, and `spec/methods_and_traits.md` describes a Rust-shaped target that the compiler does not have. What exists is a narrower mechanism (ADR-47): a trait declares methods over its own `T`, `impl Trait for Type` provides them, and a body can only call one when the receiver type arrives through a generic bound. That call is written `method(value, ...)` — plain function syntax.

So the language can describe per-type behaviour, but a struct cannot carry its own methods. Every program writes `rect_area(r)`, `point_shift(p, dx, dy)`, and the receiver is a parameter among others. That is the exact shape Go and Rust avoid, and it is the first thing a reader of `std/core` notices: 256 modules of data models, every one of them operating on structures through free functions.

An `impl` block already parses. `impl Counter { ... }` currently reports `impl names unknown trait 'Counter'`, because the parser reads the name after `impl` as a trait name and then demands `for`. The syntax a user would guess is already the syntax the tokenizer accepts; only the declaration and call paths are missing.

## Decision

Accept `impl Type { fn name(self, ...) -> R { body } }` and `value.name(args)`.

**A method is an ordinary function with the receiver as parameter 0.** This is not a design flourish — it is what makes the feature safe to add to a compiler that emits C directly from the AST. The declaration pass already allocates a function id, registers a `Signature`, and emits a body; the call path already builds `Expr::Call(id, args, ret)` and the backend already prints `ae_fn_<id>(...)`. A method needs one new name space and one new receiver spelling; it needs nothing from the backend, the runtime, or code generation. There is no method table, no vtable, no function pointer, and no indirection that a hand-written call would not have.

**Resolution is by the receiver's static type.** `value.name(args)` looks up `(struct name, method)` in the method map at compile time. A struct with no `impl` gets a diagnostic that names the declaration form; a struct with methods gets its method list, sorted, so a typo is a one-line fix.

**The receiver is by value, and `&self` is rejected.** Borrows in this language exist only for `string`, `vec`, `vec[string]`, `vec[f64]`, and maps. A struct cannot be borrowed, so `&self` would be a promise the type system cannot keep. Rather than accept `&self` and silently copy, the compiler rejects it and says why. The consequence is visible and intentional: calling a method on an owned value moves it, exactly as passing it to a function does, and `c.value(); c.value();` fails the way `f(c); f(c);` fails.

**`self` is a parameter name, not a keyword.** It is recognized only as the first parameter inside an inherent `impl`, where the impl already fixed its type, so the parameter list does not repeat it. Anywhere else, `fn f(self: i64)` reports that `self` is only available as a method receiver. This keeps the tokenizer unchanged and keeps the spelling from leaking into ordinary functions.

**Declaration rules**, mirroring the trait coherence rules already in place:

- the `impl` must be in the file that declares the struct, so two files cannot give one type two methods of the same name;
- the `impl` must follow the struct's declaration (the signature pass is single-sweep);
- generic structs are out of scope for v1 (`impl Pair[K, V]` is rejected with a message saying so);
- a method may not be named `main`, may not use a built-in name, may not repeat a field name (because `value.name` would then have to mean two things), and may not repeat another method on the same type;
- `pub` is accepted and is a no-op: a method is exactly as visible as the type it belongs to.

**Known gaps, stated rather than implied**: no methods on enums, no associated functions (`Point::new()` needs `::` on a type name, which the enum-variant syntax already occupies), no generic impl blocks, and no `self.other()` — that call moves `self`, so shared logic goes in a free function, which is also the rule for trait impl bodies.

## Evidence

Re-runnable:

- `cargo test --workspace --locked` — 255 passing.
- `python scripts/snapshots.py` — `ui`, `json`, and `run` suites, including the new `tests/run/methods.ae` in both debug and release.
- `python scripts/verify_generated_c.py` — `examples/methods.ae` in the case list, so the generated C is also built with `-pedantic-errors -Werror=return-type -O1` and checked against its expected output.
- New diagnostics pinned as `tests/ui/method_{borrow_receiver,unknown_name,missing_impl,field_collision,self_outside}.stderr`.

Shape check, `cargo run -- emit-c examples/methods.ae`:

```
static int64_t ae_fn_0(ae_t0 v0);          // fn abs_i64
static ae_t0 ae_fn_1(ae_t0 v0, ...);   // fn shifted(self, dx, dy)
    ae_t0 t5 = ae_fn_1(t4, INT64_C(1), INT64_C(2));
```

The method is a static function taking the struct by value; the call passes the receiver first. Nothing distinguishes it from a plain call.

## Consequences

- `spec/methods_and_traits.md` describes the target (implicit `&self`, auto-borrow, receiver resolution order, orphan rules); the implemented subset is this ADR's, and the spec now carries a status note pointing here.
- `docs/plan.md` §5.2's "no method syntax" entry becomes "no closures or iterators; inherent methods landed in ADR-0182".
- The gap matrix does not move for closures or iterators: neither is easier after this ADR, and nothing here touches the borrow checker, MIR, or the backend.
- Unblock, not a claim: methods make `std/core`-shaped code shorter, but the compiler itself still does not use those modules, and M1's acceptance is unchanged.
- Revisit when structs can be borrowed (`&T` for user types). At that point `&self` becomes expressible and should be added in the same change as the borrow rule, not before it.
