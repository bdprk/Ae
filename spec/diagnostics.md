# Diagnostic recovery (current implementation)

Token ranges use half-open UTF-8 byte offsets. The EOF token has an empty range at the end of the file. Parser errors raised at the current token or at a known token offset use that token's full range; existing text diagnostics continue to report the start line and column.

Each `check --json` diagnostic contains `file`, `line`, `column`, `message`, `byte_start`, and `byte_end`. The byte fields are zero-based offsets into the UTF-8 source with an exclusive end; both are `null` when no validated source range is available. Existing line and column fields remain one-based and count Unicode scalar values. A zero-length range is valid for EOF. JSON consumers must tolerate additional fields.

JSON diagnostics also contain `code`, a stable string for classified errors or `null` when no code has been assigned. The current lexical codes are:

| Code | Meaning |
|---|---|
| `E0001` | Unexpected character |
| `E0002` | Unsupported string escape |
| `E0003` | Unterminated string literal |
| `E0004` | Raw newline or NUL in a string literal |
| `E0005` | Integer literal exceeds the supported range |
| `E0006` | Invalid float literal |
| `E0007` | Float literal is not finite |
| `E0008` | Nonzero float literal underflows to zero |

These codes identify the error category; the human-readable message is not a stable identifier. No codes are invented for unclassified errors.

The following semantic codes are currently assigned where the corresponding checks are implemented:

| Code | Meaning |
|---|---|
| `E0101` | An unknown variable was referenced |
| `E0102` | A value was used or borrowed after it was moved |
| `E0103` | An immutable variable was assigned or modified |
| `E0104` | A duplicate variable binding was declared |

Every JSON diagnostic includes `suggestions`, an array of human-readable guidance strings. An empty array means no suggestion is available. Suggestions are not guaranteed edits and must not be applied automatically. The current implementation offers conditional guidance for unsupported string escapes (`E0002`) and immutable local or parameter bindings whose declarations are known (`E0103`); it does not infer the programmer's intent or suggest `var` for loop or pattern bindings.

Every JSON diagnostic includes `secondary`, an array of related source labels. Each label has `file`, `line`, `column`, `byte_start`, `byte_end`, and `message` (which may be `null`). When the declaration is known, `E0102`, `E0103`, and `E0104` include a label on the corresponding local or parameter binding. The `E0102` label identifies the declaration, not necessarily the move site; `E0104` identifies the earlier binding. A missing label is represented by an empty array; no declaration or move location is guessed.

When lexical analysis encounters an unexpected character outside a string or comment, `check --json` reports that character and may continue scanning for further lexical errors. Diagnostics retain source order, byte offsets, and line and Unicode-scalar column positions from the original source. No tokens from a source with lexical errors are accepted for parsing or code generation.

At most 32 lexical diagnostics are reported per source file. After a complete numeric literal has been scanned, an out-of-range or invalid numeric value is reported and scanning may resume at the end of that literal. An unsupported escape in a string with an unambiguous closing quote can be reported while scanning continues through the string and subsequent source. If the closing quote cannot be found before a raw newline, NUL, or end of file, scanning ends at the first unsupported escape. Unterminated strings and raw newlines or NUL without an earlier recoverable escape also stop scanning. Text-mode commands continue to report the first error.

When the first compilation error is lexical, JSON checking may also collect lexical errors from independently reachable sibling imports. It follows depth-first import order, visits canonical files once, and returns at most 32 diagnostics for the graph. A file with lexical errors cannot reveal further imports. An unreadable file, malformed import header, or import cycle is reported as a diagnostic and stops traversal of that branch; independently reachable siblings are still scanned. Loader errors without a verified source span have null location fields. Failed canonical files are reported once; a cycle does not change the state of a file already being visited. If its first diagnostics differ from the normal compilation result, the normal result takes precedence. Supplementary diagnostics never authorize compilation.

After declaration collection succeeds, `check --json` may report body diagnostics from ordinary functions and impl methods, in source order. Each body is checked using a fresh copy of the declaration state; failed checks cannot modify the state used by other functions. No program with errors is accepted for code generation. Recovery falls back to the original first diagnostic if its function boundaries or original error cannot be verified. Within a body, recovery can continue after an unknown variable used as the sole argument of `print` or `assert`, or as the entire condition of `if` or `while`. For each such error, the checker replays the function from the original declaration state with a side-effect-free, context-appropriate literal substituted solely for diagnostic collection. Other errors within the same body and declaration errors remain fail-fast. At most 32 body diagnostics are returned.
