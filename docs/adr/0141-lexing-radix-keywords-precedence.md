# ADR-0141: Lexer Escapes, Literal Radix, Keyword Metrics, and Operator Precedence

## Context
Under the Standard Library Blueprint (Domain C: Text & Lexing Engine), the compiler frontend requires:
1. Standard escape character decoding (`\n`, `\r`, `\t`, `\\`, `\"`, `\0`).
2. Literal numeric radix representations (Decimal, Hexadecimal, Binary, Octal) and digit validations.
3. Common keyword length metrics (`fn`, `let`, `struct`, `match`).
4. Operator token precedence hierarchies for expression parsing.

## Design
- `std/core/char_escape.ae`: `escape_char_to_byte`, `is_valid_escape_char`.
- `std/core/literal_num.ae`: `NumRadix`, `num_radix_base`, `is_binary_digit`, `is_octal_digit`.
- `std/core/keyword_matcher.ae`: `is_keyword_byte_match`, keyword length metrics.
- `std/core/operator_kind.ae`: `OperatorPrecedence`, `operator_precedence_rank`, `operator_is_higher`.
