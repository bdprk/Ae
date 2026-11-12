# ADR-0175: Linker Scripts, Unix ar Archives, PE .def Exports, and Cross-Module LTO Summaries

## Status
Accepted

## Context
Following binary object container formats (ELF64, Mach-O, PE/COFF, Wasm) and assembly generation mappings (ADR-0170 through ADR-0174), the AE self-hosting toolchain requires structured representations for linking, static archiving, platform symbol definitions, and inter-procedural whole-program optimization (LTO). Specifically:
1. **Linker Script Placement**: Declarative memory regions (`MEMORY`) and section mappings (`SECTIONS`), alignment boundaries, and virtual vs load memory addressing (`VMA` / `LMA`).
2. **Unix Archive (`ar`) Handling**: `.a` static libraries containing multiple object files, member headers (60-byte standard format), 2-byte alignment padding, and special symbol table (`/`) and long-filename (`//`) entries.
3. **PE Module Definition (`.def`) Exports**: Explicit symbol ordinals, `NONAME`, `DATA` attributes, and DLL export forwarding for Windows binaries.
4. **Cross-Module LTO Summaries**: Function summary records capturing instruction counts, visibility, estimated call graph edges, and inlining/import thresholds.

## Decision
1. Implemented `std/core/ir_linker_script.ae`:
   - `MemoryRegion` tracking origin, length, and access flags (Read, Write, Execute, Alloc).
   - `SectionPlacement` tracking region mapping, alignment calculations, and load flags.
   - Pure functions `region_end`, `region_can_allocate`, and `placement_align_vma`.
2. Implemented `std/core/ir_archive_ar.ae`:
   - `ArMemberHeader` modeling standard 60-byte Unix archive member headers.
   - Detection helpers `ar_is_symbol_table` and `ar_is_longnames_table`.
   - Alignment helper `ar_padded_size` (2-byte padding).
3. Implemented `std/core/ir_def_exports.ae`:
   - `DefExportEntry` modeling Windows `.def` export definitions.
   - Attributes for ordinal assignment, `NONAME`, `DATA`, and forwarding status.
4. Implemented `std/core/ir_lto_summary.ae`:
   - `FuncLtoSummary` capturing function size, call edge counts, visibility, and hotness.
   - `LtoCallEdge` representing inter-module direct and indirect call edges.
   - Heuristics for cross-module import decisions (`lto_should_import`).

## Consequences
- These are preliminary data structures and helpers. They do not parse archives or linker scripts, link binaries, configure exports, or perform LTO. Further implementation and validation are required.
