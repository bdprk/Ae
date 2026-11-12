# ADR-0135: Buffer Cursor, Event Counter, Hash Combiner, and Numeric Intervals

## Context
Stream parsers, performance metrics, composite hashing, and range queries require:
1. Bounded buffer read/write position cursors with remaining space queries and clamping.
2. Event success/failure accounting and error rate per mille computation.
3. Overflow-safe hash combination algebra for composite composite keys.
4. Closed interval `[start, end]` ranges with containment and overlap testing.

## Design
- `std/core/cursor.ae`: `BufferCursor`, `cursor_advance`, `cursor_remaining`, `cursor_has_remaining`, `cursor_reset`.
- `std/core/counter.ae`: `EventCounter`, `event_counter_record_success`, `event_counter_record_error`, `event_counter_error_rate_permille`.
- `std/core/hash_combine.ae`: `hash_combine_i64` and `hash_pair_i64`.
- `std/core/interval.ae`: `Interval`, `interval_length`, `interval_contains`, `interval_overlaps`.
