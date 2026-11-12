# Methods, Impl Blocks, and Traits Specification (M1.2)

> **Status (ADR-0182, ADR-0183, ADR-0184, ADR-0185)**: this document describes the
> M1 target, not the current language. Implemented today: inherent
> `impl Type { fn m(self, ...) -> R }` with the call form `value.m(...)`,
> associated functions `impl Type { fn f(...) -> R }` with the call form
> `Type::f(...)`, `&self` receivers with auto-borrow (ADR-0184), and `&mut self`
> receivers over exclusive mutable borrows with field writes through the
> pointer (ADR-0185). Not implemented from this spec: methods on enums,
> generic impl blocks, a `Self` type, trait objects, associated types, and the
> orphan rule in its general form (only "same file as the type" is enforced).

## 1. Syntax and Grammar

### 1.1 Inherent `impl` Blocks
Inherent implementations associate methods and functions directly with a nominal type (struct or enum):

```ae
impl TypeName {
    // Associated function (static method, no receiver)
    pub fn new() -> TypeName { ... }

    // Inherent methods with receiver
    pub fn by_value(self) { ... }
    pub fn by_ref(&self) -> i64 { ... }
    pub fn by_mut(&mut self, val: i64) { ... }
}
```

### 1.2 Trait Definitions
Traits declare capability interfaces with method signatures and optional default implementations:

```ae
pub trait Display {
    fn fmt(&self) -> String;
}

pub trait Clone {
    fn clone(&self) -> Self;
}
```

### 1.3 Trait Implementations
```ae
impl Display for Point {
    fn fmt(&self) -> String {
        return "Point";
    }
}
```

## 2. Receiver Resolution Rules
When resolving `expr.method(args...)`:
1. Check inherent methods on `Type` where receiver matches `Type`, `&Type`, `&mut Type`.
2. Check in-scope trait methods for `Type`.
3. Auto-borrow (`&` or `&mut`) is applied implicitly when calling methods expecting a reference.

## 3. Coherence and Orphan Rules
1. An `impl Trait for Type` block is allowed only if:
   - `Trait` is defined in the current crate/module, OR
   - `Type` is defined in the current crate/module.
2. Overlapping implementations for the same type are rejected at compile-time.
