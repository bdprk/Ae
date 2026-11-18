# Destructors and Drop Glue Specification (ADR-0210)

> **Status**: Draft specification for user-defined destructors (`Drop`) and compiler-generated drop glue, prerequisite to self-hosted owning containers (`ADR-0208`, `ADR-0210`).

## 1. Syntax and Visibility

A nominal type (`struct` or `enum`, including generic types) may define at most one destructor in an inherent `impl` block:

```ae
impl MyVec[T] {
    fn drop(&mut self) {
        // release resources
    }
}
```

Rules:
- Signature: `fn drop(&mut self)` with no additional parameters and unit return type `()`.
- Visibility: A destructor cannot be marked `pub` and **cannot be called directly** via `x.drop()` or `Type::drop(&mut x)`. Only compiler-generated drop glue may invoke it.
- Explicit early release in safe code is performed by moving the value into a consuming function (e.g., `drop(x)`).

## 2. Execution Semantics and Order

1. **Exactly-Once Guarantee**: For every initialized value of a type with a destructor, `drop(&mut self)` runs **at most once** when the value reaches a normal destruction point (scope exit, early `return`, `?` error propagation, `break`, `continue`, or overwrite by assignment).
2. **Move Transfers Obligation**: Moving a value transfers its cleanup obligation to the destination; the moved-from source does **not** run `drop`.
3. **Field Drop Order**: `drop(&mut self)` runs **before** the compiler automatically drops the value's owned fields in declaration order. A destructor must not manually drop fields that the compiler will subsequently drop.
4. **Generic Instantiation**: Drop glue for a generic type `Container[T]` resolves `T`'s drop behavior statically at monomorphization time.

## 3. In-Place Element Drop (`ptr_drop_in_place`)

Containers managing raw allocations (`*mut T`) require an `unsafe` primitive to drop initialized elements in place without freeing the backing buffer:

- `unsafe { ptr_drop_in_place(p: *mut T) }`: Runs `T`'s destructor (if any) and drops `T`'s owned fields in place.
- Preconditions (caller-enforced): `p` must be non-null, properly aligned for `T`, and point to a valid, initialized `T` that is not aliased.
- Calling `ptr_drop_in_place` on an uninitialized or already-dropped slot is undefined behavior.

## 4. Drop-Flag Representation

Because a moved-from zeroed slot is not necessarily a valid `&mut self` receiver for user-defined destructors:
- When liveness cannot be proven statically at a control-flow join (`Mixed` state, conditional move, or partial initialization), the compiler emits a stack-allocated boolean drop flag (`u8`/`bool`).
- Drop glue checks the flag before invoking `drop(&mut self)` and clears it upon move or destruction.
- Both the C backend and the MIR interpreter must implement identical normal-exit drop semantics.

## 5. Borrowing, Reentrancy, and Abort Policy

- **Exclusive Access**: `drop(&mut self)` requires exclusive access; no outstanding borrows of the value may exist at the destruction point.
- **No Reentrancy**: A value cannot be accessed or moved again once its destructor begins executing.
- **Abort Policy**: Panic or trap inside a destructor terminates the process immediately. Stack unwinding and recovery from a destructor trap are not supported.
