# ADR-0111: Hash Trait and SwissTable Key Constraints (M1.7 / M1.10)

## Status
Accepted

## Context
Per Language Charter §2.1, §2.4 and Roadmap M1.10, user-extensible collections like `Map[K, V]` and `Set[T]` require compile-time trait bounds `K: Hash + PartialEq` rather than specialized monomorphic integer/string map built-ins.

## Decision
1. Introduce the `Hash` trait in `std/core/hash.ae`.
2. Standardize hash mixing and scalar hashing behavior.
3. User structs implementing `Hash` and `PartialEq` can serve as keys in general collections.

## Verification
- Unit and run snapshot tests in `tests/run/hash_trait.ae`.
- Continuous stress regression on dual platforms (Windows x86-64 and Linux AMD EPYC on InstaBox).