# ADR-0147: Stream Chunks, I/O Error Kinds, Open Options, and Seek Arithmetic

## Context
Under the Standard Library Blueprint (Tier 3 std: Operating Environment & I/O), high performance file systems, sockets, and stream pipelines require:
1. Stream chunk fragmentation tracking (`StreamChunk`, `offset`, `size`, `is_last`, `next`).
2. Standardized I/O error classifications (`IoErrorKind`, `NotFound`, `PermissionDenied`, `TimedOut`, `io_error_is_recoverable`).
3. File open options bitflags (`OpenOptions`, `read_only`, `write_new`, `append`).
4. Seek position offset computation (`SeekFrom::Start`, `SeekFrom::Current`, `SeekFrom::End`).

## Design
- `std/core/stream_chunk.ae`: `StreamChunk`, `stream_chunk_next`, `stream_chunk_is_last`.
- `std/core/io_error_kind.ae`: `IoErrorKind`, `io_error_kind_code`, `io_error_is_recoverable`.
- `std/core/io_mode.ae`: `OpenOptions`, `open_options_read_only`, `open_options_write_new`, `open_options_append`.
- `std/core/io_seek.ae`: `SeekFrom`, `calculate_seek_offset`.
