# ADR-0185: Mutable Struct Borrows — `&mut Type` and `&mut self`

## Status

Implemented (Windows-verified: full `cargo test --workspace --locked` suite, the `ui`/`json`/`run` snapshot suites, and `scripts/verify_generated_c.py` including the new `examples/struct_mut_borrow.ae` case built with `-pedantic-errors -Werror=return-type -O1`; Linux coverage rides the CI matrix on the next push).

## Context

ADR-0184 landed read-only struct borrows and had to leave `&mut self` on the floor: without an exclusivity rule, two callers could write the same struct at once, and without a write-through form, `&mut` would have been a pointer with nothing to do. The plan's M1.5 row is otherwise complete — methods exist in all three receiver forms, which is what `spec/methods_and_traits.md` describes as the target.

The pieces the borrow needed already existed in ADR-0184's shape: a pointer parameter whose pointee is the caller's struct, a statement-scoped register for call borrows, and the field-store statement. What was missing was a second register with the opposite rule — a writer excludes everyone — plus the keyword to spell it.

## Decision

**`mut` is a keyword, and `&mut Type` is the exclusive mutable borrow of a user struct.** `Kind::Mut` joins the lexer (reserved everywhere, like `fn` or `var`), the formatter classifies it, and `parameter_type` accepts `&mut Struct` and `&mut Pair[string, i64]`. The new `Type::StructMut(sid)` names the same layout as `Struct`/`StructRef`; the C parameter is `ae_t{k} *` (no `const`), which is the entire backend delta.

**Exclusivity is one rule: a mutable borrow excludes every other borrow for the statement that makes the call.**

- The register `mut_borrows` sits beside `call_borrows`, is statement-scoped for the same reason (the callee has returned when the statement ends), and feeds `is_borrowed`, so moves and writes are blocked while the mutable call runs.
- Taking a mutable borrow requires: the variable to be `var`-declared (the callee writes what the caller owns), no live borrow of any kind, and no second mutable borrow. Taking a shared borrow while one is mutably held is rejected symmetrically.
- Shrinking is free, upgrading is not: a `&mut` parameter or receiver forwards as a pointer for a `&Type` target (a `T *` becomes `const T *` implicitly in C), but a `&Type` cannot feed a `&mut Type`, and a `&self` method cannot call a `&mut self` method through its own borrow.

**Writes go through the pointer with the existing field-store statement.** `Statement::FieldStore` gains a `through_pointer` flag: `c.n = c.n + 1;` inside a `&mut self` body emits `v0->n = ...`, and overwriting an owned field through the borrow releases the old pointee field first, exactly as the by-value form does — the caller's struct is released correctly because the pointer targets it. Reads are unchanged: a `StructMut` variable emits as the dereferenced struct, so field reads, method calls on the borrow, and string views of its fields compose as in ADR-0184.

**A `&mut` slot is mutable without `var`.** The receiver binding and `&mut Type` parameters set the writable flag themselves — writing is the purpose of the form. Whole-struct assignment through a borrow (`c = Counter { ... }`) is rejected rather than guessed at: it would have to release the old pointee before storing, which deserves its own statement shape. Map-field writes through a borrow are rejected too, with the workaround named (a by-value receiver).

**Call-site spelling is explicit, method receivers are not.** A `&mut Type` parameter is fed with `f(&mut value)` — the same `&` the shared form uses plus the keyword — while `value.m(...)` auto-borrows mutably whenever `m` declares `&mut self`, subject to the same exclusivity and `var` checks. The asymmetry is deliberate: the receiver form already names its mutability at the declaration, so the call site has nothing new to say.

## Evidence

Re-runnable:

- `cargo test --workspace --locked` — 200 passing, `tests/run/struct_mut_borrow.ae` added: `&mut self` stores (`self.n = self.n + 1`), a `&mut` parameter called through two frames, a `&mut self` method reaching a `&self` method through the same borrow, and direct field reads after each borrow ends.
- `python scripts/snapshots.py` — five new `ui` diagnostics:
  - `mut_borrow_immutable`: `cannot borrow 'c' mutably: it was declared without 'var'`;
  - `mut_borrow_shared`: `cannot borrow 'c' mutably while it is borrowed in this statement`;
  - `mut_borrow_shared_call`: the same rule through a receiver (`c.doubled() + c.inc()`);
  - `mut_borrow_assign_self`: `cannot assign to 'c' itself through a '&mut' borrow; write its fields instead`;
  - `mut_borrow_of_readonly`: `'inc' borrows its receiver mutably, which a read-only borrow cannot grant`.
- `python scripts/verify_generated_c.py` — 37 cases, `struct_mut_borrow.ae` compiled and output-checked.

Shape check, `cargo run -- emit-c examples/struct_mut_borrow.ae`:

```
static int64_t ae_fn_1(ae_t0 *v0, int64_t v1);   // fn inc: non-const pointer
    int64_t t8 = ae_fn_1(&v6);
        v0->n = t5 + INT64_C(1);                          // write through the pointer
```

The mutable receiver is a non-const pointer; the write is member assignment through it; the release helper targets the pointee's field. Nothing else changes.

## Consequences

- M1.5 is complete in substance: `self`, `&self`, `&mut self`, auto-borrow, and receiver forwarding all work; what remains from `spec/methods_and_traits.md` is the resolution-order polish (associated functions already refuse receiver calls, so the practical order is fixed).
- The `mut` keyword is reserved across the language; the formatter keeps it glued after `&`, matching how `&vec` already formats.
- Known gaps, stated rather than implied: no whole-struct assignment through a `&mut` borrow; no map-field writes through one; `&mut` is not available on type parameters in generic functions (`&T` stays read-only); a `&mut` borrow cannot be stored, only forwarded, like every other borrow.
- Next per the roadmap: methods on enums, generic impl blocks, and `Self` (M1.5 leftovers), then M1.8 closures and iterators.
