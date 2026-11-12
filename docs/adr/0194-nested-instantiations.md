# ADR-0194: Nested Instantiations in Generic Declarations

## Status

Implemented (Windows-verified: the full `cargo test --workspace --locked` suite — 207 tests — plus the 137-file run-suite/examples sweep, all clean; Linux coverage rides the CI matrix on the next push).

## Context

M1.10's container direction needs generic declarations to *compose*: a `Ledger` whose field is `Pair[i64, Pair[string, f64]]`, or a `Boxed[V]` whose field is `Pair[string, V]` — the outer declaration's own parameter appearing inside a nested instantiation. Before this ADR, a bare parameter worked as a field type (`inner: V`), but the moment it appeared inside a nested bracket (`Pair[string, V]`), `named_type` failed with "unknown type 'V'": the nested argument parser had no knowledge of the enclosing declaration's parameters, and there was no payload shape for "an instantiation to be resolved later".

## Decision

**A nested instantiation in a generic declaration stays a *shape* until instantiation.** `Payload::Nested(index)` names an entry in a side table (`nested_shapes`) holding the generic declaration's id, whether it is a struct or an enum, and the argument payloads it was spelled with — which may themselves be parameters, concrete types, or further nested shapes. `field_type` (and its recursive `payload_argument`) parse such bracket arguments by checking the enclosing declaration's parameters first; resolution happens recursively in `instantiate_struct`/`instantiate_enum` per argument list, so `Boxed[i64].inner` is the layout `Pair[string, i64]`, and `Boxed[string].inner` is `Pair[string, string]` — each with its own budget-counted registration and dedup key.

**Non-generic declarations resolve nested shapes immediately** — their arguments are all concrete by construction (no parameters to defer). This is where the change caught its own first bug: the resolution *instantiates the inner type*, which registers a layout; reserving the struct's own id before that resolution handed `Ledger`'s slot to the inner `Pair` (the struct's fields then lived in the wrong `structs` entry, and every field access on the literal failed with the inner type's name). The fix — resolve all field types, *then* reserve the id — is the same ordering discipline `instantiate_struct` itself follows for its own fields.

**Method signatures needed no change**: ADR-0187 parses generic impl methods per instantiation with the parameters substituted, so nested types in method signatures and bodies (`Pair[K, V] { ... }` constructions) already worked — this ADR extends the same deferred-shape idea to the *declaration* layer, where payloads live in a table rather than per-parse substitutions.

## Evidence

Re-runnable:

- `cargo test --workspace --locked` — 207 passing; `tests/run/nested_generics.ae` covers two-level nesting through struct fields and literals (`Ledger.entries.right.right` → f64, `Boxed[i64].inner.right` → i64, `Boxed[string].inner.right` → string — three distinct instantiations of one declaration).
- Manual sweep: all 137 run-suite and example files unchanged (the deferred-shape path activates only when a bracket argument names a parameter).

## Consequences

- Generic declarations compose like the plan's `Vec[T]`-shaped containers need: a container struct can carry another instantiation as a field, which is the shape the future standard-library containers take.
- Known gaps, stated rather than implied: nested shapes are not yet usable in *trait* method payloads (traits take `T` only in first position); no direct recursive field (`struct List[T] { next: List[T] }` — a self-referential layout needs indirection the C struct model does not have yet); the instantiation budget (256 layouts) bounds composition depth × breadth as before.
- Next: the container direction continues with allocation-capable generic containers in the language, or M2.10's differential fuzzing.
