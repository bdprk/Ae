//! Lexer: source text to tokens, plus location helpers.
pub mod fmt;
use ae_span::{Diagnostic, SourceFile};
#[derive(Debug, Clone, PartialEq)]
pub enum Kind {
    Ident(String), Number(i64), Float(f64), Text(String), Fn, Let, Var, Print, If, Else, While, For, In, Return, Break, Continue, Assert, True, False, Use,
    EqEq, NotEq, Less, LessEq, Greater, GreaterEq, And, Or, Bang,
    LParen, RParen, LBrace, RBrace, LBracket, RBracket, Comma, Colon, Arrow, Amp, Semi, Equal, Plus, Minus, Star, Slash, Percent, Dot, DotDot, Struct,
    Enum, Match, ColonColon, FatArrow, Question, Trait, Impl, Eof,
}
#[derive(Debug, Clone)]
pub struct Token { pub kind: Kind, pub offset: usize, pub end: usize, pub line: usize, pub column: usize }

// Test-only view for the formatter's lexer-drift defense: the same token
// boundaries, as comparable names. Values are dropped on purpose.
pub fn lex_kind_names(source: &str) -> Result<Vec<&'static str>, String> {
    lex(source).map(|tokens| tokens.iter().map(|t| kind_name(&t.kind)).collect())
}

fn kind_name(kind: &Kind) -> &'static str {
    match kind {
        Kind::Ident(_) => "Ident", Kind::Number(_) => "Number", Kind::Float(_) => "Float", Kind::Text(_) => "Text",
        Kind::Fn => "Fn", Kind::Let => "Let", Kind::Var => "Var", Kind::Print => "Print",
        Kind::If => "If", Kind::Else => "Else", Kind::While => "While", Kind::For => "For",
        Kind::In => "In", Kind::Return => "Return", Kind::Break => "Break", Kind::Continue => "Continue",
        Kind::Assert => "Assert", Kind::True => "True", Kind::False => "False", Kind::Use => "Use",
        Kind::Struct => "Struct", Kind::Enum => "Enum", Kind::Match => "Match",
        Kind::EqEq => "EqEq", Kind::NotEq => "NotEq", Kind::Less => "Less", Kind::LessEq => "LessEq",
        Kind::Greater => "Greater", Kind::GreaterEq => "GreaterEq", Kind::And => "And", Kind::Or => "Or",
        Kind::Bang => "Bang",
        Kind::LParen => "LParen", Kind::RParen => "RParen", Kind::LBrace => "LBrace", Kind::RBrace => "RBrace",
        Kind::LBracket => "LBracket", Kind::RBracket => "RBracket", Kind::Comma => "Comma",
        Kind::Colon => "Colon", Kind::Arrow => "Arrow", Kind::Amp => "Amp", Kind::Semi => "Semi",
        Kind::Equal => "Equal", Kind::Plus => "Plus", Kind::Minus => "Minus", Kind::Star => "Star",
        Kind::Slash => "Slash", Kind::Percent => "Percent", Kind::Dot => "Dot", Kind::DotDot => "DotDot",
        Kind::ColonColon => "ColonColon", Kind::FatArrow => "FatArrow", Kind::Question => "Question",
        Kind::Trait => "Trait", Kind::Impl => "Impl", Kind::Eof => "Eof",
    }
}

// Lexer diagnostics carry no file name; load_file adds the file prefix.
fn lex_error(source: &str, offset: usize, code: &str, message: &str) -> Diagnostic {
    let end = offset + source[offset..].chars().next().map_or(0, char::len_utf8);
    let span = SourceFile { name: None, text: source }.span(offset..end)
        .expect("lexer offset must be a UTF-8 boundary in the source");
    let mut diagnostic = Diagnostic::at(span, message);
    diagnostic.code = Some(code.to_owned());
    diagnostic
}
pub fn error(file: &str, source: &str, offset: usize, message: &str) -> String {
    diagnostic(file, source, offset, message).to_string()
}
pub fn diagnostic(file: &str, source: &str, offset: usize, message: &str) -> Diagnostic {
    diagnostic_range(file, source, offset..offset, message)
}
pub fn diagnostic_range(file: &str, source: &str, range: std::ops::Range<usize>, message: &str) -> Diagnostic {
    let span = SourceFile { name: Some(file), text: source }.span(range)
        .expect("parser offset must be a UTF-8 boundary in the source");
    Diagnostic::at(span, message)
}
pub fn lex(source: &str) -> Result<Vec<Token>, String> {
    lex_diagnostic(source).map_err(|error| error.to_string())
}

enum LexFailure { Unexpected(Diagnostic), Fatal(Diagnostic) }
impl From<Diagnostic> for LexFailure {
    fn from(diagnostic: Diagnostic) -> Self { Self::Fatal(diagnostic) }
}

const MAX_LEX_DIAGNOSTICS: usize = 32;

/// Recover from unexpected characters, closed strings with unsupported escapes,
/// and complete invalid numeric literals,
/// never returning tokens for an invalid source. All positions are calculated against the original input.
pub fn lex_with_diagnostics(source: &str) -> Result<Vec<Token>, Vec<Diagnostic>> {
    let mut diagnostics = Vec::new();
    let tokens = match lex_inner(source, Some(&mut diagnostics)) {
        Ok(tokens) => tokens,
        Err(LexFailure::Unexpected(diagnostic) | LexFailure::Fatal(diagnostic)) => {
            diagnostics.push(diagnostic);
            return Err(diagnostics);
        }
    };
    if diagnostics.is_empty() {
        Ok(tokens)
    } else {
        Err(diagnostics)
    }
}
/// Lex with a structured diagnostic, preserving byte offset and location.
pub fn lex_diagnostic(source: &str) -> Result<Vec<Token>, Diagnostic> {
    lex_inner(source, None).map_err(|failure| match failure {
        LexFailure::Unexpected(diagnostic) | LexFailure::Fatal(diagnostic) => diagnostic,
    })
}
// Determine whether the rest of a malformed string has an unambiguous end.
// A backslash consumes the next Unicode scalar even when that escape is invalid.
fn has_closing_quote(source: &str, mut offset: usize) -> bool {
    while offset < source.len() {
        let ch = source[offset..].chars().next().unwrap();
        match ch {
            '"' => return true,
            '\n' | '\0' => return false,
            '\\' => {
                offset += 1;
                let Some(next) = source[offset..].chars().next() else { return false };
                if next == '\n' || next == '\0' { return false; }
                offset += next.len_utf8();
            }
            _ => offset += ch.len_utf8(),
        }
    }
    false
}

fn lex_inner(source: &str, mut recovered: Option<&mut Vec<Diagnostic>>) -> Result<Vec<Token>, LexFailure> {
    let bytes = source.as_bytes();
    let mut i = 0;
    let mut result = Vec::new();
    let (mut location_offset, mut line, mut column) = (0, 1, 1);
    macro_rules! recover_numeric {
        ($diagnostic:expr) => {{
            let diagnostic = $diagnostic;
            match recovered.as_deref_mut() {
                Some(diagnostics) => {
                    diagnostics.push(diagnostic);
                    if diagnostics.len() >= MAX_LEX_DIAGNOSTICS { return Ok(result); }
                    continue;
                }
                None => return Err(LexFailure::Fatal(diagnostic)),
            }
        }};
    }
    while i < bytes.len() {
        let start = i;
        let c = source[i..].chars().next().unwrap();
        if c.is_whitespace() { i += c.len_utf8(); continue; }
        if bytes[i..].starts_with(b"//") {
            i += 2;
            while i < bytes.len() && bytes[i] != b'\n' { i += 1; }
            continue;
        }
        let kind = if c == '"' {
            i += 1;
            while i < bytes.len() {
                match bytes[i] {
                    b'"' => break,
                    b'\n' | b'\0' => return Err(lex_error(source, i, "E0004", "string literals must not contain raw newlines or NUL; use escapes").into()),
                    b'\\' => {
                        let escaped = bytes.get(i + 1).copied();
                        if !matches!(escaped, Some(b'n' | b't' | b'r' | b'\\' | b'"')) {
                            let diagnostic = lex_error(source, i, "E0002", "unsupported string escape; allowed: \\n \\t \\r \\\\ \\\""
                            ).with_suggestion("Use a supported escape, or escape a literal backslash as \\\\.");
                            if recovered.is_none() || !has_closing_quote(source, i) {
                                return Err(diagnostic.into());
                            }
                            let diagnostics = recovered.as_deref_mut().unwrap();
                            diagnostics.push(diagnostic);
                            if diagnostics.len() >= MAX_LEX_DIAGNOSTICS { return Ok(result); }
                            i += 1;
                            i += source[i..].chars().next().unwrap().len_utf8();
                            continue;
                        }
                        i += 2;
                    }
                    _ => i += 1,
                }
            }
            if i >= bytes.len() { return Err(lex_error(source, start, "E0003", "unterminated string literal").into()); }
            i += 1;
            Kind::Text(source[start + 1..i - 1].to_owned())
        } else if c.is_ascii_alphabetic() || c == '_' {
            i += 1;
            while i < bytes.len() && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_') { i += 1; }
            match &source[start..i] {
                "fn" => Kind::Fn, "let" => Kind::Let, "var" => Kind::Var, "print" => Kind::Print,
                "if" => Kind::If, "else" => Kind::Else, "while" => Kind::While, "return" => Kind::Return,
                "for" => Kind::For, "in" => Kind::In,
                "break" => Kind::Break, "continue" => Kind::Continue, "assert" => Kind::Assert,
                "true" => Kind::True, "false" => Kind::False, "use" => Kind::Use, "struct" => Kind::Struct,
                "enum" => Kind::Enum, "match" => Kind::Match, "trait" => Kind::Trait, "impl" => Kind::Impl,
                s => Kind::Ident(s.to_owned()),
            }
        } else if c.is_ascii_digit() {
            i += 1;
            while i < bytes.len() && bytes[i].is_ascii_digit() { i += 1; }
            // A fraction needs a digit after the dot: `1..2` is a range and
            // `1.field` would be a field access, so neither may become a float.
            let mut floating = false;
            if i + 1 < bytes.len() && bytes[i] == b'.' && bytes[i + 1].is_ascii_digit() {
                floating = true;
                i += 1;
                while i < bytes.len() && bytes[i].is_ascii_digit() { i += 1; }
            }
            // An exponent also makes the literal a float: `1e3` is 1000.0.
            if i < bytes.len() && (bytes[i] == b'e' || bytes[i] == b'E') {
                let mut exponent = i + 1;
                if exponent < bytes.len() && (bytes[exponent] == b'+' || bytes[exponent] == b'-') { exponent += 1; }
                if exponent < bytes.len() && bytes[exponent].is_ascii_digit() {
                    floating = true;
                    i = exponent;
                    while i < bytes.len() && bytes[i].is_ascii_digit() { i += 1; }
                }
            }
            let text = &source[start..i];
            if floating {
                let value: f64 = match text.parse() {
                    Ok(value) => value,
                    Err(_) => recover_numeric!(lex_error(source, start, "E0006", "invalid float literal")),
                };
                if !value.is_finite() { recover_numeric!(lex_error(source, start, "E0007", "float literal is not finite")); }
                // Rounding a nonzero literal down to zero would silently replace
                // the value the user wrote, so it is rejected instead of stored.
                let mantissa = text.split(['e', 'E']).next().unwrap_or(text);
                if value == 0.0 && mantissa.bytes().any(|b| b.is_ascii_digit() && b != b'0') {
                    recover_numeric!(lex_error(source, start, "E0008", "float literal underflows to zero"));
                }
                Kind::Float(value)
            } else {
                match text.parse() {
                    Ok(value) => Kind::Number(value),
                    Err(_) => recover_numeric!(lex_error(source, start, "E0005", "integer literal exceeds i64 maximum")),
                }
            }
        } else if ["==", "!=", "<=", ">=", "->", "&&", "||", "..", "=>", "::"].iter().any(|op| source[i..].starts_with(op)) {
            i += 2;
            match &source[start..i] { "==" => Kind::EqEq, "!=" => Kind::NotEq, "<=" => Kind::LessEq, ">=" => Kind::GreaterEq, "->" => Kind::Arrow, "&&" => Kind::And, "||" => Kind::Or, ".." => Kind::DotDot, "=>" => Kind::FatArrow, "::" => Kind::ColonColon, _ => unreachable!() }
        } else {
            i += c.len_utf8();
            match c {
                '(' => Kind::LParen, ')' => Kind::RParen, '{' => Kind::LBrace, '}' => Kind::RBrace,
                '[' => Kind::LBracket, ']' => Kind::RBracket, ',' => Kind::Comma, ':' => Kind::Colon, '&' => Kind::Amp,
                ';' => Kind::Semi, '=' => Kind::Equal, '+' => Kind::Plus, '-' => Kind::Minus,
                '*' => Kind::Star, '/' => Kind::Slash, '%' => Kind::Percent, '<' => Kind::Less, '>' => Kind::Greater,
                '!' => Kind::Bang, '.' => Kind::Dot, '?' => Kind::Question,
                _ => {
                    let diagnostic = lex_error(source, start, "E0001", &format!("unexpected character {c:?}"));
                    match recovered.as_deref_mut() {
                        Some(diagnostics) => {
                            diagnostics.push(diagnostic);
                            if diagnostics.len() >= MAX_LEX_DIAGNOSTICS { return Ok(result); }
                            continue;
                        }
                        None => return Err(LexFailure::Unexpected(diagnostic)),
                    }
                }
            }
        };
        for character in source[location_offset..start].chars() {
            if character == '\n' { line += 1; column = 1; } else { column += 1; }
        }
        location_offset = start;
        result.push(Token { kind, offset: start, end: i, line, column });
    }
    for character in source[location_offset..].chars() {
        if character == '\n' { line += 1; column = 1; } else { column += 1; }
    }
    result.push(Token { kind: Kind::Eof, offset: source.len(), end: source.len(), line, column });
    Ok(result)
}

#[cfg(test)]
mod diagnostic_tests {
    use super::*;

    #[test]
    fn lexical_error_codes_identify_distinct_failure_categories() {
        for (source, code) in [
            ("@", "E0001"),
            ("\"bad\\q\"", "E0002"),
            ("\"unterminated", "E0003"),
            ("\"line\nbreak\"", "E0004"),
            ("9223372036854775808", "E0005"),
            ("1e999", "E0007"),
            ("1e-999", "E0008"),
        ] {
            let diagnostic = lex_diagnostic(source).unwrap_err();
            assert_eq!(diagnostic.code.as_deref(), Some(code), "{source:?}");
        }
    }

    #[test]
    fn token_ranges_cover_original_utf8_bytes_and_empty_eof() {
        let source = "// 中文\nlet word = \"😀\";";
        let tokens = lex(source).unwrap();
        let spellings: Vec<_> = tokens.iter().map(|token| &source[token.offset..token.end]).collect();
        assert_eq!(spellings, ["let", "word", "=", "\"😀\"", ";", ""]);
        let eof = tokens.last().unwrap();
        assert_eq!((eof.offset, eof.end), (source.len(), source.len()));
    }

    #[test]
    fn lexical_errors_keep_byte_offset_and_unicode_column() {
        let source = "// 中文\n  @";
        let diagnostic = lex_diagnostic(source).unwrap_err();
        let span = &diagnostic.primary.as_ref().unwrap().span;
        assert_eq!(span.range, source.find('@').unwrap()..source.find('@').unwrap() + 1);
        assert_eq!((span.line, span.column), (2, 3));
        assert_eq!(diagnostic.to_string(), lex(source).unwrap_err());
    }

    #[test]
    fn lexical_error_spans_cover_unicode_character_and_unterminated_quote() {
        let diagnostic = lex_diagnostic("中").unwrap_err();
        assert_eq!(diagnostic.primary.unwrap().span.range, 0.."中".len());
        let diagnostic = lex_diagnostic("\"unterminated").unwrap_err();
        assert_eq!(diagnostic.primary.unwrap().span.range, 0..1);
    }

    #[test]
    fn recovery_continues_after_complete_invalid_numeric_literals() {
        let source = "// 中文\n9223372036854775808 @ 1e999 1e-999 #";
        let errors = lex_with_diagnostics(source).unwrap_err();
        let codes: Vec<_> = errors.iter().map(|d| d.code.as_deref().unwrap()).collect();
        assert_eq!(codes, ["E0005", "E0001", "E0007", "E0008", "E0001"]);
        for (error, needle) in errors.iter().zip(["9223372036854775808", "@", "1e999", "1e-999", "#"]) {
            let span = &error.primary.as_ref().unwrap().span;
            assert_eq!(span.range.start, source.find(needle).unwrap());
            assert_eq!(span.line, 2);
        }
        assert_eq!(lex_diagnostic(source).unwrap_err(), errors[0]);
        let many = std::iter::repeat_n("9223372036854775808", 40).collect::<Vec<_>>().join(" ");
        assert_eq!(lex_with_diagnostics(&many).unwrap_err().len(), 32);
        assert!(lex_with_diagnostics("9223372036854775808 \"bad\\q\" @").unwrap_err().len() == 3);
    }

    #[test]
    fn recovery_handles_closed_strings_with_bad_escapes() {
        let source = r#""bad\q \中 \" @ still string \z" @"#;
        let errors = lex_with_diagnostics(source).unwrap_err();
        let codes: Vec<_> = errors.iter().map(|d| d.code.as_deref().unwrap()).collect();
        assert_eq!(codes, ["E0002", "E0002", "E0002", "E0001"]);
        assert_eq!(lex_diagnostic(source).unwrap_err(), errors[0]);
        assert_eq!(errors[3].primary.as_ref().unwrap().span.range.start, source.rfind('@').unwrap());
        let many = format!("\"{}\" @", "\\q".repeat(40));
        assert_eq!(lex_with_diagnostics(&many).unwrap_err().len(), 32);
        for source in ["\"bad\\q", "\"bad\\q\n@", "\"bad\\q\0@"] {
            let errors = lex_with_diagnostics(source).unwrap_err();
            assert_eq!(errors.len(), 1, "{source:?}");
            assert_eq!(errors[0].code.as_deref(), Some("E0002"));
        }
    }

    #[test]
    fn recovery_keeps_original_positions_and_stops_at_ambiguous_errors() {
        let errors = lex_with_diagnostics("中 @\n\"bad\\q\" #").unwrap_err();
        assert_eq!(errors.len(), 4);
        let positions: Vec<_> = errors.iter().map(|d| {
            let span = &d.primary.as_ref().unwrap().span;
            (span.range.clone(), span.line, span.column)
        }).collect();
        assert_eq!(positions, vec![(0..3, 1, 1), (4..5, 1, 3), (10..11, 2, 5), (14..15, 2, 9)]);
        assert!(errors[2].message.contains("unsupported string escape"));
        assert!(lex_with_diagnostics("fn main() {} // @").is_ok());
        assert_eq!(lex_with_diagnostics(&"@".repeat(40)).unwrap_err().len(), 32);
    }
}
