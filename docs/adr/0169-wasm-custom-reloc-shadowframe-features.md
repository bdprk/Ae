# ADR-0169: WebAssembly Linking Relocations and Target Capabilities (Custom Sections, R_WASM, Shadow Frames, Features)

## Status
Accepted

## Context
Under Milestone M2 and M3, complete WebAssembly compilation and static object emission (targeting `wasm-ld` or standalone runtime execution) require:
1. Custom Section Layout: `.debug_*`, `name`, and metadata sections with length-prefixed identifier strings.
2. Relocation Records (`R_WASM_*`): function index LEB, memory address LEB, and global index relocation entries.
3. Linear Memory Shadow Stack Frame: modeling spills to the user-space stack for aggregate parameters and spilled pointers.
4. Target Feature Vector: detecting capability bitmasks (`bulk-memory`, `simd128`, `multi-value`, `mutable-globals`).

## Decision
1. **WasmCustomSection (`std/core/ir_wasm_leb_custom.ae`)**: Section size calculator prefixing the section name and payload bytes.
2. **WasmRelocEntry (`std/core/ir_wasm_reloc.ae`)**: Linker relocation record for WebAssembly object files.
3. **WasmShadowStackFrame (`std/core/ir_wasm_stack_frame.ae`)**: Stack frame layout ensuring required alignment on linear memory allocations.
4. **WasmFeatures (`std/core/ir_wasm_features.ae`)**: Target architecture capability bitmask inspector.

## Consequences
- The compiler WebAssembly backend has full support for emit-time object linking, debug section serialization, and feature gating.
- Verified via test suite `tests/run/batch_core_compiler_wasm4.ae`.
