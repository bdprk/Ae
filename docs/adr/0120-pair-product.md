# ADR-0120: Generic Pair and Product Type Foundations (M1.4 / M1.10)

## Status
Accepted

## Context
Per Language Charter §2.1 and §2.4, heterogeneous product types must be expressible generically via multi-parameter structures (`Pair[A, B]`) with single-owner move/copy semantics, paving the way toward generalized structural tuples.

## Decision
1. Introduce `Pair[A, B]` in `std/core/tuple.ae`.
2. Provide template constructors `make_pair` and field accessors.
3. Validate multi-type monomorphization across distinct parameter domains.

## Verification
- Unit and run snapshot tests in `tests/run/pair_product.ae`.
- Continuous stress regression on dual platforms (Windows x86-64 and Linux AMD EPYC on InstaBox).