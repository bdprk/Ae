# Source-Level Container Safety Plan

Status: working design and implementation gates, reviewed against the repository on 2026-11-18. This document refines ADR-0208 and ADR-0209; it does not claim those gates have passed.

## Direction

Implement generic collections in AE, preserving the charter's static safety, deterministic release and no per-element-type container families. First establish a sound, limited scalar container, then extend the same design to owned elements. Retain existing built-ins until a replacement meets the migration gates in `spec/stdlib.md`.

For owned elements, select statically resolved per-type drop glue as the target, including an explicit user-defined destructor mechanism if needed. A per-instance function pointer is not the default: it adds indirection and requires proving hook provenance and transfer. Struct constraints can express admissibility but do not themselves perform destruction. Exact syntax and dispatch remain subject to a spec and implementation review; choosing the direction does not establish that the current compiler supports it.

Introduce `u8` with its own explicit conversion and arithmetic specification before relying on byte-oriented containers. It is not a prerequisite for validating an otherwise sound scalar storage prototype; adding it alone does not solve ownership, alignment or release.

## Current evidence and gaps

1. `c_size_of` supports the current scalars (`i64`, `f64`, `bool`) and substituted scalar parameters, as well as `#[repr(C)]` structs. The scalar query uses the same x86-64 ABI assumptions as the existing struct-layout calculation. Alignment queries and a verified layout model for other targets remain open; do not assume all elements have `i64` alignment.
2. `runtime_ex.c` contains allocator and free bridges; `tests/run/extern_alloc_ok.ae` demonstrates one explicit allocation and free. It does not demonstrate cleanup on move, overwrite, scope exit, error propagation or partial initialization.
3. `frontend/interp.rs` explicitly traps on pointer arithmetic. Four-way agreement cannot currently validate an allocation-based container. Record compiled backend and sanitizer coverage separately; do not count an expected interpreter trap as a successful execution comparison.
4. MIR drop elaboration removes redundant existing releases. It does not by itself prove that a user-defined raw-buffer wrapper is owned, non-copyable, or invokes a destructor. The current `Copy` constraint is limited to generic functions (`spec/generics.md`), so the admissibility of every container instantiation needs enforcement rather than a comment.
5. `spec/unsafe.md` contains target operations and an outdated status header; its U-matrix and implementation must be read together. A passing example is not proof that arbitrary raw-pointer programs are safe.

## Invariants before a safe API

- The allocation has sufficient size and alignment for `T`; checked capacity arithmetic rejects overflow before allocation or pointer arithmetic. Zero capacity and allocation failure have defined behavior.
- Exactly `len` elements are initialized, with `0 <= len <= cap`. An index is checked before access. Growth preserves initialized values and leaves the old state valid if allocation fails under the chosen failure policy.
- Ownership of the allocation transfers on move exactly once. Safe code cannot copy the owning handle, free it twice, access it after move, manufacture a conflicting alias, or observe uninitialized storage.
- Destruction releases each initialized owned element once and frees the buffer once. Overwrite, removal, early return, error propagation and partially initialized construction obey the same invariant. Specify destruction order and trap/abort behavior before implementation.
- References or views into a container cannot outlive it or coexist with mutations that invalidate them. Raw pointer operations stay inside an audited unsafe boundary; a safe method must establish all its preconditions.
- Generated C must obey the target C rules for alignment, effective type, aliasing and pointer arithmetic. Casts and successful sample output alone are insufficient evidence of soundness.

## Implementation and acceptance order

1. Specify scalar size/alignment queries and allowed type set, allocation failure and growth. Add positive and negative tests for concrete and generic scalar queries on Windows and Linux. Define overflow and zero-capacity cases before implementation.
2. Specify a non-copyable owning handle and its cleanup/transfer rules; add adversarial tests for assignment, move, return, overwrite, scope exit and error paths. Prove automatic cleanup rather than relying on callers to invoke `free`.
3. Implement the scalar container behind safe methods only after those prerequisites hold. Test empty, boundary, growth, invalid index, overflow and repeated moves; verify leaks and generated C with sanitizers where available. Compare compiled backends and explicitly report interpreter exclusions.
4. Specify and implement per-type drop glue and owned-element initialization/partial-failure behavior, with tests for nested values, overwrite, removal, move and exactly-once release. Only then migrate owned-element built-ins.
5. Give `u8` independent literal, conversion, overflow, layout and FFI tests; implement byte containers only when those tests and the ownership gates pass.

Each slice requires spec and positive/negative tests first, dual-platform evidence, and a recorded ADR. M2.13's 24-hour differential criterion remains a separate gate: archived historical seed totals alone do not establish a completed run on the current revision or resolve its exact segment-counting policy.
