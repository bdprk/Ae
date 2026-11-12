# ADR-0171: Windows PE/COFF 64-Bit Object Emission Primitives (COFF Header, Section, Relocations, Symbol Table)

## Status
Accepted

## Context
Under Milestone M2 and M3, complete platform self-hosting requires cross-compilation and native object file emission for Windows environments (PE/COFF 64-bit object `.obj` format linking with MSVC `link.exe` or LLVM `lld-link`):
1. COFF File Header: machine target codes (`0x8664=AMD64`, `0xAA64=ARM64`), section counts, symbol table offsets, and `IMAGE_FILE_*` flags.
2. COFF Section Header: raw data pointers, virtual memory bounds, and characteristics flags (`IMAGE_SCN_MEM_EXECUTE`, `IMAGE_SCN_MEM_WRITE`).
3. COFF Relocation Records: PC-relative 32-bit branches (`IMAGE_REL_AMD64_REL32`) and 64-bit absolute addresses (`IMAGE_REL_AMD64_ADDR64`).
4. COFF Symbol Table: storage classes (`IMAGE_SYM_CLASS_EXTERNAL`), section numbers (1-based, 0 for undefined externals), and symbol offsets.

## Decision
1. **CoffHeader (`std/core/ir_pe_coff_header.ae`)**: Architecture validator verifying AMD64 / ARM64 machine identification.
2. **CoffSectionHeader (`std/core/ir_pe_coff_section.ae`)**: Memory access permission bitmask decoder (`is_executable`, `is_writable`).
3. **CoffRelocEntry (`std/core/ir_pe_coff_reloc.ae`)**: Standard 10-byte COFF relocation record representation.
4. **CoffSymbol (`std/core/ir_pe_coff_symtab.ae`)**: 18-byte COFF symbol record layout supporting external and undefined symbol verification.

## Consequences
- The compiler native backend now encompasses the complete triumvirate of major desktop native object formats: Linux ELF64 (ADR-0155/0165), macOS Mach-O (ADR-0170), and Windows PE/COFF (ADR-0171).
- Verified via test suite `tests/run/batch_core_compiler_coff.ae`.
