# ADR-0184: Struct Borrows — `&Type` Parameters and `&self` Receivers

## Status

Implemented (Windows-verified: full `cargo test --workspace --locked` suite, the `ui`/`json`/`run` snapshot suites, and `scripts/verify_generated_c.py` including the new `examples/struct_borrow.ae` case built with `-pedantic-errors -Werror=return-type -O1`; Linux coverage rides the existing CI matrix on the next push).

## Context

ADR-0182 shipped inherent methods but had to reject `&self`, because a struct could not be borrowed at all: the only read-only borrows in the language were the four container shapes (`string`, `vec`, `vec[string]`, `vec[f64]`) and fixed arrays. A method therefore always moved its receiver, `c.value(); c.value();` failed the way `f(c); f(c);` fails, and `self.other()` was out of reach. The plan's M1.5 row (`self`/`&self`/`&mut self`, auto-borrow) was blocked on exactly one missing primitive: a borrow of a user struct.

The C backend already knew how to lend a value: a borrowed `vec` parameter is a `const ae_vec *`, the call site takes the address of a bare variable slot, and the borrow's lifetime is the call. A struct borrow is the same contract with a different pointer type.

## Decision

**`&Type` is a read-only borrow of a user struct.** It is accepted in parameter position (`fn area(r: &Rect) -> i64`), as a generic instantiation (`&Pair[string, i64]`), and as a receiver (`impl Rect { fn area(&self) -> i64 }`). The new `Type::StructRef(sid)` names the same layout as `Type::Struct(sid)`; the `&` is what removes it from the move economy.

**A borrow is never stored and never returned.**

- `let alias = r;` where `r: &Rect` is rejected — borrows live only as parameters and call arguments, mirroring the rule for container borrows.
- `-> &Type` is rejected at the return annotation for functions and for methods alike: a borrow of a parameter would outlive the callee's frame, a borrow of a local would outlive the local. Returning a value and re-borrowing at the call site is always sound.
- Assigning to a borrow parameter, and writing `r.field = v` through one, are rejected with diagnostics that say the borrow is read-only.

**Field reads pass through the pointer unchanged.** A borrow parameter's emitted shape is the dereferenced struct (`(*v{id})`), so `Expr::Field`, views of string fields, and method calls compose without a new expression form. Reading an owned field (`r.label`) yields the field's owned *type* but never moves: the existing move rules already reject moving a field out of a struct, and `+`/view contexts copy the bytes.

**Auto-borrow is the receiver's job, not the call-site's.** `value.m(...)` where `m` declares `&self` borrows the receiver for the call — the caller keeps ownership, and no write to `value` can race it because the borrow register blocks moves and writes while the borrow is live. The receiver must be a bare struct variable (or an existing borrow, which forwards its pointer): a temporary would dangle, and a field of a variable would need a place-form the `Argument` enum does not have yet. Both rejections name the fix. A by-value method on a borrow is rejected too: there is no value to move.

**Borrow scope has two registers.** The persistent register (`borrows`) keeps the existing lifetime rules: views, token cursors, `for`-in containers, and `match` subjects are borrowed until their scope or loop ends. The new statement-scoped register (`call_borrows`) holds borrows a call argument created; the callee has returned by the time the statement ends and nothing retains the pointer, so the next statement may write the struct again:

```text
print(plot.area());   // borrows plot for the call
plot.w = 40;          // legal again: the borrow died with the statement
```

Both registers feed one check (`is_borrowed`), so every existing "cannot modify while borrowed" site covers both kinds without duplication.

**Call-site spelling stays explicit.** A `&Type` parameter is fed with `f(&value)` — the same `&` the container borrows use — or forwarded by passing a borrow parameter along. Auto-borrow applies only to methods, which is where the plan's M1.5 asks for it.

**Two moving-check false positives fell out of the same walk, and are fixed here rather than left standing.** The move bookkeeping ran after the postfix chain but before the binary-operator loop, so it decided what an expression meant without seeing the operator. Two wrong verdicts were reachable in ordinary code (a repo example — `l.text + "/tail"` in a *return* — trips the first):

- `return l.text + "/x";` was rejected as "cannot move a field out of a struct", although `+` copies field views (the `concat.ae` rule) and `print(l.text + "/x")` already worked.
- `let b = s == t;` marked `s` moved, although comparisons consume nothing, and `print(s)` on the next line failed.

The rule is now operator-aware: with a binary operator following, an owned *variable* is marked only for `+` (its only consuming operator on owned types), and a field or element view is never a move target there; the right-hand side of a string `+` is parsed under a plus-operand flag that keeps variable consumption checked while allowing read-only shapes. `s + s` still rejects — the left mark happens before the right side resolves.

## Evidence

Re-runnable:

- `cargo test --workspace --locked` — 200 passing, including the new `concat_reads_fields_and_equality_does_not_consume` regression test.
- `python scripts/snapshots.py` — `ui`, `json`, and `run` suites. `tests/run/struct_borrow.ae` exercises `&Rect` parameters, forwarding through two frames, `&self` auto-borrow called twice on one variable, field reads including a string field under `count_lines_with`, writes after borrows end, and a by-value receiver ordered last. The `ui` suite pins five new diagnostics:
  - `method_mut_receiver`: `'&mut self' does not exist yet` (next ADR's territory, named as such);
  - `struct_borrow_write`: `cannot write through 'r': struct borrows are read-only in this version`;
  - `struct_borrow_store`: `cannot store a borrowed struct in a variable`;
  - `struct_borrow_moved`: `cannot borrow 'r' because it was moved`;
  - `struct_borrow_temporary`: `'area' borrows its receiver, so the receiver must be a bare struct variable`.
- `python scripts/verify_generated_c.py` — 36 cases, including `struct_borrow.ae` compiled with `-pedantic-errors -Werror=return-type -O1` and checked against its output.

Shape check, `cargo run -- emit-c examples/struct_borrow.ae`:

```
static int64_t ae_fn_0(const ae_t2 *v0);   // fn area(&self): pointer parameter
    ae_t2 t7 = { ... };
    int64_t t8 = ae_fn_0(&v6);                 // auto-borrow: address of the slot
```

The borrowed receiver is a `const` pointer; the call takes the address of the caller's slot; nothing else in the generated program changes.

## Consequences

- `docs/current-language.md` documents `&Type` parameters, `&self` receivers, and the two-register borrow scope; `spec/methods_and_traits.md`'s status note moves `&self` with auto-borrow from the spec's target list into the implemented list.
- The plan's M1.5 row narrows to `&mut self` and the receiver-resolution polish; methods on enums, generic impl blocks, and `Self` remain open.
- `self.other()` now works when `other` declares `&self` — the shared-logic escape hatch ADR-0182 had to route through free functions.
- Known gaps, stated rather than implied: no `&mut self`; no method call on a struct-typed field (`plot.origin.distance()` is rejected with "bind it first" — auto-borrow needs a place-form for fields); `slice_view`/`tokenize` still require owned struct variables as bases; a `&Type` parameter cannot be re-borrowed into a variable, only forwarded.
- Revisit `&mut self` next (ADR-0185): the mutable borrow needs an exclusivity register (one writer excludes all readers), a write-through form for `r.field = v`, and `var` parameters spelled without `var`.
