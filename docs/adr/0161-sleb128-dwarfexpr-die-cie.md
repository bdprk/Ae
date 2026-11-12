# ADR-0161: DWARF Advanced Debugging Representation (SLEB128, DWARF Expressions, DIE, CIE)

## Status
Accepted

## Context
Under Milestone M2 (Self-hosting compiler backend), debugging symbol emission into `.debug_info`, `.debug_abbrev`, and `.eh_frame` requires structured low-level primitives:
1. Signed LEB128 (SLEB128) encoding for negative stack offsets and signed constants.
2. DWARF expression opcodes (e.g. `DW_OP_fbreg` for frame-base relative variable location descriptors).
3. Debugging Information Entry (`DwarfDie`) tags and child existence indicators.
4. Call Frame Information Common Information Entry (`DwarfCie`) specifying code/data alignment and return address register bindings.

## Decision
1. **SLEB128 (`std/core/ir_leb128_signed.ae`)**: Zero-allocation signed LEB128 length calculator and arithmetic shift 7-bit byte extractor.
2. **DwarfExprOp (`std/core/ir_dwarf_expr.ae`)**: Opcode and operand pair modeling stack-based DWARF location operations.
3. **DwarfDie (`std/core/ir_dwarf_die.ae`)**: DIE structure supporting compile unit and subprogram tag categorization.
4. **DwarfCie (`std/core/ir_dwarf_cie.ae`)**: Architecture-specific stack alignment and return register definition.

## Consequences
- DWARF generation can accurately describe variable locations, frame base addresses, and call frames purely in `core`.
- Verified via test suite `tests/run/batch_core_compiler_dwarf2.ae`.
