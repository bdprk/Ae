# ADR-0209: Container Prerequisites — Storage Primitives and the Element-Release Protocol

## Status

Proposed (2026-11-15); implementation status reviewed 2026-11-18. Step A has landed in part; this proposal is not evidence that a safe source-level container can yet be built. See the implementation audit below and `docs/container-safety-plan.md` for the remaining gates.

## Implementation audit (2026-11-18)

- Pointer offset, byte offset, and pointer reinterpretation are implemented, with compiled-path examples in `tests/run/ptr_offset.ae` and `tests/run/ptr_retag.ae`. The interpreter explicitly traps on pointer arithmetic; the compiled examples do not establish interpreter agreement for this surface.
- Generic substitution for `c_size_of` supports the scalar types `i64`, `f64`, and `bool` as well as `#[repr(C)]` structs. The scalar and generic-scalar cases are covered by `scalar_size_queries_in_generic_and_concrete_contexts`; the struct case is exercised by `tests/run/generic_layout_query.ae`. Alignment queries remain a separate prerequisite.
- The allocator bridge is present in `runtime_ex.c` and exercised by `tests/run/extern_alloc_ok.ae`. These facts do not establish automatic cleanup, safe ownership transfer, or a complete generic container.
- The four walls in the Context section describe the proposal's original baseline, not the current tree. The statements that a scalar `Vec[T]` is writable today and that its acceptance corpus is differential-covered are objectives, not verified current results.

## Context

ADR-0208 slice 1 landed the allocator bridge (`ae_std_alloc`/`ae_std_free` reachable from AE source through extern declarations, with pointer type annotations to spell the signatures). Writing the actual `Vec[T]` source immediately runs into four walls, each confirmed in the code today:

1. **No pointer reinterpret casts.** The borrow-as-pointer cast rejects pointee changes outright — `cannot cast &i64 to *mut f64; reinterpret casts do not exist yet` (`expression.rs`, the cast grammar only takes the address of a same-typed local). A scalar-generic container allocating through an `i64`-pointee bridge cannot obtain a `*mut f64` view of its own buffer.
2. **`c_size_of[T]()` does not evaluate over a type parameter.** The layout queries resolve the bracketed name through `struct_names` directly (`expression.rs:458`) — inside a generic body, `T` is not a struct name and the query fails at parse time, even under instantiation re-parsing (the substitution table is consulted by `named_type`, never by this query path). A generic container cannot learn its element size.
3. **No pointer offset arithmetic.** `p.offset(n)` / `p.byte_offset(n)` are specified (`spec/unsafe.md` §2.2) but not implemented — the only deref form is the base pointer's `.deref()`, so element `i` of a buffer is unreachable.
4. **No element-release protocol** (the ADR-0208 slice-2 fork): scalar elements need nothing, owned elements need a per-T release path — struct-level constraints, a per-instance hook, or Drop-trait dispatch.

## Decision

Slice 2 lands in two steps, each gated on the differential corpus the same way slice 1 was:

**Step A — storage primitives** (language surface, small and independently testable):
- `p.offset(n)` / `p.byte_offset(n)` on `*const T`/`*mut T`: `n` is an element count / byte count, computed inside `unsafe` only, returning the same pointer type at the offset — the arithmetic is the program's own responsibility (spec §2.2's "no bounds reasoning").
- Pointer reinterpret casts `p as *const U` / `p as *mut U`: spelled explicitly, rejected outside `unsafe`-adjacent positions? No — the cast itself is safe (an address reinterpretation); what it enables is the caller's business, per hard rule §2.2.3 (the C side of the boundary already accepts this model). The cast validates layout compatibility the way the language validates everything else: `c_size_of` equality is NOT required (a `*mut u8` view of an `i64` buffer is the byte-level escape hatch byte-oriented code needs) — the requirement is that both sides are non-owned pointees, keeping the U11 guard intact.
- `c_size_of[T]()` / `c_offset_of[T](field)` evaluate through the **current substitution table first** (a type parameter under instantiation resolves to its concrete struct, then the existing layout path runs; a bare unknown name still fails with today's message). This makes layout queries usable inside generic bodies without weakening the concrete-name error.

**Step B — the release protocol** (one of the three ADR-0208 candidates, adjudicated with its own adversarial matrix before any owned-element container lands):
- The scalar-generic `Vec[T]` over `i64`/`bool`/`f64` does NOT wait for step B: with steps A's primitives it is writable today — allocation by `n * c_size_of`-shaped constants (per-instantiation constants, once the query resolves), offset arithmetic, deref reads/writes. `bool`'s 1-byte storage through an `i64`-pointee buffer needs the reinterpret cast (step A) or a byte-granular `u8` scalar — the M1.4 type inventory already promises `u8..u128`; adding `u8` is the smaller hammer and lands with its own positive/negative tests.
- `f64` elements through an `i64`-pointee buffer need a bit-level reinterpret (`*mut i64` → `*mut f64` on the same address) — exactly step A's cast, with no value conversion involved.

## Consequences

- Slice 2's acceptance corpus mirrors slice 1's: a scalar `Vec[T]` written in `std/vec.ae` exercising alloc → offset → store → load → free through the bridge, differential-covered, leak-clean.
- The byte-granular `u8` scalar (step B's smaller hammer) unblocks byte-oriented containers and the `bytes(s)` migration path; it is an M1.4-family addition with its own conversions story (no implicit widening).
- Nothing here relaxes the U11 owned-pointee guard: every reinterpret view is of non-owned storage, and owned elements wait for the release protocol.
- What this ADR does NOT decide: which release-protocol candidate wins (step B), and whether `u8` lands before or with the first byte-oriented container.

## Rollback

Step A's primitives are additive language surface (one new expression form, one query-path tweak); reverting is a code revert with no data or format compatibility concerns. Step B's protocol choice is gated on its own ADR precisely so a wrong pick cannot anchor the container era.
