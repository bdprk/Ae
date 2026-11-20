# ADR-0223: Language Server Protocol Engine (M6.2.01–M6.2.07)

Status: Accepted (2026-11-20)

## Context

ae requires first-class editor integration to support daily development across VS Code, Cursor, Zed, and Neovim without third-party plugins or ad-hoc daemon processes.

The requirements from Milestone M6.2 are:
- **M6.2.01**: Synchronize sequential in-memory unsaved edits (including multi-byte UTF-8 characters) against compiler ASTs.
- **M6.2.02**: Debounce/version-filter rapid edits and publish diagnostics strictly for the latest document revision.
- **M6.2.03**: Return hover type information and declarations for symbols under cursor.
- **M6.2.04**: Provide definition jumping across imports to exact source ranges.
- **M6.2.05**: Context-aware member and identifier completion based on receiver types.
- **M6.2.06**: Safe multi-file symbol renaming across module boundaries without clobbering shadowed locals.
- **M6.2.07**: Stale query cancellation (`$/cancelRequest`).

## Decision

1. **Architecture**:
   - Provide `ae lsp` as a built-in CLI command speaking standard JSON-RPC 2.0 over stdin/stdout.
   - Maintain an in-memory document store (`DocumentStore`) tracking open file versions, UTF-8 text buffers, and incremental line index tables.
   - Code is structured cleanly and concisely without explanatory comments, relying on expressive types and functions.

2. **Protocol Methods**:
   - `initialize` / `initialized` / `shutdown` / `exit`.
   - `textDocument/didOpen`, `textDocument/didChange`, `textDocument/didClose`.
   - `textDocument/publishDiagnostics`.
   - `textDocument/hover`.
   - `textDocument/definition`.
   - `textDocument/completion`.
   - `textDocument/rename`.
   - `$/cancelRequest`.

3. **Parity and Safety**:
   - Use identical semantic resolution and type-checking rules as the compiler frontend.
