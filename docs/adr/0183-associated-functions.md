# ADR-0183: Associated Functions and `::` Resolution

## Status

Implemented (dual platform: `tests/run/methods.ae` runs in debug and release, and `tests/ui` pins the three new diagnostics).

## Context

ADR-0182 gave structs methods but left constructors to free functions. That is half a feature: an `impl` block exists to keep a type's operations next to its name, and the operation every type has is "build one". Without `Point::new(...)`, the block fills up with behaviour while construction drifts to `point_new(x, y)` — the exact split `impl` was meant to remove, and the reason `spec/methods_and_traits.md` lists associated functions in its first example.

`::` is already taken. The parser reads `Ident :: Ident` as an enum variant (`IntResult::Ok`, `Option[i64]::Some`), and a name that is not an enum produces `'X' is not an enum; variants are written Enum::Variant`. So the question is not whether `::` can carry a second meaning, but how the compiler decides between them without guessing.

## Decision

**A method declared without a receiver is an associated function**, called as `Type::name(args)`.

The declaration already answers the question that the call site cannot: whether parameter 0 was spelled `self`. So the method map stores exactly that bit — `MethodDef { signature, receiver }` — and both call forms check it:

- `value.name(...)` requires `receiver == true`; otherwise the diagnostic says `'Point::new' is an associated function, so it takes no receiver; call it as 'Point::new(...)'`.
- `Type::name(...)` requires `receiver == false`; otherwise it says `'Point::get' takes a receiver; call it as 'value.get(...)' instead`.

Neither direction falls back silently, and neither error can be fixed by "trying the other one": the message names the form that works.

**`::` resolution order is fixed and shallow.** If the name is an enum, it is a variant path — unchanged, including generic enums. If the name is a struct, it is an associated function on that struct's impl block. If it is neither, the error says both meanings rather than only the enum one. A name cannot be both an enum and a struct, so the order never has to break a tie.

**An associated function is still an ordinary function.** It has no receiver, so its parameters are exactly as written and the call passes them through — no prepended argument, no new backend path. The only thing the impl block contributes is the `(type, name)` namespace.

**Visibility** follows the type: the struct's `pub` is checked before the method is resolved, so an associated function on a private type is unreachable from another file for the same reason its literal is.

## Evidence

- `cargo test --workspace --locked` — 255 passing.
- `python scripts/snapshots.py` — `ui` (12 cases, three of them new), `json`, `run` (82 cases; `methods.ae` now builds its values through `Counter::starting` and `Counter::zero`).
- `python scripts/verify_generated_c.py` — `examples/methods.ae` builds `Point::new(3, 4)` under `-pedantic-errors -Werror=return-type -O1`.
- Shape: `cargo run -- emit-c examples/methods.ae` shows the constructor as `static ae_t0 ae_fn_1(int64_t v0, int64_t v1)`, called like any function.

## Consequences

- `docs/claims.md` item 26 no longer lists associated functions as missing; it now records that `::` carries two meanings decided by the declaration, and that both forms remain struct-only and same-file.
- `docs/current-language.md` describes the pair as one mechanism with two call forms.
- Still absent: methods and associated functions on enums, generic impl blocks, a `Self` type, and `self.other()`. The last one stays absent for a semantic reason, not a parsing one: `self` is by value, so calling another method on it moves it.
- `::` is now a namespace operator in two senses. If a future feature wants `Type::CONST`, it extends this resolution rather than reusing the token elsewhere.
