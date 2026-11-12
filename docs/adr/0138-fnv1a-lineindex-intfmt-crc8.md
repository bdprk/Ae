# ADR-0138: FNV-1a Hash, Line Indexer, Integer Formatter, and CRC-8

## Context
High speed symbol tables, source mapping, integer text serialization, and packet validation require:
1. Low-latency non-cryptographic FNV-1a byte streaming hashing.
2. Source line indexer tracking newline boundaries.
3. Fixed 4-slot ASCII digit buffer formatter for integers 0..9999 without heap allocations.
4. CRC-8-CCITT packet checksum computation.

## Design
- `std/core/fnv1a.ae`: `fnv1a_init`, `fnv1a_feed_byte`.
- `std/core/line_indexer.ae`: `LineOffsets8`, `line_offsets8_add_line`.
- `std/core/int_formatter.ae`: `DigitBuffer4`, `digit_buffer4_from_u16`.
- `std/core/crc8.ae`: `crc8_update_byte`.
