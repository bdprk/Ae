# ADR-0199: In-Process MIR/CFG Execution Interpreter (`--interp`)

- **Date**: 2026-11-13
- **Status**: Accepted
- **Milestone**: M2.11 (`ae test --interp` / `ae run --interp` UB & checked-semantics detector)

## Context

The AE language charter (§5.2 M2.11, §7.2 Layer 3) calls for an in-process interpreter (`ae test --interp` and `ae run --interp`) that executes lowered programs directly inside the compiler without invoking a system C compiler or linker. Its primary roles are:

1. **Zero-CC Execution & Fast Test Iteration**: Running unit tests (`#[test]`) or scripts without paying external C compilation and linking overhead.
2. **Reference Semantics & UB Detection**: Acting as an independent execution oracle that checks signed 64-bit arithmetic overflow (`+`, `-`, `*`, `-x`), division/modulo by zero (both `i64` and `f64`), array and vector index bounds, `assert` conditions, use-after-move on owned bindings, and loop-step bounds (`MAX_STEPS`).

## Decision

1. Added `crates/ae-compiler/src/frontend/interp.rs` exposing `interpret_path(&Path) -> Result<(String, u8), String>`.
2. Supported the core value domain (`Val::Int`, `Val::Float`, `Val::Bool`, `Val::Str`, `Val::Array`, `Val::VecI`, `Val::VecS`, `Val::VecF`, `Val::Struct`, `Val::Enum`, `Val::Closure`, `Val::FnPtr`, `Val::Ref`, and `Val::Moved`).
3. Supported `&self` and `&mut self` method dispatch via caller-frame slot references (`Val::Ref(var_id)`), propagating field and collection mutations back to the caller's binding.
4. Wired `--interp` into both `ae run <source.ae> [--interp]` and `ae test <source.ae> [filter] [--interp]` in `src/main.rs`.

## Consequences

- `ae run <file> --interp` and `ae test <file> --interp` produce identical stdout and `ae:line:col: <trap>` stderr diagnostics to the C backend on checked arithmetic, assertions, and collection operations.
- Zero compiler warnings across the workspace.
