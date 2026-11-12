# ADR-0156: Compiler Debug Information and Stack Map Primitives (ULEB128, DWARF Line, StackMap, DebugLoc)

## Status
Accepted

## Context
Under Milestone M2 (Self-hosting compiler backend), debugging emission (DWARF .debug_line / .debug_info), GC safepoint tracking, and error diagnostics require compact zero-allocation descriptors:
1. Variable-length integer encoding (ULEB128) for DWARF / WebAssembly offsets.
2. DWARF line number matrix delta calculator (`dwarf_line_addr_delta`, `dwarf_line_num_delta`).
3. Precise stack maps (`StackMapFrame`) for safepoints and GC pointer root discovery via bitmask.
4. Source location descriptors (`DebugLoc`) with support for compiler-synthesized code tracking.

## Decision
1. **ULEB128 (`std/core/ir_leb128.ae`)**: Pure functional length calculator and byte-by-byte continuation bit extraction without heap allocation.
2. **DwarfLineEntry (`std/core/ir_dwarf_line.ae`)**: Line table state change tracker computing relative address and line deltas.
3. **StackMapFrame (`std/core/ir_stack_map.ae`)**: Bitmask-backed live stack slot query for register/frame garbage collection safepoints.
4. **DebugLoc (`std/core/ir_debug_loc.ae`)**: Compact source location span distinguishing user syntax from synthetic boilerplate.

## Consequences
- The compiler backend can generate standards-compliant DWARF line sections and stack root tables entirely within `core`.
- Verified via test suite `tests/run/batch_core_compiler_debug.ae`.
