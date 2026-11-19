//! Diagnostic snippet rendering with the pinned display policy (ADR-0212).
//!
//! Diagnostics address positions by 1-based character columns plus absolute
//! UTF-8 byte offsets, which is what an editor needs. A terminal additionally
//! has to decide how many cells each character occupies when drawing an
//! underline. The policy: a tab advances to the next multiple of
//! [`TAB_WIDTH`], East Asian wide/fullwidth characters (the BMP CJK ranges in
//! [`cell_width`]) occupy 2 cells, and every other character occupies 1. An
//! underline is one `^` per occupied cell, so the terminal points at the same
//! token the editor does; a terminal must be configured with the same tab
//! width for the two to align.

/// How many terminal cells a tab advances to: the next multiple of this.
pub(super) const TAB_WIDTH: usize = 4;

/// Cells one character occupies. The wide set is the BMP CJK/fullwidth
/// ranges a source file realistically contains; anything outside them
/// occupies one cell rather than guessing from an incomplete table.
fn cell_width(ch: char) -> usize {
    let code = ch as u32;
    let wide = matches!(code,
        0x1100..=0x115F   // Hangul Jamo
        | 0x2E80..=0x303F // CJK radicals, Kangxi radicals, CJK punctuation
        | 0x3040..=0x318F // Hiragana, Katakana, Bopomofo
        | 0x3190..=0x31FF // CJK compatibility, Katakana phonetic extensions
        | 0x3400..=0x4DBF // CJK Unified Ideographs Extension A
        | 0x4E00..=0x9FFF // CJK Unified Ideographs
        | 0xAC00..=0xD7A3 // Hangul syllables
        | 0xF900..=0xFAFF // CJK compatibility ideographs
        | 0xFF00..=0xFF60 // Fullwidth forms
        | 0xFFE0..=0xFFE6 // Fullwidth signs
    );
    if wide { 2 } else { 1 }
}

/// Cells `text` occupies when drawn starting at 0-based `column`; tabs jump
/// to the next [`TAB_WIDTH`] stop instead of occupying one cell.
fn advance(column: usize, text: &str) -> usize {
    let mut current = column;
    for ch in text.chars() {
        current = if ch == '\t' {
            (current / TAB_WIDTH + 1) * TAB_WIDTH
        } else {
            current + cell_width(ch)
        };
    }
    current
}

/// Renders one snippet: the numbered source line and a caret line underlining
/// the span's cells. The span is clipped to the requested line (a multi-line
/// span underlines its first line only). Returns `None` when the line number
/// is outside the source or the span starts past the line's text, in which
/// case the caller prints the message without a snippet.
pub(super) fn render_snippet(source: &str, line: usize, byte_start: usize, byte_end: usize) -> Option<String> {
    if line == 0 { return None; }
    let mut line_start = 0usize;
    for (index, raw) in source.split_inclusive('\n').enumerate() {
        if index + 1 == line {
            let without_newline = raw.strip_suffix('\n').unwrap_or(raw);
            let text = without_newline.strip_suffix('\r').unwrap_or(without_newline);
            if byte_start > line_start + text.len() { return None; }
            let from = (byte_start - line_start).min(text.len());
            let to = byte_end.min(line_start + text.len()).max(line_start + from) - line_start;
            let caret_column = advance(0, &text[..from]);
            let caret_width = advance(0, &text[from..to]);
            let number = line.to_string();
            let pad = " ".repeat(number.len());
            return Some(format!(
                "{pad}{line} | {text}\n{pad}{blank} | {caret}{marks}",
                blank = pad,
                caret = " ".repeat(caret_column),
                marks = "^".repeat(caret_width),
            ));
        }
        line_start += raw.len();
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tabs_advance_to_the_next_stop_and_wide_characters_take_two_cells() {
        // A tab from column 1 lands on 4; from column 4 it lands on 8.
        assert_eq!(advance(0, "\t"), 4);
        assert_eq!(advance(4, "\t"), 8);
        assert_eq!(advance(2, "\t"), 4);
        // The pinned wide set occupies two cells; ASCII one.
        assert_eq!(advance(0, "中"), 2);
        assert_eq!(advance(0, "é"), 1);
        assert_eq!(advance(0, "世界x"), 5);
        // Tab stops inside a run: tab, wide char, then one ASCII char.
        assert_eq!(advance(0, "\t中x"), 7);
        // A byte-offset prefix lands the same way as the equivalent text:
        // `ab`→2, tab from 2→4, `c`→5.
        assert_eq!(advance(0, "ab\tc"), 5);
    }

    #[test]
    fn snippets_underline_the_span_after_a_tab_and_wide_characters() {
        let source = "fn main() {\n\tlet s = \"中文\"; print(nope);\n}\n";
        // `nope` occupies four cells starting at cell 26: tab→4, `let s = "`→13,
        // `中文`→17, `"; print(`→26 (the tab is one byte, one char, four cells).
        let start = source.find("nope").unwrap();
        let rendered = render_snippet(source, 2, start, start + 4).unwrap();
        let lines: Vec<&str> = rendered.split('\n').collect();
        assert_eq!(lines[0], " 2 | \tlet s = \"中文\"; print(nope);");
        assert_eq!(lines[1], format!("   | {}^^^^", " ".repeat(26)));
    }

    #[test]
    fn multiline_spans_clip_to_the_first_line_and_missing_lines_return_none() {
        let source = "fn main() {\n    return nope\n        + 1;\n}\n";
        let start = source.find("nope").unwrap();
        let end = source.find("1;").unwrap() + 2;
        let rendered = render_snippet(source, 2, start, end).unwrap();
        assert!(rendered.contains("    return nope"), "{rendered}");
        // The underline covers `nope` (11 cells in: 4 spaces + `return `) and
        // clips at the newline; the `+ 1;` continuation is not drawn.
        assert!(rendered.contains(&format!("   | {}^^^^", " ".repeat(11))), "{rendered}");
        assert!(!rendered.contains('+'), "{rendered}");
        assert!(render_snippet(source, 9, start, start + 4).is_none());
        assert!(render_snippet(source, 0, start, start + 4).is_none());
        // A span starting past the line's text is not drawable.
        assert!(render_snippet(source, 1, start, start + 4).is_none());
    }

    #[test]
    fn crlf_lines_render_without_the_carriage_return() {
        let source = "fn main() {\r\n    print(nope);\r\n}\r\n";
        let start = source.find("nope").unwrap();
        let rendered = render_snippet(source, 2, start, start + 4).unwrap();
        assert!(rendered.contains("    print(nope);\n"), "{rendered}");
        assert!(!rendered.contains('\r'), "{rendered}");
        assert!(rendered.contains("^^^^"), "{rendered}");
    }
}
