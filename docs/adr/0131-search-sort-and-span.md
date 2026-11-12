# ADR-0131: Search, Sort, Span, and UTF-8 Boundaries

## Context
Compilers, linters, analyzers, and text processors require foundational algorithms:
1. Binary search and linear search over ordered collections.
2. In-place sorting and orderedness validation.
3. Diagnostic source spans with line, column, and file indexing.
4. UTF-8 byte stream boundary inspection.

## Design
- `std/core/search.ae`: `BinarySearchResult` tracking found status and insertion point; `binary_search_i64` and `linear_find_i64`.
- `std/core/sort.ae`: `is_sorted_i64_8` order validator and `bubble_sort_i64_8` fixed-buffer sorting.
- `std/core/span.ae`: `TextPosition` and `SourceSpan` primitives for precise diagnostics.
- `std/core/utf8.ae`: `is_utf8_continuation_byte` and `utf8_sequence_length`.
