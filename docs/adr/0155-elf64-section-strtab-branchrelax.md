# ADR-0155: ELF64 Object File and Branch Relaxation Primitives

## Status
Accepted

## Context
Under Milestone M2, self-hosting native binary emission requires binary container representations and target instruction sizing:
1. Standard ELF64 header fields (magic, architecture x86_64, entry point, section header offset).
2. Section header descriptor flags (`ALLOC`, `WRITE`, `EXECINSTR`) and alignment specifications.
3. String table (`.strtab` / `.shstrtab`) index offset calculation.
4. Jump branch displacement relaxation heuristics (distinguishing 8-bit short jumps from 32-bit near jumps).

## Decision
1. **Elf64Header (`std/core/ir_elf_header.ae`)**: Zero-allocation container validating ELF magic bytes, x86_64 architecture identification, and header layout.
2. **SectionDescriptor (`std/core/ir_section.ae`)**: Section descriptor defining permissions and size attributes.
3. **StrtabBuilder (`std/core/ir_strtab.ae`)**: Null-terminated string table indexing calculator.
4. **BranchDecision (`std/core/ir_branch_relax.ae`)**: Computes branch displacements and determines whether short jumps (-128 to 127) or near jumps are required.

## Consequences
- The compiler backend can now construct valid ELF64 section structures and perform final jump offset relaxation cleanly in `core`.
- Verified via `tests/run/batch_core_compiler_elf.ae`.
