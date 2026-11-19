# Module Loading, Visibility, and Interface Summaries (current implementation)

## 1. Loading

- `use "relative/path.ae";` appears only at the top of a file; only comments may precede it.
- Paths are resolved relative to **the directory containing the importing file**; a module's own `use` statements resolve relative to its own location. Loading is depth-first in source order, and import depth must not rely on recursive host-language calls.
- Files are identified by canonicalized filesystem paths: a fully loaded file is loaded once, so duplicate imports and diamond dependencies are permitted; importing a file currently on the active loading chain is a cycle error. The diagnostic lists the modules from the first occurrence of the repeated file through the edge that returns to it, using the same label for both occurrences.
- Loading errors prevent compilation; a failed module is never treated as successfully loaded. Loader errors without a verified source span report `null` location fields in JSON diagnostics (see `spec/diagnostics.md`).
- `main` must be unique across the entire program; library files do not require `main`.

## 2. Visibility

- Top-level items are **private by default**: only `fn`, `struct`, or `enum` declarations marked `pub` are usable from other files.
- A file always sees its own private items. Preset result enums are public to all files.
- Name collisions across files are consistently errors — file-isolated private namespaces are not implemented, so private items also cannot collide across files.
- Using an unexported name reports `function 'x' is private to <file> (mark it 'pub' to export it)` (same shape for structs and enums).
- If a `pub` function's parameter or return type is a private type local to its file, the error is reported **at the definition site** (`public function 'x' exposes private struct 'Y'`), not at each call site.
- Generic instantiation treats the declaration file as the "current file": a `pub fn` generic body may call the declaration file's private functions and types (see `spec/generics.md` §5).

## 3. Interface Summary and Dependency Graph

- `ae deps <source.ae>` lists each module in loading order, its `uses` dependencies, the count of public interface entries, and its summary hash, concluding with the program-wide fingerprint.
- The summary consists solely of `pub` function signatures, `pub` struct/enum shapes, and generic declarations. Function bodies, private items, comments, and local variables do not participate: **modifying only bodies leaves all hashes unchanged; modifying a `pub` signature changes the module's and the program's fingerprints.**
- Path labels use `/` for cross-platform comparability; case is not folded (Windows/Linux case differences remain a platform discrepancy).
- The summary is today a **diagnostic and planning tool only**: it does not participate in cache invalidation. The backend is still one translation unit per program, so a body change still recompiles part of the program (see `docs/current-language.md`, Toolchain). Per-package reuse is M3.5 territory.
- `deps` fully loads and checks the program; syntax and type errors surface exactly as in `check`.

## 4. Test Matrix

| # | Case | Expected |
|---|---|---|
| M1 | Duplicate import of the same file (diamond) | Loaded once, compiles |
| M2 | True import cycle | Rejected with the closed cycle in import order |
| M3 | Cross-file use of a non-`pub` function | Rejected with the `mark it 'pub'` message |
| M4 | `pub fn` exposing a file-private struct | Rejected at the definition site |
| M5 | Same private name declared in two files | Rejected (collision) |
| M6 | Body-only change leaves the `deps` summary hash unchanged | Hashes identical |
| M7 | `pub` signature change changes the module and program fingerprints | Hashes differ |
| M8 | Cross-file instantiation of a `pub` generic that calls a private helper | Runs |

See `examples/modules/` and the multi-file cases in `tests/run`; `ae deps examples/modules/*.ae` shows the summary surface.
