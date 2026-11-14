# Unsafe, Raw Pointers, and FFI Specification (M2.12)

Status: target specification for the M2.12 milestone. The current language (2026-11-14) implements none of this; the spec lands before the implementation per the plan's Spec-First rule, and each section gains a positive/negative test pair as its slice merges.

## 1. Design Rules

### Hard Rules:
1. **Unsafe Never Weakens Safe Code**: every guarantee safe code enjoys (ownership, borrow checking, bounds, checked arithmetic, initialization) holds unchanged. `unsafe` only unlocks operations the checker cannot prove; it never disables a check on code the checker can prove.
2. **The Author Owns the Invariant**: every `unsafe fn` documents its preconditions in a doc comment; every `unsafe {}` block carries a comment naming the invariant that makes it sound. A violation is undefined behavior — the language's safety story (ADR-0205) does not extend across this boundary.
3. **No Safe Path to UB**: constructing a dangling pointer, an unaligned reference, or an out-of-bounds view must require `unsafe` (or an `unsafe fn`) somewhere in the program. If a safe spelling reaches UB, that is a compiler bug with the same severity as a borrow-checker false negative.
4. **Explicitness at Every Boundary**: raw pointers do not convert to references implicitly; `extern "C"` functions are declared, not inferred; `#[repr(C)]` is opt-in per struct. Nothing about the C boundary is ambient.
5. **Same Semantics on Both Platforms**: an FFI program's observable behavior (exit codes, stdout, file effects) is identical on Windows and Linux; the C backend's generated code already satisfies this for pure code.

### Non-Goals:
- No safe transmute of arbitrary values; no reinterpretation without `unsafe`.
- No lifetime-carrying raw pointers: a raw pointer is just an address, typed by pointee layout only.
- No C++-style inheritance or exception interop: `extern "C"` is a flat C ABI.

## 2. Raw Pointers

### 2.1 Types
- `*const T` and `*mut T` for any `T` with a known layout (scalars, `#[repr(C)]` structs, arrays).
- Copyable, comparable for equality, passable by value; they carry no ownership and are not tracked by the borrow checker. Creating one from a reference is safe; using one to read or write memory is `unsafe`.

### 2.2 Operations (all require `unsafe`)
- `p.deref()` → `T` (by value, `T: Copy`-sized) — reads the pointee.
- `p.write(value)` / `p.read()` for `*mut T`.
- `p.offset(n)`, `p.byte_offset(n)` — pointer arithmetic, no bounds reasoning.
- `ptr::null[T]()`, `p.is_null()` — `is_null` is safe (it reads nothing).
- `&v as *const T` / `&mut v as *mut T` — borrows to pointers, safe (an explicit cast).
- `p as usize` / `usize as *const T` — integer round trips, safe casts; what you do with the result is not.

### 2.3 Diagnostics
- Dereferencing outside `unsafe` is rejected: `dereferencing a raw pointer requires 'unsafe'` (E0700).
- A raw pointer escapes to a non-pointer type only through an explicit cast; no inference bridges.

## 3. `unsafe` Blocks and Functions

### 3.1 Syntax
```
unsafe { ... }                       // block: the operations inside are unchecked
unsafe fn poll(fd: i64) -> i64 { ... }   // the whole body is unchecked territory
```

### 3.2 Rules
- A body may call `unsafe fn`s and dereference raw pointers inside an `unsafe` block only.
- `unsafe fn` bodies do not implicitly behave like `unsafe` blocks — the body still requires `unsafe {}` internally (Rust 2024 style): the reviewer sees where unchecked memory access actually happens.
- Calling an `unsafe fn` from safe code requires an enclosing `unsafe` block.
- `unsafe` propagates no capability through closures: a closure literal's body is checked like any other safe body, and captures may not lend `*mut` storage the closure writes without `unsafe`.
- The borrow checker keeps checking inside `unsafe` blocks: creating a reference from a pointer inside `unsafe` re-enters the tracked world, and the usual rules apply from that point.

### 3.3 Diagnostics
- `call to unsafe function 'poll' requires 'unsafe'` (E0701) at the call site.
- An `unsafe` block may not be silent decoration: a block whose body contains no unchecked operation triggers the `unnecessary unsafe` warning (W0702).

## 4. `extern "C"`

### 4.1 Declarations (calling C)
```
extern "C" fn getenv(name: *const u8) -> *const u8;      // no body: resolved at link time
extern "C" fn abs(v: i64) -> i64;
```
- A declaration without a body is emitted as a C declaration (`extern` prototype); the generated program links against the symbol.
- Argument and return types must be C-ABI-shaped: scalars, `*const T`/`*mut T`, `#[repr(C)]` structs by value, and `&[i64; N]`-style borrowed arrays are legal; owned strings, vectors, maps, and enums are not (they carry release logic the C side cannot honor).
- Calling a declared `extern "C"` fn is `unsafe` — the C side's preconditions are unknown to the compiler.

### 4.2 Exposures (C calling AE)
```
#[no_mangle]
extern "C" fn ae_add(a: i64, b: i64) -> i64 { return a + b; }
```
- `#[no_mangle]` keeps the symbol name; without it the name is not emitted as a C-visible symbol.
- An exposure's body is safe code: the compiler wraps it, so the C side cannot observe an AE panic directly — a trap inside an exposure aborts the process with the runtime's trap diagnostics (same as today).
- Exposures with `&Struct`/`&mut Struct` parameters or `#[repr(C)]` by-value structs are the only structured crossing points.

## 5. `#[repr(C)]`

### 5.1 Layout
- `#[repr(C)] struct Pair { a: i64, b: i64 }` fixes field order and C padding rules (each field at its natural alignment, struct alignment = max field alignment).
- Without the attribute, layout is the compiler's choice (today: declaration order, natural alignment); `#[repr(C)]` is required for any struct crossing the FFI boundary in either direction.
- The layout is pinned by a compile-time query pair, `c_size_of[T]()` and `c_offset_of[T]("field")` (each field's byte offset), evaluated in the parser to constants. The `c_` prefix keeps the pair distinct from the generic-borrow convention's user-definable `size_of[T]`; the names belong to the same `#[repr(C)]`/`extern "C"` family.

### 5.2 Diagnostics
- Passing a non-`#[repr(C)]` struct across `extern "C"` is rejected: `'Pair' is not '#[repr(C)]'; the C ABI requires a fixed layout` (E0703).

## 6. Precondition Documentation

Every `unsafe fn` and every nontrivial `unsafe {}` block must carry a doc comment naming:
1. What memory the caller must guarantee valid (pointers non-null, in-bounds, aligned).
2. What aliasing the caller must guarantee absent (no live borrows overlapping the raw pointer's access).
3. What the callee assumes about thread context (single-threaded, or which synchronization the caller owes).

The compiler enforces presence (a doc comment is required on `unsafe fn`, E0704); it cannot enforce truth.

## 7. Test Matrix (each cell lands with its slice)

| # | Case | Expected |
|---|---|---|
| U1 | `let p = &v as *const i64; unsafe { print(p.deref()); }` | Runs, prints the value |
| U2 | `let p = &v as *const i64; print(p.deref());` | Rejected E0700, positioned at the deref |
| U3 | `unsafe fn f() { print(1); }  f();` (no unsafe at the call) | Rejected E0701 |
| U4 | `unsafe { f(); }` where `f` is safe | W0702 |
| U5 | `extern "C" fn abs(v: i64) -> i64;` called inside `unsafe` | Links against libc `abs`, runs |
| U6 | An `extern "C"` declaration taking a `string` | Rejected (owned types are not C-ABI-shaped) |
| U7 | `extern "C" fn` exposed with `#[no_mangle]`, called from a C stub | Bidirectional example runs on both platforms |
| U8 | A `#[repr(C)]` struct passed by value to a declaration | Matches the C layout check (`c_size_of`/`c_offset_of`); the by-value call compiles with a real C prototype |
| U9 | A struct without `#[repr(C)]` crossing the boundary | Rejected E0703 |
| U10 | `unsafe fn` without a doc comment | Rejected E0704 |
| U11 | A dangling deref under the MIR UB-detection mode (M2.11) | Trapped with a pointer diagnostic |
| U12 | Differential: every U-case's safe mirror compiles and runs identically under interp / MIR / tree / release | Four-way agreement |
