# ADR-0200: Three-Way Differential Fuzzing (`--debug`, `--release`, `--interp`)

- **Date**: 2026-11-13
- **Status**: Accepted
- **Milestone**: M2.10 / M2.11 (§7.2 Layer 2 Differential Fuzzing)

## Context

Previously, `scripts/diff_fuzz.py` generated deterministic random well-typed ae programs (covering integers, booleans, strings, vectors, structs, conditionals, and checked division/modulo guards) and compared two C-backend execution paths (`ae build --debug` at `-O0` vs `ae build` at `-O2`). With the addition of the in-process interpreter (`ae run --interp`, ADR-0199), the language charter (§7.2 Layer 2) specifies three-way differential testing across independent execution backends.

## Decision

1. Extended `scripts/diff_fuzz.py` to execute every generated seed under `ae run <src> --interp` alongside `--debug` (`-O0`) and `--release` (`-O2`), asserting byte-identical stdout across all three runs.
2. Added an automated integration test in `tests/execution.rs` that verifies 3-way differential equivalence across the generated corpus and representative `tests/run/*.ae` programs.

## Consequences

- Any divergence between the C codegen (`-O0` or `-O2`) and the in-process interpreter (`--interp`) on arithmetic precedence, short-circuiting, string concatenation, vector mutation, or struct field access fails immediately with a minimal seed reproduction.
