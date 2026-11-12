# ADR-0164: Low-Level Machine ABI Interoperability (Params, SRet, Varargs, Alloca)

## Status
Accepted

## Context
Under Milestone M2 (Self-hosting native backend code emission), full compliance with standard System V and Windows x64 ABI standards requires exact primitives:
1. ABI parameter placement: distinguishing register-allocated slots (up to 6 general-purpose registers) from stack spill slots.
2. Structure return (sret): enforcing the ABI threshold (structures > 16 bytes returned via hidden pointer in `rdi`).
3. Variadic argument save areas (`va_list`): modeling general-purpose (GP) register save areas and overflow boundaries.
4. Dynamic stack allocation (`alloca`): aligning dynamically sized stack reservations and computing stack pointer adjustments ($16$-byte aligned).

## Decision
1. **AbiParamSlot (`std/core/ir_abi_param.ae`)**: Parameter placement descriptor recording register index or stack offset.
2. **AbiReturnSlot (`std/core/ir_abi_ret.ae`)**: Return layout descriptor enforcing 16-byte `sret` threshold or double register return (`rax`/`rdx`).
3. **AbiVarargsArea (`std/core/ir_abi_varargs.ae`)**: Register save area offset and GP register capacity validator.
4. **AbiAlloca (`std/core/ir_abi_alloca.ae`)**: Alignment padding calculator and stack pointer downward delta calculator.

## Consequences
- The compiler native backend can construct foreign function interfaces (FFI) and standard C calling convention frames entirely within `core`.
- Verified via test suite `tests/run/batch_core_compiler_abi2.ae`.
