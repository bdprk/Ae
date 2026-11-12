# ADR-0117: Send and Sync Concurrency Marker Traits (M1.4 / M4.1)

## Status
Accepted

## Context
Per Language Charter §2.4 and §2.6, safe concurrency requires compile-time prevention of data races. The `Send` marker indicates a type whose ownership can safely transfer across task/thread boundaries, while `Sync` indicates a type safe to share via immutable references.

## Decision
1. Introduce `Send` and `Sync` marker contracts in `std/core/sync.ae`.
2. Standard scalar types (`i64`, `bool`) implement both `Send` and `Sync`.
3. Concurrency scopes (`thread::scope`, `task::scope`) constrain passed payloads by `T: Send`.

## Verification
- Unit and run snapshot tests in `tests/run/sync_markers.ae`.
- Continuous stress regression on dual platforms (Windows x86-64 and Linux AMD EPYC on InstaBox).