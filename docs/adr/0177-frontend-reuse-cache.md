# ADR-0177: Frontend Reuse Cache Keyed on the Source Unit Set

## Status
Accepted

## Context
`ae build` and `ae run` re-ran the whole frontend — lexing, parsing, name resolution, type checking, and C generation — on every invocation, even when no source byte had changed. The existing cache (ADR-4, ADR-27, ADR-28) sits *after* code generation: it keys on the generated C text, so a no-op rebuild still paid for the entire frontend before it could reuse the compiled executable. On a 3,002-line program with a debug compiler that frontend was 263 ms against 702 ms of C compilation; on larger programs the frontend dominates.

ADR-0176 added the vocabulary for an incremental layer (query keys, dependency state, crate interface summaries, session accounting) but nothing consumed it. `describe_path` already produced a stable import-graph description and its own documentation said it "does not yet participate in a cache decision".

The backend still emits one translation unit per program, so there is no per-item object file to reuse. What can be reused today is the whole frontend result, keyed on the exact bytes it read.

## Decision
1. The compiler reports what the output depends on. `frontend::compile_path_tracked` returns the C halves plus one `SourceUnit` per file in the import graph, each carrying its stable label, its path, and the SHA-256 of the bytes that were compiled. The plain `compile_path` / `compile_path_parts` entry points keep their behaviour; `emit-c` therefore remains the authority for what a program means.
2. The driver caches that result per entry file (`src/frontend_cache.rs`), under `<cache>/frontend/<sha256(canonical entry path)>/`, holding the generated C and an entry file that records the tool identity, the query key, the hash of the stored C, and one `unit <digest> <label> <path>` line per source unit.
3. A lookup re-reads and re-hashes every recorded unit and serves the cached C only when all digests match, the stored C matches its own hash, the recorded root equals the requested one, and the tool identity agrees. Any doubt is a miss; the caller then compiles from source and republishes.
4. The query key is a SHA-256 over the sorted `(label, digest)` pairs plus the tool identity, so it depends on what was compiled and not on the order the loader visited files in. It is reported under `AE_TIMINGS=1` as `ae frontend-cache: hit|miss (key ...)`, and the frontend timing line then reads `0.000 (frontend cache hit)`.
5. Tool identity is version plus the compiler executable's size and modification time. A rebuilt compiler during development must not keep serving yesterday's C, and hashing a multi-megabyte executable on every build would spend a large part of the saved time.
6. Only `build` and `run` consult the cache. `emit-c`, `check`, and `test` always run the frontend: their output *is* the frontend's answer, and `test` compiles a staged copy whose path changes between runs.
7. `ae cache stats` reports frontend entries and bytes, and both caches share `AE_CACHE_LIMIT`: pruning is a single least-recently-used sweep over `entries/` and `frontend/`. `ae cache clear` removes both, because a "cleared" cache that still skipped compilation would be a lie.

## Measurement
Single-platform verification (Windows x86-64, debug-built compiler, one 3,002-line synthetic program, `AE_TIMINGS=1`):

| Run | frontend_and_codegen_ms | c_compile_and_link_ms |
|---|---|---|
| cold cache | 263.456 | 701.970 |
| warm cache | 0.000 (hit) | 1.965 (hit) |

Both executables were byte-identical. Full report and exact commands: `bench/reports/2026-11-12-windows-frontend-cache.md`. This is a before/after pair on one machine, not a comparison against another language, and not a release-mode measurement.

## Consequences
- Warm builds no longer run the frontend. Every no-modification build after the first now costs a handful of file reads plus a cache copy.
- Any benchmark that reports "warm build" or "no modification" times across this change is not like-for-like: before it, the frontend ran every time. `bench/README.md` records this.
- The cache has no per-item granularity: one changed byte anywhere in the import graph runs the full frontend, exactly as before. The "modify implementation" and "modify interface" cases of M5.4 are untouched, and no claim about them follows from this ADR.
- Tool identity uses size plus a one-second-resolution modification time. A rebuild producing an executable of identical length within the same second would not be distinguished; `ae cache clear` is the remedy, and CI or release builds start from an empty `AE_CACHE`.
- The entry file is published after the C file it describes, and a lookup verifies the C hash, so an interrupted publish reads as a miss rather than as a stale hit.
- Entries are per entry-file path. Moving a project invalidates every entry, which costs one rebuild and no correctness.
- Added tests: seven unit tests in `src/frontend_cache.rs` (round trip, changed unit, missing unit, damaged C, changed tool identity, another root, order-independent key) and one integration test in `tests/execution.rs` that builds a two-file program twice, edits only the imported file, damages the stored C, and checks `cache stats` and `cache clear`.
- Revisit criteria: when the backend emits per-crate or per-item objects, the unit-granularity cache here is superseded by the query engine of ADR-0176 rather than extended.
