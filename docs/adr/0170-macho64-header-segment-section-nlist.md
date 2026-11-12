# ADR-0170: macOS Mach-O 64-Bit Object Emission Primitives (Header, Segment, Section, Nlist)

## Status
Accepted

## Context
Under Milestone M2 and M3, native compiler self-hosting across operating systems requires cross-platform binary format emission (Linux ELF64, Windows PE/COFF, and macOS Mach-O 64-bit):
1. Mach-O 64-bit Header (`mach_header_64`): magic bytes (`0xFEEDFACF`), CPU types (x86_64 vs ARM64 Apple Silicon), file type (`MH_OBJECT`), and load command count.
2. Segment Command (`LC_SEGMENT_64`): VM address sizing, file offsets, and section containment.
3. Section Descriptor (`section_64`): section naming, alignment exponent ($2^{align}$), and instruction attribute flags.
4. Symbol Table (`nlist_64`): string table offset, symbol type bitmasks (`N_EXT`, `N_SECT`), and section index.

## Decision
1. **MachO64Header (`std/core/ir_macho_header.ae`)**: Multi-architecture CPU type validator supporting both ARM64 and x86_64.
2. **MachOSegment64 (`std/core/ir_macho_segment.ae`)**: Load segment command descriptor tracking file offsets and contained section counts.
3. **MachOSection64 (`std/core/ir_macho_section.ae`)**: Section container verifying executable code flags and size attributes.
4. **MachONlist64 (`std/core/ir_macho_symtab.ae`)**: Symbol record validating external linkage and section definition.

## Consequences
- The compiler native emission pipeline now possesses first-class, zero-allocation binary descriptors for macOS Darwin targets.
- Verified via test suite `tests/run/batch_core_compiler_macho.ae`.
