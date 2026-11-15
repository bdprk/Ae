# ADR-0208: Self-Hosted Containers via Raw Byte Buffers and the Existing Unsafe Surface

## Status

Proposed (2026-11-15). The M1.10 gate ("implement `Vec[T]`, `String`, `Map[K, V]`, `Set[T]`, `Option`, `Result` in the language itself"; plan §5.3 freezes further hand-written container families) needs a container substrate that the language, not the compiler, owns. This ADR decides that substrate before the first slice lands.

## Context

Today every container is a compiler built-in: `vec`/`vec[string]`/`vec[f64]` with `ae_vec_*` C runtime families, maps with `map_new`/`map_get`, plus hand-written result enums. Hard rule §2.2.1 ("implement once, use for any type") and the M1.10/M1.11 acceptance rows require generic containers written **in AE source** (`spec/stdlib.md` §3 migration inventory), with the runtime reduced to type-agnostic primitives.

The language gained exactly the pieces a self-hosted substrate needs in M2.12 (ADR-0207): raw pointers `*const T`/`*mut T`, `unsafe` blocks and functions with E0700/E0701 enforcement, `extern "C"` declarations against the linked runtime, and `#[repr(C)]` with the `c_size_of`/`c_offset_of` layout queries. The natural candidate is to build containers on that surface with **zero compiler changes**.

The blocker is the declaration-time guard from ADR-0207's U11 row: **a pointer to an owned value is rejected** because the borrow checker does not track pointer borrows, so `struct Vec[T] { data: *mut T }` with `T = string` is rejected at the declaration — the exact shape a container needs. Two ways past it:

- **(b) Tracked pointer-to-owned**: teach the checker to treat `*mut T`-holding structs as owning heaps. This is the Rust model. It needs borrow-checker surgery (place graphs over pointer indirections, drop elaboration through the indirection), UB-detection integration in the interpreter, and a long correctness tail. It is also strictly more power than M1.10 needs.
- **(a) Raw byte buffers**: the container stores `data: *mut u8` (or a `#[repr(C)]` byte-blob handle) — u8 is not owned, the U11 guard is untouched. Element access computes `(data as usize) + i * c_size_of[T]()` inside `unsafe`, reading/writing scalars through the pointer ops, and owned elements are constructed/moved through `extern` calls into runtime helpers that already exist (`ae_string_*`, release routines).

## Decision

**M1.10 containers are written in AE source over raw byte buffers (option a). The compiler's container built-ins become deprecated aliases (spec/stdlib.md §4 policy) once the generic types pass their slices; tracked pointer-to-owned (option b) is explicitly deferred** — it needs its own ADR, adversarial matrix, and the M2.11 UB-mode backstop, and none of it blocks the migration.

Concretely, per container:

1. **Storage**: `struct Vec[T] { data: *mut u8, len: i64, cap: i64 }`. Growth uses `extern "C" fn ae_alloc(n: i64) -> *mut u8` / `ae_alloc_resize` / `ae_free` declarations; the compiler needs no new built-ins. One runtime addition is required, and it is the slice's first step: today the allocator family (`ae_alloc`/`ae_alloc_try`/`ae_alloc_resize`/`ae_free`) is `static` in `runtime.c` with `size_t` signatures an AE declaration's emitted prototype (int64_t everywhere) cannot match. The bridge is two `ae_std_*` wrappers appended to `runtime_ex.c` with int64_t signatures, **static, following the U7 exposure-trampoline precedent** — same translation unit, retained by the pruner whenever a declaration references the symbol; no de-static'ing of the originals, and the built-in container families (`ae_vec_*` etc.) die with the migration. The tree-shaking root set already keeps any `ae_*` symbol an AE declaration references (ADR-38), and the extern-prototype machinery (M2.12 slice 4) emits the C prototypes, so no further plumbing is involved. The `T = u8`-only pointee keeps the U11 guard intact for everything else.
2. **Element access**: scalar `T` (`i64`, `f64`, `bool`) reads/writes via `unsafe` pointer ops at `data + i * c_size_of[T]()`. Owned `T` (`string`, nested containers) travel through runtime helper declarations (`ae_vec_store(bytes, i, ptr, release_id)`-shaped, one per runtime element family during the migration), which reuse the existing release-routine registration instead of reinventing drop logic in source. The helper signatures are the migration's real design surface and land per slice with their own tests.
3. **Bounds**: `len` is checked in safe code before any `unsafe` body runs — the language's bounds contract (§2.2.3) is honored by the container's own logic, not by trusting raw memory.
4. **Drop**: `impl Vec[T]` gets the deterministic release through the existing drop path (ADR-0204 elaboration works on the MIR for AE-defined types once the struct is owned); the buffer's release calls `ae_free`, owned elements release through the runtime helpers.
5. **The interpreter (M2.11)**: byte-buffer arithmetic inside `unsafe` is opaque to the Moved-slot traps; the per-container bounds checks in safe code are what the UB-detection mode verifies. Known-honest limitation, stated in `spec/stdlib.md` §1 until option (b).

## Consequences

- The freeze list holds: no new hand-written families after the migration starts; `vec[f64]`/`strvec_new`/`f64map_new` et al. leave the reserved registry per spec/stdlib.md §4.
- The compiler shrinks: container type-checking, C emission, and runtime selection for containers move out of built-ins into source the compiler treats like any user code.
- `extern "C" fn ae_alloc` declarations in stdlib source are the first *stdlib-owned* FFI declarations — they also exercise the exposure/prototype machinery (U7/U8) on real stdlib code instead of tests.
- What this ADR does **not** do: it does not relax the U11 guard, does not add allocation built-ins to the language surface, and does not make `unsafe` part of any user-visible container API — every `unsafe` lives inside the stdlib's method bodies, behind safe methods, exactly the boundary ADR-0207 §3.2 describes.
- Evidence per slice: the differential corpus grows the container shapes it already generates (borrow surface for views into `Vec[T]`), and the leak harness (`--leak`) asserts the byte-buffer release paths balance for the migrated examples.

## Rollback

The built-in families stay emitted and linked until their alias is removed (one-version grace), so any slice can revert by re-pointing the alias to the built-in — no data-format compatibility exists between the built-in container and the source one mid-migration, which is why the migration is per-domain (spec/stdlib.md §4.1) and each slice migrates its examples atomically.

## Slice-2 Open Decision: the Element-Release Protocol (written before implementation)

A `Vec[T]` over **scalars** (`i64`/`bool`/`f64`) needs no release hook — memcpy semantics, free the buffer, done. That shape can land as the first true source container, and per-instantiation checking keeps it sound.

A `Vec[T]` over **owned elements** (`string`, nested containers) cannot: the generic body must call T's release routine, and the language has no protocol for that yet. Three candidate mechanisms, to be adjudicated in a dedicated ADR before the owned-element slice:

1. **Struct-level constraints** (`struct Vec[T: Copy]`): constraints exist today only on functions (ADR-40's allowlist model); extending them to structs is a language change with its own coherence questions — and the current constraint model (built-in allowlists) cannot express "has a release routine" anyway.
2. **Per-instance release hook**: each container instance carries a release-function pointer (the `ae_allocator` vtable-of-one pattern already in runtime.c) — no language change, but every element release goes through an indirect call and the hook's provenance needs compile-time enforcement.
3. **Drop-trait dispatch** (the Rust model, plan M1.7's `Drop`): the honest end state, and the largest surface — drop elaboration (ADR-0204) would dispatch through a trait impl instead of per-type emitted routines.

Until that decision, the migration inventory's owned-element rows (`vec[string]` → the eventual `Vec[string]`-shaped container) stay on the built-in implementations; the freeze list already prevents new hand-written families from accreting in the meantime.
