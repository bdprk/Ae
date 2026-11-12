# ADR-0150: IR Opcodes, Basic Blocks, Virtual Registers, and CFG Edges

## Context
Under the Standard Library Blueprint (Domain C: Text, Lexing & M2 Self-Hosting Compiler backend), code generation and optimization passes require:
1. Intermediate Representation (IR) opcode instructions (`IrOpcode`, `OpConstInt`, `OpLoadLocal`, `OpJump`, `OpReturn`, `is_jump`, `is_terminator`).
2. Basic Block header tracking (`BasicBlockHeader`, `instruction_count`, `has_terminator`).
3. Virtual Register pool allocation (`VRegPool`, `alloc`, `allocated_count`).
4. Control Flow Graph branch edge classifications (`EdgeKind`, `EdgeFallthrough`, `EdgeBranchTrue`, `EdgeBranchFalse`, `EdgeUnconditionalJump`).

## Design
- `std/core/ir_opcode.ae`: `IrOpcode`, `ir_opcode_is_jump`, `ir_opcode_is_terminator`.
- `std/core/basic_block.ae`: `BasicBlockHeader`, `basic_block_add_instruction`.
- `std/core/vreg.ae`: `VRegPool`, `vreg_pool_alloc`, `vreg_pool_allocated_count`.
- `std/core/cfg_edge.ae`: `EdgeKind`, `edge_kind_is_conditional`.
