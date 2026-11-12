# ADR-0162: Compiler Runtime Subsystems (TLS Relaxation, TLS Layout, Safepoint Table, Exception Actions)

## Status
Accepted

## Context
Under Milestone M2 (Self-hosting compiler runtime emission), final static linking and binary layout generation for concurrent threads and unwinding tables require:
1. TLS link-time relaxation: transforming General Dynamic (GD) accesses to Local Exec (LE) in executable binaries without dynamic linker queries.
2. TLS section sizing: accurately calculating `.tdata` and `.tbss` segments with strict architecture alignment padding.
3. GC Safepoint address span recording: encoding table offsets and address ranges for runtime garbage collector page sweeps.
4. Landing pad action table chains: modeling type-filter matches, catch-all guards, and handler chaining for DWARF `.gcc_except_table`.

## Decision
1. **TlsAccessPattern (`std/core/ir_tls_model.ae`)**: Decision logic determining when GD/IE TLS access sequences can be relaxed to fast LE instructions ($FS:[offset]$).
2. **TlsSectionLayout (`std/core/ir_tls_size.ae`)**: Section size and alignment calculator for TLS segments.
3. **SafepointTableState (`std/core/ir_safepoint_table.ae`)**: Running address span ($max\_addr - min\_addr$) and entry count accumulator.
4. **ActionTableEntry (`std/core/ir_landing_pad_table.ae`)**: Type filter identifier and next action table offset mapper.

## Consequences
- The compiler binary generator has complete, zero-allocation modeling for TLS optimization and exception handling tables within `core`.
- Verified via test suite `tests/run/batch_core_compiler_runtime2.ae`.
