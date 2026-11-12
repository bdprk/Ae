# ADR-0137: ASCII Case Compare, Tokenizer State, Path Parsers, and LEB128 Decoding

## Context
Compilers, lexers, CLI arguments, and file path manipulation require:
1. Case-insensitive ASCII character comparison and sorting predicates.
2. Tokenizer cursor offset, line, and column position tracking on character advance.
3. Path separator detection and file extension extraction offsets.
4. Streaming stateful LEB128 variable-length integer decoding.

## Design
- `std/core/ascii_case.ae`: `ascii_char_eq_ignore_case`, `ascii_compare_char`.
- `std/core/tokenizer_state.ae`: `TokenizerState`, `tokenizer_advance_char`.
- `std/core/path_parser.ae`: `path_is_separator`, `path_find_last_separator`, `path_find_extension_dot`.
- `std/core/leb128_decode.ae`: `Leb128Decoder`, `leb128_decoder_feed_byte`.
