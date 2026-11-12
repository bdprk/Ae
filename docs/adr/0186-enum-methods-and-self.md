# ADR-0186: Enum Methods and the `Self` Type

## Status

Implemented (Windows-verified: full `cargo test --workspace --locked` suite, the `ui`/`json`/`run` snapshot suites, and `scripts/verify_generated_c.py` including the new `examples/enum_methods.ae` case built with `-pedantic-errors -Werror=return-type -O1`; Linux coverage rides the CI matrix on the next push).

## Context

ADR-0182's inherent impls worked on structs only, and its known-gaps list said so plainly: no methods on enums, no `Self`. The consequence showed up in every `std/core` module that models a result shape: an enum's behaviour lived in free functions (`color_priority(c)`, `shape_area(s)`), while its constructors lived next to the type. The mechanism was already general — a method is an ordinary function with the receiver as parameter 0, and an enum is an owned type the move checker already handles — so the only real questions were where `Self` resolves and how `::` tells a variant path from an associated function.

## Decision

**An inherent impl targets a struct or an enum declared in the same file.** The target is resolved once per block into `ImplTarget::Struct(sid) | ImplTarget::Enum(eid)`; the (type, method) name space, the coherence rule (the impl lives with the type), the method-name rules, and the call form are identical for both. Field-collision checks apply to structs only, because enums have no field syntax for `value.name` to collide with.

**Enum receivers are by value in this version.** An enum is an owned type: calling `value.area()` moves it, exactly as passing it to a function does, and the move diagnostics that already govern enums govern method calls. `&self` and `&mut self` on enums are rejected with a message that names the by-value spelling — a borrowed enum would need match-through-a-pointer and a sentinel story the enum layout does not have yet. Inside a method body the receiver is a parameter like any other, so `match self { Shape::Circle(r) => ... }` reads payloads through the existing binding rules (views, no moves), and every call consumes its receiver — one binding per call.

**`Self` stands for the impl's own type, in signatures and in expressions.** The parser state carries the impl's target through both passes (signature collection and body parsing), trait impls included, where it stands for the concrete type being implemented. Three forms work:

- `-> Self` in a method or associated-function signature — the constructor's return type;
- `Self { ... }` — a struct literal of the impl's type (an enum errors with the variant spelling named);
- `Self::name(...)` — an associated function of the impl's type, which is how a method delegates to its constructor.

Outside an impl block, `Self` is rejected in both positions with the same message.

**`::` stays shallow and deterministic, with one refinement.** The name after `::` decides: for an enum, a variant name is a variant path and anything else reaches the impl block's associated functions — no guessing, and no change for structs, where the only meaning was already the associated function. This widens ADR-0183's fixed order ("if the name is an enum, it is a variant path") by one token of lookahead, which is what makes `Color::red()` and `Color::Red` coexist.

## Evidence

Re-runnable:

- `cargo test --workspace --locked` — 200 passing; new run cases `tests/run/enum_methods.ae` (match bodies over payloads, associated constructor, `Self` delegation) and `tests/run/self_type.ae` (`Self { ... }` literals, `-> Self` signatures).
- `python scripts/snapshots.py` — two new `ui` diagnostics:
  - `enum_borrow_receiver`: `enum methods take their receiver by value in this version; write 'self', not '&self'`;
  - `self_outside_impl`: `'Self' is only available inside an impl block`.
- `python scripts/verify_generated_c.py` — 38 cases, `enum_methods.ae` compiled and output-checked.

Shape check, `cargo run -- emit-c examples/enum_methods.ae`:

```
static int64_t ae_fn_1(ae_e0 v0);          // fn area(self): enum by value
    int64_t t8 = ae_fn_1(v6);                  // receiver moves, as in a plain call
```

An enum method is a static function taking the enum by value; the call is a plain call. Nothing in the backend changed.

## Consequences

- `docs/current-language.md` documents enum methods, the `::` refinement, and `Self`; the plan's M1.5 leftover list shrinks to generic impl blocks and the resolution-order polish.
- The `std/core` modelling layer can now attach behaviour to its result-shaped enums instead of pairing each with free functions.
- Known gaps, stated rather than implied: no borrowed enum receivers (`&self` on enums); no destructuring move — match bindings are views, so a method cannot consume its payload and hand it on; generic enums and structs still reject inherent impls; `Self` has no generic form (there are no generic impls to give it one).
- Next per the roadmap: M1.8 — closures with capture inference, then the `Iterator` trait and its adapters.
