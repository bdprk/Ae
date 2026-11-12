# ADR-0174: GNU Assembler Syntax and Operand Addressing Representation (GAS Syntax, Suffixes, ModR/M AddrMode, Condition Codes)

## Status
Accepted

## Context
Under Milestone M2 and M3, self-hosting native code emission targeting toolchains like GNU `as`, Apple `as`, and Clang requires exact syntactic and structural modeling of assembler dialects:
1. Assembler Dialects: configuring operand order (AT&T source-first vs Intel destination-first) and prefix rules (`%reg`, `$imm`).
2. Instruction Mnemonic Suffixes: size letter indicators (`b` for 1 byte, `w` for 2 bytes, `l` for 4 bytes, `q` for 8 bytes quadword).
3. Complex Addressing Modes: modeling effective addresses ($base + index \times scale + displacement$) including RIP-relative PC displacement.
4. Condition Code Inversion: machine flags for conditional branches (`je`/`jne`, `jl`/`jge`) and Boolean branch inversion in control-flow lowering.

## Decision
1. **GasSyntaxConfig (`std/core/ir_gas_syntax.ae`)**: Configurable dialect switcher supporting AT&T and Intel syntax.
2. **AsmOpcodeMap (`std/core/ir_asm_opcode_map.ae`)**: Maps operand width in bytes to ASCII size suffixes (`'b'`, `'w'`, `'l'`, `'q'`).
3. **AsmAddrMode (`std/core/ir_asm_addr_mode.ae`)**: Generic ModR/M and SIB addressing mode container with RIP-relative detection.
4. **AsmCondCode (`std/core/ir_asm_cond_code.ae`)**: Condition code representation with inverse branch predicate calculation ($E \leftrightarrow NE$, $L \leftrightarrow GE$).

## Consequences
- The compiler backend can seamlessly switch between AT&T and Intel assembly formatting, compute ModR/M byte offsets, and negate branch targets purely in `core`.
- Verified via test suite `tests/run/batch_core_compiler_asm2.ae`.
