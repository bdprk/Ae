# ADR-0180: Link-Time Symbol Resolution, Section Layout, and Relocation Application

## Status
Accepted

## Context
Two separate gaps met in the same place.

The first is a measurement. `docs/plan.md` §9 sets M5.4 at "modify implementation < 100 ms", and the frontend reuse cache (ADR-0177) removed the frontend from a no-op rebuild. It did nothing for the case M5.4 actually names — a changed function body — because a body edit misses the cache and re-runs the C toolchain. Measuring the phases on this machine separates the two halves of a build:

| Step | Program | Median of 5 |
|---|---|---|
| frontend `load` (lex, parse, resolve, check) | 1,484 lines, 120 functions | 24.8 ms |
| frontend `emit` (C generation) | same | 8.5 ms |
| `gcc -c` | same, 151 KB generated C | 285.7 ms |
| link one object | same | 117.7 ms |
| `gcc -c` | `hello.c`, 60 bytes | 96.5 ms |
| link one object | `hello.o` | 112.9 ms |
| compile + link | `hello.c` | 186.9 ms |

Windows 10.0.26300, AMD64, gcc (MinGW UCRT) 16.2.0 at `-O0`, single run each. The frontend rows are reproducible with `AE_PHASE_SPLIT=1`, which makes the compiler print `ae phase: load_ms=… emit_ms=…`; the gcc rows were timed around the compiler invocation itself. The frontend is ~6% of a cold build and already 0% of a warm one; the C toolchain is the other 94%. A build that invokes gcc at all cannot finish in under ~187 ms on this machine even for a program with one `puts` call. **M5.4's budget is therefore not reachable through the C backend, no matter how much of the frontend is cached.** Reaching it requires emitting an object file and linking it inside the compiler, which is also the direction the IR series was already heading: container formats for ELF, Mach-O, PE, and Wasm exist (ADR-0150 to 0175), as do arithmetic text mapping, linker scripts, `ar` archives, `.def` export tables, and LTO summaries.

The second gap is that the thing between those two ends did not exist. `std/core/ir_reloc.ae` held one struct with four fields, a PC32 subtraction, and nothing else; `ir_section.ae` described a section header. Nothing modelled what a link does with the symbols and sections it is handed:

- No rule for two objects defining the same name. Strong versus weak, duplicate definitions, and file-scope locals masking each other were unrepresented.
- No answer for an unresolved name. Undefined-weak-resolves-to-zero and undefined-strong-is-an-error had no form, and neither did "this reference obliges the archive to hand over a member".
- No layout arithmetic. Nothing placed one input section after another, rounded to its alignment, or turned an offset into an address.
- No application step. A resolved address was never checked against the width of the field it was about to be written into, so a truncating relocation was indistinguishable from a good one.

## Decision
1. Implemented `std/core/ir_link_sym.ae`:
   - `LinkSymbol` carries a name id, a binding (local, global, weak), the defining object and section (`-1` for undefined), the value, and the size. Undefinedness is `obj < 0` rather than a separate flag, so the two places a symbol can live are the two places a caller already reads.
   - `link_sym_merge_decision` is the folding rule, returning keep, replace, or conflict. A reference never displaces a definition; a weak definition never displaces a strong one; a strong definition overrides a weak one; two strong definitions conflict; two locals of the same name are two symbols, which is how every object file names its static helpers.
   - `link_sym_merge` applies a decision, and reads both records field by field first — this compiler moves a struct passed by value, so a function that returns one of its arguments cannot name it again after the call that consumed it.
2. Implemented `std/core/ir_link_resolve.ae`:
   - `link_resolve_pair` turns the merge decision into a resolution: a duplicate reports no winner at all rather than naming one, because a named winner would invite a caller to link against it.
   - `link_final_status` gives the state of a name once every object has been offered: a definition is fine, an undefined weak reference is fine and takes zero, an undefined strong reference is an error.
   - `link_pull_archive_member` states when an archive owes a member: only an undefined strong global asks. An undefined weak reference may stay zero, and a satisfied reference needs nothing.
   - `link_ref_is_zero` keeps "bound to address zero" apart from "defined at offset zero", which a caller folding an address into a value must not confuse.
3. Implemented `std/core/ir_section_merge.ae`:
   - `align_up` rounds by division, so no power-of-two assumption is baked in: container headers record a numeric alignment, and any positive value works. Zero and one mean no constraint; a negative offset is returned unchanged rather than rounded into the previous section.
   - `SectionCursor` records bytes placed so far; `merge_place` advances it past an item's aligned slot, `merge_slot_offset` predicts the next offset without advancing, `merge_padding` reports the cost, and `merge_address` turns an offset into an address once the output section has a base.
   - A negative size reserves its alignment slot but contributes no bytes.
   - `merge_flags_compatible` refuses to merge sections whose executable and writable bits disagree.
4. Implemented `std/core/ir_reloc_apply.ae`:
   - `reloc_fits_signed` and `reloc_fits_unsigned` are the width checks; `reloc_mask` reports the bits a field covers, with `-1` for an eight-byte field rather than a value a caller could mistake for a length.
   - `reloc_apply_pcrel` computes `S + A - P` and rejects a result outside the signed range of the field (R_X86_64_PC32, R_X86_64_PLT32). `reloc_apply_abs` checks the zero-extended range (R_X86_64_32) and `reloc_apply_abs_signed` the sign-extended one (R_X86_64_32S), which is why the same value fits one and not the other.
   - `AppliedReloc` zeroes `value` on overflow, so a caller that ignores `ok` cannot patch an address it never proved.
   - `reloc_pcrel_place` encodes the x86-64 rule that a PC-relative field is measured from its own end; `reloc_field_in_bounds` refuses a field that would run past the section.
5. Added `tests/run/batch_core_link.ae`: the whole merge matrix including both local cases, duplicate and undefined resolution, weak-zero versus defined-at-zero, archive pull-in, alignment boundaries (power-of-two and not, zero, one, negative offset), cursor placement and padding, address folding, section flag compatibility, both width checks at every boundary, PC-relative overflow in both directions, the sign-extension difference between the two absolute forms, and field placement.

## Consequences
- This is the arbitration and arithmetic of a link, not a linker. There is no object reader, no string table, no name interning or hash table, no archive scan, no section writer, and no output image. Nothing here links a real object; **M5.4 remains unmet, and the measurement above says the C backend cannot meet it.**
- Only x86-64 relocation forms are modelled, and only their width check: no GOT or PLT indirection, no TLS models, no paired REL/RELA handling, no AArch64 branch-range or ADRP page arithmetic, no COFF section-relative fixups. `ir_asm_arm64.ae` and `ir_pe_coff_reloc.ae` describe those targets' encodings without any of them being applicable here.
- `link_sym_merge_decision` assumes the caller has already interned the name. Two symbols with the same `name_id` are assumed to be the same name; nothing checks that, and a caller that skips interning will merge unrelated symbols.
- The merge table is a scalar decision, not a container. Applying it across a program's symbol table in one pass, with the order-independence a real link needs, is not implemented.
- Weak definitions are dropped whole when a strong one appears; there is no COMDAT group selection, so a weak definition with a different size than the strong one is not diagnosed.
- `align_up` divides rather than masks, so it is correct for any positive alignment but slower than a mask; a linker hot path would specialise the power-of-two case.
- Revisit criteria: when object emission lands, this becomes the resolution stage of a real link; at that point the archive scanner, the section writer, and an end-to-end "link two objects and run the result" test must exist before any claim about self-hosted linking.
