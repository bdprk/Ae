# ADR-0212: Diagnostic Display Policy for Tabs and Multibyte Text

- Date: 2026-11-19
- Status: Accepted; implemented in `src/render.rs` with the CLI surface `ae check --render` and the tests `tabs_advance_to_the_next_stop_and_wide_characters_take_two_cells`, `snippets_underline_the_span_after_a_tab_and_wide_characters`, `multiline_spans_clip_to_the_first_line_and_missing_lines_return_none`, `crlf_lines_render_without_the_carriage_return`, `check_render_underlines_tokens_after_tabs_and_wide_characters`.
- Scope: how diagnostics address positions and how a terminal draws an underline. It does not change the default `check` output; the plain messages and `--json` shape are frozen by existing tests.

## Evidence and problem

The charter's M0.1 gate is "editor and terminal point to the same token". `ae-span` already records absolute UTF-8 byte ranges and the JSON diagnostics expose `byte_start`/`byte_end`, but nothing pinned (a) how byte offsets behave after multibyte characters on the same line, and (b) how many terminal cells a character occupies when an underline is drawn. A tab or a CJK character before the error token otherwise makes every consumer guess.

## Decision

- **Position contract (already true, now pinned by test):** `byte_start`/`byte_end` are absolute UTF-8 byte offsets into the whole file; `line` is 1-based; `column` is a 1-based Unicode character position within the line (not bytes, not UTF-16 units). `json_spans_pin_utf8_byte_offsets_and_char_columns_after_multibyte_text` pins all three after `héllo世界` on one line.
- **Terminal cell policy:** a tab advances to the next multiple of 4 cells (`TAB_WIDTH`); the BMP East-Asian wide/fullwidth ranges (`U+1100..=115F`, `U+2E80..=303F`, `U+3040..=318F`, `U+3190..=31FF`, `U+3400..=4DBF`, `U+4E00..=9FFF`, `U+AC00..=D7A3`, `U+F900..=FAFF`, `U+FF00..=FF60`, `U+FFE0..=FFE6`) occupy 2 cells; every other character occupies 1. Nothing outside these ranges is guessed from an incomplete width table; extending the set is a spec change with its own fixture.
- **Snippet shape:** for each diagnostic with a drawable primary span, `check --render` prints the plain `ae: file:line:column: message` line, then the numbered source line and a caret line — `{pad}{line} | {text}` and `{pad}{pad} | {caret}{marks}` — where `caret` is `display_column(span start)` spaces and `marks` is one `^` per cell the span occupies, with tabs inside the span advancing per the same policy. Multi-line spans clip to the first line. CRLF line endings render without the carriage return. A diagnostic without a drawable span (legacy text message, unreadable or foreign file, span past the line) prints without a snippet rather than inventing one.
- **CLI surface:** `ae check --render [source.ae|project]` (both flag positions like `--json`). Success prints nothing and exits 0; diagnostics go to stderr and exit 1. The default `check` output is unchanged, so existing UI snapshots stay valid; adopting snippets as the default rendering is a separate decision with a snapshot rebless.
- **Consumer contract:** a terminal must be configured with tab width 4 for the caret to align with the tab-expanded source line; editors consume the byte offsets and character columns directly, so both point at the same token.

## Alternatives rejected

- Byte columns in diagnostics would break every existing column assertion and disagree with the LSP convention of character-oriented positions; rejected in favor of char columns plus separate byte offsets.
- Depending on a full Unicode width implementation would add a dependency for the rare ranges; the pinned BMP set covers real source text and is honest about its limits.
- Making snippets the default `check` output immediately would churn every pinned stderr snapshot in one step without a consumer that needs them yet; rejected.

## Required evidence

Unit tests pin the tab stops, the wide-character set, the caret geometry after a tab plus wide characters, multi-line clipping, CRLF handling, and the out-of-range `None` cases. The CLI test pins the whole rendered block (message line, source line, caret line) for a token that follows a tab and CJK characters, and the silent success path. Both platforms run the same policy; no platform-specific width table exists.
