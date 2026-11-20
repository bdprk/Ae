# ADR-0178: Unwind Table Encoding Primitives (DW_EH_PE, CFI Opcodes, CFA Rows, LSDA Tables)

## Status
Accepted

## Context
ADR-0160 and ADR-0162 model landing pads, safepoints, and exception action tables as *structures*: which regions have cleanups, which edges leave a frame. They do not model a single byte of the tables an unwinder actually reads. That leaves the self-hosting toolchain unable to emit or verify `.eh_frame`, `.eh_frame_hdr`, or the LSDA a landing pad points at — the part that matters as soon as Rust or C++ frames must unwind across ae code, which ADR-0177's plan section 8 lists as a risk ("C backend UB leaking into semantics" is one thing; an unwinder walking a table with wrong field widths is another).

Four pieces are missing and each has an independently checkable rule:
1. The pointer encoding byte (`DW_EH_PE_*`) that tells a reader how every field in these tables is stored.
2. The CFI opcode space, including which operands force the extended form.
3. The CFA row state machine that those opcodes advance.
4. The LSDA's four tables and their sizing arithmetic.

## Decision
1. Implemented `std/core/ir_eh_pe.ae`:
   - An encoding byte is decomposed into its format nibble, the `0x10` pcrel bit, the `0x80` indirect bit, and the `0xff` omitted form.
   - `eh_pe_field_bytes` returns the stored width per format, and returns 0 — never a guess — for the variable-width LEB128 formats and for unknown formats; `eh_pe_is_variable_width` distinguishes the two zero cases a caller cares about.
   - `eh_pe_value` resolves a pcrel field against its own address; `eh_pe_apply_offset` handles the datarel base separately so a caller cannot substitute the field address for the base.
2. Implemented `std/core/ir_cfi.ae`:
   - Named constants for the primary and extended opcodes, and the three primary constructors (`0x40 | delta`, `0x80 | reg`, `0xc0 | reg`) that return `-1` when the six-bit operand does not fit, rather than silently truncating.
   - `cfi_fde_len` sizes an FDE record from its encoded parts, including the ULEB128 width of the augmentation length; `cfi_fde_cie_pointer` and its validity check encode the backwards-only CIE reference; `cfi_is_terminator` covers the zero-length record that ends the section.
3. Implemented `std/core/ir_cfi_state.ae`:
   - `CfaRow` tracks the CFA register, its offset, and the remember/restore depth.
   - Register and offset move independently (`def_cfa_register` keeps the offset, `def_cfa_offset` keeps the register); `cfa_row_restore` floors at depth zero instead of failing on malformed input; `cfa_row_canonical_address` is the CFA = SP + offset definition; `cfa_row_same_state` ignores the remember stack, which is bookkeeping rather than observable frame state.
4. Implemented `std/core/ir_lsda.ae`:
   - Call-site entries are four pointer-encoded fields; the table and type-table sizes derive from the encoding through `ir_eh_pe`.
   - `lsda_sleb_bytes` gives the minimal signed LEB128 width, which sizes an action record (type filter followed by the byte distance to the next record).
   - Action zero is the cleanup-only form; `lsda_action_record_offset` converts a backwards-relative action value into a byte position inside the action table and returns 0 rather than a negative index.
5. Added `tests/run/batch_core_unwind.ae` covering: every encoding form in use (0x9b, 0x1b, absptr, udata8, sdata8, uleb128, omitted), the pcrel and indirect bits, each primary opcode at its 63/64 boundary, FDE length with one- and two-byte augmentation lengths, the CIE pointer direction rule, CFA register/offset independence, remember-depth limits, restore-underflow, and the SLEB128 width boundaries at 63/64, 8191/8192 and 134217727/134217728.

## Consequences
- These are encoding rules, not an encoder. Nothing here writes a section, walks a real `.eh_frame`, or is exercised against a C++ or Rust unwinder. The corresponding claim stays unclaimed until a test unwinds a real binary.
- Register numbers in `cfa_row_init` are x86-64 System V (CFA = RSP + 8 at function entry). A second target needs its own initial row, not a parameterised one.
- Only the primary opcode forms are constructed. `DW_CFA_expression`, `DW_CFA_val_expression`, the AArch64-specific opcodes, and the `.eh_frame_hdr` binary search table are not modelled.
- The action-record size rule assumes the Itanium ABI's two-SLEB128 record shape. A different personality routine with a different LSDA format would need its own module.
- The models are deliberately permissive about malformed input (returning a sentinel instead of failing): a compiler emitting from these helpers must still validate, and a reader must not treat a sentinel as data.
- Revisit criteria: when the first real landing pad is emitted, extend this module into a writer with a round-trip test against `readelf --debug-dump=frames`, and record the evidence in `bench/reports/`.
