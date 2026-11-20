# Type System Specification (M1.1)

## 1. Overview and Design Rules
ae's type system provides zero-cost abstractions, static memory safety, and explicit capability boundaries without runtime overhead or hidden allocations.

### Hard Rules:
1. **No Implicit Numeric Conversions**: Numeric conversions between distinct scalar types must be explicit via constructors or casts.
2. **Explicit Integer Semantics**: Integer overflow traps by default in both debug and release configurations. Checked/wrapping operations are provided as explicit methods.
3. **No `nil`/`null`**: Absence of value is modeled exclusively through algebraic types `Option[T]` and fallibility through `Result[T, E]`.

## 2. Primitive Types

### 2.1 Scalars
- Signed integers: `i8`, `i16`, `i32`, `i64`, `i128`, `isize` (pointer width)
- Unsigned integers: `u8`, `u16`, `u32`, `u64`, `u128`, `usize` (pointer width)
- Floating point: `f32`, `f64` (IEEE 754)
- Boolean: `bool` (`true`, `false`)
- Unicode scalar: `char` (32-bit scalar value representing a valid Unicode code point)
- Unit type: `()` (zero-sized, single-value)
- Never type: `!` (indicates non-returning expressions, divergent control flow)

### 2.2 Composite Types
- Tuples: `(T1, T2, ...)`
- Fixed-size arrays: `[T; N]` (inline contiguous memory of constant size `N`)
- Slices: `&[T]` (borrowed view of contiguous elements: pointer + length)
- Strings:
  - `str`: Dynamically sized borrowed UTF-8 string slice (`&str`).
  - `String`: Owned, growable UTF-8 string buffer managed by the allocator.

### 2.3 References and Pointers
- Shared reference: `&T` (read-only, aliasing allowed)
- Mutable reference: `&mut T` (exclusive access, no aliasing)
- Raw pointers: `*const T`, `*mut T` (usable only in `unsafe` blocks)

## 3. User-Defined Types

### 3.1 Structs
Structs declare nominal product types with named fields:
```ae
struct Point {
    x: i64,
    y: i64,
}
```

### 3.2 Enums (Algebraic Data Types)
Enums declare nominal sum types with tagged payloads:
```ae
enum Option[T] {
    Some(T),
    None,
}

enum Result[T, E] {
    Ok(T),
    Err(E),
}
```

## 4. Methods and `impl` Blocks

### 4.1 Inherent Implementations
Methods are associated with nominal types via `impl`:
- `self`: Consumes the instance by value (move).
- `&self`: Borrows the instance immutably.
- `&mut self`: Borrows the instance mutably.
- Static associated functions omit `self` (called via `Type::function()`).

### 4.2 Auto-deref and Method Resolution
Method lookup inspects candidate receivers in order: `T`, `&T`, `&mut T`.

## 5. Traits and Generics

### 5.1 Trait Declarations and Bounds
Traits define capability contracts satisfied nominal or structural implementations:
```ae
trait Summarize {
    fn summarize(&self) -> String;
}
```

### 5.2 Coherence and Orphan Rules
An implementation `impl Trait for Type` is valid if either `Trait` or `Type` is defined within the current crate.

### 5.3 Marker & Built-in Traits
- `Copy`: Types with bitwise duplicate semantics.
- `Send`: Safe to transfer across task/thread boundaries.
- `Sync`: Safe to share immutable references across task/thread boundaries.
- `Drop`: Deterministic destructor hook executed on scope exit.
