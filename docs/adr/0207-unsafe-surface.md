# ADR-0207: The unsafe Surface, Raw Pointers, and the C Boundary (M2.12)

- **Date**: 2026-11-15
- **Status**: Implemented (Single platform; the target specification lives in `spec/unsafe.md`, whose U1–U12 matrix this ADR closes out)
- **Milestone**: M2.12

## Context

The plan's M2.12 asks for `unsafe`, raw pointers, `extern "C"`, and `#[repr(C)]` with dual-platform FFI use cases. The language's safety story (ADR-0205) holds for safe code; the boundary needs an explicit, opt-in surface where the author — not the compiler — owns the invariants. The design (spec-first, 1c605c2) fixed five hard rules before any implementation: unsafe does not weaken safe code; the author owns the invariant; safe code cannot reach UB; every boundary is explicit; dual-platform semantics agree.

The slices landed in order, each gated by the spec's U-matrix cells: the `unsafe {}` scoping container (slice 1), `*const T` reads (2), `*mut T` writes (3), `extern "C"` declarations (4), `unsafe fn` declarations (5), `#[repr(C)]` structs with the layout query pair (6/8), the precondition doc-comment enforcement (7/10), the `#[no_mangle]` exposure (8/9), the W0702 decoration warning (4), and the differential surfaces for pointers in the fuzzer.

## Options

| Option | Pros | Cons | Resource Cost |
|---|---|---|---|
| Skip unsafe entirely | Zero risk | The FFI milestone (M5.4's C interop) has no foundation | — |
| Rust-shaped full borrow tracking over raw pointers | Sound from day one | Months of borrowck work the plan defers to M2.5+; blocks the milestone | Compile: large |
| Scoped unsafe surface: local-frame pointers, scalars first, owned pointees rejected until borrows are tracked | Milestone-sized; every unsound shape rejected at the boundary; the interp arms UB traps for later slices | Pointer-to-owned is deferred (spec records it) | Compile: small per slice |

## Decision

1. **The unsafe block is a scoping container.** `Statement::Unsafe` flattens into the current CFG block; `unsafe_depth` gates the unchecked operations. `returns()` treats the block transparently (a `return` inside returns from the function).
2. **Pointers are local-frame and scalars-first.** `Type::Ptr`/`PtrMut` intern their pointee; the take-address operation accepts bare scalar variables only — an owned pointee is rejected because the borrow is untracked and safe code could dangle it by moving (spec hard rule 3). `p.deref()` and `*p = value` are legal only inside `unsafe`, with E0700/E0701-shaped diagnostics. The interpreter's deref traps on a moved slot — the U11 backstop, armed for pointer-to-owned.
3. **extern "C" declarations are prototypes, exposures are definitions.** A declaration (no body) emits a real C prototype — slice 4 left the prototype emission unwired and every foreign call compiled as an implicit declaration; the fix numbers prototype parameters by position and emits them after the struct typedefs. An exposure (`#[no_mangle]` + body) emits under its own name with external linkage; its signature is extern_c=false because the body is safe AE code, so calls from AE are safe. The C ABI shape check covers scalars, raw pointers, and `#[repr(C)]` structs by value — recursively through fields.
4. **`#[repr(C)]` is a promise with queries.** The attribute rejects generic structs (no single layout) and owned fields (release logic the C side cannot honor); `c_size_of[T]()`/`c_offset_of[T]("field")` evaluate the C layout math in the parser to constants, pinning regressions in tests. The `c_` prefix keeps the pair distinct from the documented user-definable `size_of[T]` generic — the first draft took the bare name and the generic-borrow test caught the collision.
5. **Precondition docs are enforced for presence, not truth.** E0704 requires a `///` line in the comment block above an `unsafe fn` (the syntax crate scans the source backwards; comments are lexer-skipped so the token stream carries nothing).
6. **W0702 is advisory.** An `unsafe` block whose body contains no structural unchecked operation (raw deref, pointer store) warns on stderr without failing the compile. The callee-aware refinement (extern/unsafe calls as unchecked operations) is deferred with the reason recorded: the statement parser runs before the signature pass completes.

## Evidence

- `cargo test --workspace --locked` — 296 tests, 0 failures, including the U-matrix corpus: `unsafe_block.ae`, `unsafe_pointer_deref.ae`, `pointer_write.ae`, `extern_c_call.ae`, `unsafe_fn_call.ae` (run) and eleven ui rejections (E0700/E0701/E0703 shapes, the attribute guards).
- `repr_c_layout_queries_and_extern_prototypes` pins the layout constants (16/0/8) and the emitted prototype; `no_mangle_exposure_runs_bidirectionally_and_stays_c_visible` runs the C→AE trampoline (42) and the direct call (3).
- `python scripts/diff_fuzz.py` — the fuzzer generates the pointer surface (take-address, deref reads, unsafe writes) with a 3× deref bias; 40-seed runs and multiple long sweeps (1500-seed pointer surface, 1200-seed final validation) at 0 failures.
- `python scripts/verify_generated_c.py` — 41 PASS / 0 FAIL.
- Single-platform verification (Windows); the FFI corpus links the runtime probe on both platforms via CI.

## Consequences

- The C boundary exists: declarations link against foreign symbols, exposures accept foreign calls, and `#[repr(C)]` structs cross by value with layouts pinned by tests.
    - Pointer-to-owned remains future work and is *prevented*, not deferred silently: the rejection is the soundness line of defense, the interp trap is armed, and the spec matrix says so.
- Remaining M2 items: M2.13's 24-hour differential acceptance — the harness (`--duration`, parallel workers, corpus save) is in place; the acceptance runs as accumulated segments.
- Revisit criteria: a demonstrated unsoundness in the unsafe surface (a safe-code path to UB) is a spec-level defect and reopens the boundary design, not just the slice.
