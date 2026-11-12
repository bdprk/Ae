# ADR-0167: WebAssembly Execution Environment Specification (Locals, Imports, Memory Limits, Structured Branches)

## Status
Accepted

## Context
Under Milestone M2 and M3, complete WebAssembly binary emission requires structured descriptors for the runtime environment:
1. Local Variable RLE: grouping multiple adjacent local variables of identical types into a compressed count and type pair.
2. External Imports: defining imports from external host environments (e.g. WASI or browser `env`) with type references.
3. Linear Memory Sizing: page limits specification (64 KiB unit) with initial and maximum boundaries.
4. Structured Control Flow Branches: relative label depth mapping for `block`, `loop`, and `if` constructs.

## Decision
1. **WasmLocalRun (`std/core/ir_wasm_local.ae`)**: Run-length encoded local variable group descriptor.
2. **WasmImport (`std/core/ir_wasm_import.ae`)**: Module and field length tracker with function/memory kind discrimination.
3. **WasmMemoryLimits (`std/core/ir_wasm_memory.ae`)**: Minimum and maximum memory page limit container with exact byte size calculation.
4. **WasmBranchTarget (`std/core/ir_wasm_branch.ae`)**: Relative label index calculator distinguishing block exits from loop backedges.

## Consequences
- The compiler WebAssembly backend can emit complete function bodies, declare WASI imports, and configure linear memory without dynamic allocation overhead.
- Verified via test suite `tests/run/batch_core_compiler_wasm2.ae`.
