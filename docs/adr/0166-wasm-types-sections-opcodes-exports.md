# ADR-0166: Multi-Target WebAssembly Backend Emission (Types, Sections, Opcodes, Exports)

## Status
Accepted

## Context
Under Milestone M2 and Milestone M3 of the ae roadmap, enabling a multi-target self-hosting compiler backend requires standard WebAssembly (Wasm) binary encoding descriptors alongside native x86_64:
1. Wasm Type Section: function signature encoding with parameter counts, result counts, and `valtype` primitives (`0x7E=i64`, `0x7F=i32`).
2. Wasm Section Containers: standard section IDs (`0=Custom`, `1=Type`, `3=Function`, `7=Export`, `10=Code`) with length prefixing.
3. Wasm Stack Opcodes: modeling operand stack push/pop effects to verify stack balance without dynamic allocations.
4. Wasm Export Entries: mapping exported functions, linear memory instances, and tables to external symbol names.

## Decision
1. **WasmFuncType (`std/core/ir_wasm_type.ae`)**: Signature descriptor tracking parameter counts, return types, and void return status.
2. **WasmSection (`std/core/ir_wasm_section.ae`)**: Section identifier and payload size encapsulation.
3. **WasmOp (`std/core/ir_wasm_opcode.ae`)**: Instruction opcode modeling net stack growth and block termination properties.
4. **WasmExport (`std/core/ir_wasm_export.ae`)**: External export descriptor linking module items to string indices.

## Consequences
- The compiler backend gains multi-target emission capability, paving the way for browser execution and embedded sandbox deployments directly from `core`.
- Verified via test suite `tests/run/batch_core_compiler_wasm.ae`.
