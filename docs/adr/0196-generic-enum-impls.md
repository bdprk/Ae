# ADR-0196: Generic Enum Impl Blocks

## Status

Implemented (Windows-verified: full `cargo test --workspace --locked` suite, the `ui`/`json`/`run` snapshot suites, and `tests/run/enum_generic_impls.ae`).

## Context

ADR-0186 added inherent methods and associated functions on concrete enums, and ADR-0187 added generic `impl Pair[K, V]` blocks on generic structs. However, generic enums (`enum Opt[T]`) could not carry an `impl Opt[T]` block: `collect_generic_impl`, `instantiate_impl_method`, `Self::Variant` resolution, and postfix method resolution all assumed `struct_names` and `struct_origin`. Under §2.2 Rule 1 ("Implement Once, Use for Any Type") and M1.10 (`Option[T]` / `Result[T, E]` in standard library code), generic enums need the same method and associated-function machinery as generic structs.

## Decision

**`impl Opt[T] { ... }` shares the template machinery of ADR-0187.**

1. **Declaration collection**: `collect_generic_impl` accepts both generic structs (`generics.struct_names`) and generic enums (`generics.names`). Coherence (same file as the declaration, one generic impl per type) and method-name rules apply identically; field-collision checks run only for struct targets. Borrowed receivers (`&self`, `&mut self`) on generic enums are rejected at declaration collection with the same diagnostic as concrete enums (`enum methods take their receiver by value in this version; write 'self', not '&self'`).
2. **Origin tracking**: `instantiate_enum` records `enum_origin: eid → (gid, args)` alongside `struct_origin`, so any instantiated enum value carries the template id and concrete type arguments needed to instantiate its methods on demand.
3. **Method and associated-function resolution**:
   - Postfix `value.method(...)` on `Type::Enum(name, eid)` looks up `enum_origin` when no concrete inherent method matches, instantiating the method from the enum's `generic_impls` template with `ReceiverKind::ByValue` mapped to `Type::Enum(display, eid)` and `impl_self` set to `Type::Enum(display, eid)`.
   - Path calls `Opt[T]::assoc(...)` and `Self::assoc(...)` / `Self::Variant` inside a generic enum method body distinguish variant names from associated functions by one token of lookahead against the declaration's variant list: variant names construct the enum literal, while other identifiers instantiate and call the associated function.

## Evidence

Re-runnable:

- `cargo test --workspace --locked` — all unit, integration, `move_walk`, and snapshot tests pass.
- `tests/run/enum_generic_impls.ae` — exercises `Opt[T]::is_some(self)`, `Opt[T]::unwrap_or(self, fallback: T) -> T`, `Opt[T]::absent() -> Self` (using `Self::None`), across `Opt[i64]`, `Opt[string]`, and `Opt[f64]`.

## Consequences

- Generic enums and generic structs now have symmetric inherent `impl` capabilities (`self`, `Self`, `Type[Args]::assoc(...)`, `value.method(...)`).
- Standard-library `Option[T]` and `Result[T, E]` shapes can be written directly in AE with ergonomic methods (`is_some`, `unwrap_or`, constructors).
