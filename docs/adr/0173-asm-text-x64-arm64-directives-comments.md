# ADR-0173: Architecture-Specific Assembly Text Emission (x86_64, ARM64, Directives, Annotations)

## Status
Accepted

## Context
Under Milestone M2 and M3, compiler verification and integration with system assemblers (`as`, GNU `gas`, LLVM `clang -c`) require human-readable and standard-compliant assembly text emission:
1. x86_64 Assembly Syntax: register mapping (Intel syntax), SysV parameter convention (`RDI`, `RSI`, `RDX`, `RCX`, `R8`, `R9`), and callee-saved preservation (`RBX`, `RBP`, `R12-R15`).
2. ARM64 (AArch64) Syntax: Link Register (`X30/LR`), Frame Pointer (`X29/FP`), 8-parameter register passing (`X0-X7`), and callee-saved sets (`X19-X28`).
3. Assembler Directives: section selectors (`.text`, `.data`, `.rodata`), alignment padding (`.align`), and visibility (`.globl`).
4. Diagnostic Annotation: structured indentation levels and comment emitters for verbose assembly output (`-S -fverbose-asm`).

## Decision
1. **Intel x86_64 Register Map (`std/core/ir_asm_intel.ae`)**: Calling convention classification for x86_64 registers.
2. **ARM64 Register Map (`std/core/ir_asm_arm64.ae`)**: AArch64 link, frame, and callee-saved register checking.
3. **AsmDirective (`std/core/ir_asm_directive.ae`)**: Low-level assembler directive layout and alignment operand tracking.
4. **AsmCommentState (`std/core/ir_asm_comment.ae`)**: Configurable comment formatting and indentation spaces manager ($indent \times 4$).

## Consequences
- The compiler can generate formatted assembly listings (`.s`) across both x86_64 and ARM64 with full directive fidelity directly from `core`.
- Verified via test suite `tests/run/batch_core_compiler_asm.ae`.
