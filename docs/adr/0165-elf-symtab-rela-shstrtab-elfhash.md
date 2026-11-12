# ADR-0165: Binary Symbol Binding and Relocation Assembly (Symtab, RELA, Shstrtab, Symbol Hashing)

## Status
Accepted

## Context
Under Milestone M2 (Self-hosting compiler backend), object file emission requires standardized ELF64 symbol table formats and symbol table link structures:
1. ELF64 Symbol Entry: encoding symbol name offset, binding scope (`LOCAL`/`GLOBAL`), type (`FUNC`/`OBJECT`), and section index into `st_info`.
2. ELF64 RELA Entries: explicit 64-bit addends with target relocation types (`R_X86_64_64`, `R_X86_64_PC32`, `R_X86_64_PLT32`, `R_X86_64_GOTPCREL`).
3. Section Header String Table (`.shstrtab`): separate null-terminated buffer for section identifier strings.
4. Fast Symbol Lookup: standard System V and GNU 33-multiplier polynomial symbol hashing.

## Decision
1. **ElfSymEntry (`std/core/ir_elf_symtab.ae`)**: Compact layout providing bitfield-packed `st_info` ($binding \times 16 + type$) calculation.
2. **ElfRelaEntry (`std/core/ir_elf_reloc_rela.ae`)**: Explicit addend relocation entry representation.
3. **ShstrtabBuilder (`std/core/ir_elf_shstrtab.ae`)**: Monotonic offset counter for section string tables.
4. **ElfHash (`std/core/ir_elf_hash.ae`)**: Implementation of GNU hash (djb2 variant, $h \times 33 + c$) and modulo bucket distribution.

## Consequences
- The linker backend can serialize `.symtab`, `.rela.text`, and `.shstrtab` sections directly to disk without memory allocation in `core`.
- Verified via test suite `tests/run/batch_core_compiler_elf2.ae`.
