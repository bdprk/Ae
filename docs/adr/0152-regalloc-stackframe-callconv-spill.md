# ADR-0152: Code Generation Backend Primitives (RegAlloc, StackFrame, CallConv, Spill)

## Status
Accepted

## Context
Under Milestone M2 and the Core/Alloc/Std Architectural Blueprint (`docs/architecture/std_architecture_blueprint.md`), code generation requires machine-independent intermediate structures for linear scan register allocation, stack frame offset calculation, System V / x64 calling convention argument passing, and spill heuristic weight modeling.

## Decision
1. **RegAllocation (`std/core/ir_reg_alloc.ae`)**: Track virtual register to physical register binding, using negative index (`-1`) to signify spill to stack.
2. **StackFrameLayout (`std/core/ir_stack_frame.ae`)**: Calculate stack frame byte allocation with 16-byte alignment and compute exact spill slot offsets.
3. **CallConv (`std/core/ir_call_conv.ae`)**: Standardize argument register assignment (first 6 args in physical registers 0..5, remaining args passed on stack with 8-byte aligned offsets).
4. **SpillCost (`std/core/ir_spill.ae`)**: Compute variable spill weights factoring in usage frequency and loop nesting depth ($1 + 8 \times \text{depth}$).

## Consequences
- Backend can cleanly lower SSA IR into machine register assignments and structured stack frames without relying on heap allocation in `core`.
- Test suite verified via `tests/run/batch_core_compiler_codegen.ae`.
