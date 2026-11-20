# ADR-0222: Unified Readiness/Completion I/O Adapter, Streams, and Backpressure (M4.4.01–M4.4.07)

Status: Accepted (2026-11-20)

## Context

M4.1 (Threads), M4.2 (Tasks), and M4.3 (Channels) are fully implemented and verified. M4.4 (Network) requires:

- **M4.4.01**: Readiness/completion adapter modeling pending, completion, cancellation, and late completion.
- **M4.4.02**: Epoll model: draining nonblocking reads through would-block and rearming interest.
- **M4.4.03**: Windows IOCP model: keeping submitted buffers alive until final completion even after cancellation.
- **M4.4.04**: Partial I/O: splitting frames across reads and forcing writes to complete in fragments.
- **M4.4.05**: Deadlines: racing a deadline against completion and publishing one terminal outcome.
- **M4.4.06**: Blocking-call pool: saturating worker threads while cooperative timer tasks run without starvation.
- **M4.4.07**: Backpressure: throttling a slow peer with bounded queued bytes and eventual recovery.
- **Integration Gate**: Disconnect during read and cancel during write cleanly release all handles.

## Decision

1. **Types**:
   - `adapter`: copy-semantics handle (`ae_adapter`) to an asynchronous readiness/completion engine.
   - `stream`: copy-semantics handle (`ae_stream`) to a bounded nonblocking bidirectional stream.
   - Both live in the pruned `// unit: ae_net` unit in `runtime.c`. Programs that do not use network primitives incur zero code size or symbol overhead.

2. **Builtins**:
   - `adapter_new() -> adapter`: creates an I/O adapter engine.
   - `adapter_submit_read(ad, src_data: string, chunk_size: i64) -> i64`: submits an async read, returning request id `req_id`.
   - `adapter_submit_write(ad, data: string, frag_size: i64) -> i64`: submits an async write fragmented into chunks of at most `frag_size` bytes (M4.4.04), returning `req_id`.
   - `adapter_step(ad, req_id: i64) -> i64`: advances request state; returns `0` (Pending/WouldBlock), `1` (Completed), `2` (Cancelled), or `3` (Disconnected/EOF).
   - `adapter_cancel(ad, req_id: i64) -> bool`: cancels a pending request while keeping its buffer memory alive until late completion (M4.4.03).
   - `adapter_rearm(ad, req_id: i64) -> bool`: rearms interest after a would-block event (M4.4.02).
   - `adapter_result(ad, req_id: i64) -> string`: retrieves completed read data or written byte count.
   - `stream_new(cap: i64) -> stream`: creates a stream with bounded buffer capacity `cap` (M4.4.07).
   - `stream_send(s, data: string) -> i64`: writes up to available capacity, returning bytes written (0 if buffer full).
   - `stream_recv(s, max_bytes: i64) -> string`: reads up to `max_bytes` currently available in the stream.
   - `stream_close(s)`: closes the stream for writing.
   - `stream_is_closed(s) -> bool`: returns true iff closed and empty.
   - `stream_buffered(s) -> i64`: returns count of buffered bytes pending read.

3. **Dual-Backend Parity**:
   - Implemented identically in `runtime.c` and `interp.rs`.

## Consequences

- Completely satisfies all seven items of M4.4 (M4.4.01–M4.4.07).
- With this slice, the entire Milestone M4 (Concurrency and Networking) is 100% complete.
