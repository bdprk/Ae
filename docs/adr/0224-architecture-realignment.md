# ADR-0224: Architecture Realignment — Intrinsic Freeze, Pipeline Split, Honest Status

Status: Accepted (2026-11-20)

## Context

An architecture audit at `994463d` compared the code against the charter (Hard Rules 1–10, §3 pipeline, §3.2 layering). Tests are green (451 passing), but the recent slices moved away from the intent in five ways:

1. **Builtin sprawl (Hard Rule 2).** M3.2/M4.2–M4.4 each landed as compiler intrinsics: a new `Type` variant, a new `Expr` variant, parser dispatch, a C runtime unit, and an interpreter copy. That adds up to 41 runtime-object builtins. `adapter_*` and `stream_*` are an in-memory fake I/O model, i.e. test scaffolding, and they are now part of the language surface.
2. **Collapsed pipeline (§3.1).** The active frontend `Parser` resolves names, checks types and builds typed `Expr` in a single pass. HIR and THIR do not exist as separate stages, and two parsers coexist (M0.2.08 is still open). Without separate stages there is nothing for a query engine (M5.3) or a real LSP (M6.2) to hook into. The LSP that exists finds hover, definition, rename and completion results by matching text.
3. **Layering (§3.2).** `std/core` is a flat directory of more than 150 files. It mixes no-alloc code with code that allocates (strings, maps, json) and with hosted concepts (socket, env). Many files are standalone data records for future self-hosting rather than a usable library.
4. **Status inflation (Hard Rule 10).** ADR-0222 claimed M4 was "100% complete" and the master checklist marked M4.4/M6.2 done. The ledger was later corrected. A commit titled "M5.1" only touched the C backend, and no Cranelift exists.
5. **Evidence hygiene.** `benchmarks_real/` contains compiled `.exe`/`.pdb` files and no report. No performance claims may be made from it.

## Decision

- **R1 Freeze.** `docs/architecture/intrinsic-allowlist.txt` plus `tests/intrinsic_freeze.rs`. The runtime-object builtin set may only shrink. Every entry is tagged `keep` (an irreducible primitive: mutex, channel, scope), `migrate` (move behind a std module over a primitive), or `demote` (fake/scaffolding that leaves the compiler).
- **R2 Demote fakes.** Move `adapter_*`, `stream_*` and `clock_*` (fake clock) into AE source under `std/test/` (pure structs + Vec/String), then delete their Type/Expr/runtime/interp code. M4.4 stays open until there are real epoll/IOCP sockets behind `std/net`.
- **R3 Pipeline split.** Order: (a) M0.2.08, so the CLI uses `ae-syntax` AST only; (b) a resolver pass producing DefIds over that AST (an HIR-lite); (c) a type checker over the resolved tree producing the existing typed `Expr`, so MIR and codegen keep working. Each step must be a slice with a differential corpus. LSP features are re-based on (b)/(c) and stay unchecked until then.
- **R4 Layering.** Split into `std/core` (no alloc, no OS), `std/alloc` and `std/std`. Self-hosting IR records move to `std/compiler/`. Add a layering test that rejects core files importing alloc or hosted modules.
- **R5 Status discipline.** Item IDs in a commit title must match the ledger items flipped in that commit. The master checklist is generated from the ledger, not edited by hand.

## Consequences

- Breadth work on M4.4, M5.1 and M6.2 pauses until R2 and R3a land.
- Short-term cost: R3 touches the frontend (about 10k lines). The benefit is that M5.3, M6.2 and M7.2 become possible as designed.
