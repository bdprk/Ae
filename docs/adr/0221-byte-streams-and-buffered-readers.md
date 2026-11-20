# ADR-0221: Byte Streams and Buffered Readers (M3.2.01, M3.2.02)

Status: Accepted (2026-11-20)

## Context

M3.2.03–M3.2.07 are complete; only M3.2.01 (byte streams with short reads and EOF) and M3.2.02 (buffered line reader spanning multiple buffers while preserving the following byte) remain in M3.2 Hosted Library.

## Decision

1. **Type (`reader`)**: `reader` is a copy-semantics handle (`ae_reader`) to a heap-allocated buffered byte stream, pruned behind `// unit: ae_reader` in `runtime.c`.
2. **Surface builtins** (all reserved names, non-`unsafe`):
   - `reader_new(data: string, chunk_size: i64, buf_cap: i64) -> reader`: creates a scripted stream over `data` whose underlying source yields at most `chunk_size` bytes per refill (`chunk_size >= 1`), backed by an internal buffer of capacity `buf_cap` (`buf_cap >= 1`).
   - `reader_read(r: reader, max_bytes: i64) -> string`: returns up to `min(max_bytes, chunk_size)` bytes (draining buffered bytes first, then at most one underlying short-read chunk), returning `""` at EOF.
   - `reader_read_line(r: reader) -> string`: refills the internal buffer as needed until `\n` or EOF, returning the full line (excluding trailing `\n` or `\r\n`) even when it spans multiple buffers, while preserving any bytes after `\n` in the internal buffer for subsequent reads.
   - `reader_eof(r: reader) -> bool`: returns `true` iff both the internal buffer and the underlying source are exhausted.
3. **Dual-backend parity**: implemented identically in `runtime.c` and `interp.rs`, verified byte-for-byte.

## Consequences

- Completes all seven items of M3.2 Hosted Library (M3.2.01–M3.2.07).
- Zero runtime cost for programs that do not use `reader`.

## Amendment 2026-11-20 — ADR-0224 R2.3 (reader migration to std/io)

The scripted reader left the compiler: `reader_new` / `reader_read` /
`reader_read_line` / `reader_eof` and the `reader` handle type were removed
(commit `5f6fc76`). The same semantics — chunked delivery capped at one chunk
per read, read_line refill windows bounded by `buf_cap`, one trailing CR
dropped, EOF yielding the empty string — now live in `std/io/reader.ae` as an
ordinary value (`ByteReader`) over the existing string primitives
(`slice` / `find` / `ends_with` / `join` / `strvec`). The state advances
through explicit `&mut` calls. `tests/run/byte_streams.ae` pins the former
builtin implementation's output byte-for-byte on both backends and all emit
modes.
