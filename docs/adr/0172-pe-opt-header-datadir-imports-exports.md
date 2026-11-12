# ADR-0172: Windows PE Executable Linking Primitives (Optional Header 64, Data Directories, Imports, Exports)

## Status
Accepted

## Context
Under Milestone M2 and M3, linking standalone Windows executable binaries (`.exe`) and dynamic link libraries (`.dll`) requires:
1. PE32+ (64-bit) Optional Header: verifying magic (`0x20b`), entry point RVA, image base (`0x140000000`), subsystem (`WINDOWS_CUI`), and security DLL characteristics (`DYNAMIC_BASE` / ASLR, `NX_COMPAT` / DEP).
2. Data Directories: mapping export tables, import tables, resource tables, and exception directories.
3. Import Directory Table: Import Lookup Table (ILT), DLL name RVA, and Import Address Table (IAT) binding.
4. Export Directory Table: ordinal base calculation and export address mapping for shared library generation.

## Decision
1. **PeOptHeader64 (`std/core/ir_pe_opt_header.ae`)**: 64-bit optional header layout with security feature bitmask checks.
2. **PeDataDirectory (`std/core/ir_pe_data_dir.ae`)**: Boundary validator and end RVA offset calculator.
3. **PeImportDirectoryEntry (`std/core/ir_pe_import_dir.ae`)**: Null-terminator detection and IAT pointer extraction.
4. **PeExportDirectory (`std/core/ir_pe_export_dir.ae`)**: Ordinal to function address array index converter.

## Consequences
- The compiler binary linker can emit completely standalone Windows PE executables and DLL export tables directly from `core`.
- Verified via test suite `tests/run/batch_core_compiler_pe2.ae`.
