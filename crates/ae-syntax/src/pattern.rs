use std::ops::Range;

use ae_span::Diagnostic;

use crate::{diagnostic_range, Kind, Token};

#[derive(Debug, PartialEq)]
pub struct Pattern {
    pub kind: PatternKind,
    pub range: Range<usize>,
}

#[derive(Debug, PartialEq)]
pub enum PatternKind {
    Binding(String),
    Variant { enum_name: String, variant: String, payload: Option<Vec<Pattern>> },
}

/// Parse a qualified variant pattern and preserve payload order and source ranges.
pub fn parse_variant_pattern(file: &str, source: &str) -> Result<Pattern, Diagnostic> {
    let tokens = crate::lex_diagnostic(source).map_err(|mut diagnostic| {
        if let Some(label) = &mut diagnostic.primary {
            label.span.file = Some(file.to_owned());
        }
        diagnostic
    })?;
    let mut parser = Parser { file, source, tokens: &tokens, cursor: 0, nodes: 0 };
    let pattern = parser.pattern(true)?;
    if parser.current().kind != Kind::Eof {
        return Err(parser.error("expected end of pattern"));
    }
    Ok(pattern)
}

struct Parser<'a> {
    file: &'a str,
    source: &'a str,
    tokens: &'a [Token],
    cursor: usize,
    nodes: usize,
}

impl Parser<'_> {
    fn current(&self) -> &Token {
        &self.tokens[self.cursor]
    }

    fn error(&self, message: &str) -> Diagnostic {
        let token = self.current();
        diagnostic_range(self.file, self.source, token.offset..token.end, message)
    }

    fn consume(&mut self, kind: Kind) -> bool {
        if self.current().kind != kind {
            return false;
        }
        self.cursor += 1;
        true
    }

    fn name(&mut self) -> Result<String, Diagnostic> {
        let Kind::Ident(name) = &self.tokens[self.cursor].kind else {
            return Err(self.error("expected a pattern name"));
        };
        self.cursor += 1;
        Ok(name.clone())
    }

    fn pattern(&mut self, qualified: bool) -> Result<Pattern, Diagnostic> {
        if self.nodes == 128 {
            return Err(self.error("pattern complexity limit exceeded (128)"));
        }
        self.nodes += 1;
        let start = self.current().offset;
        let name = self.name()?;
        let kind = if self.consume(Kind::ColonColon) {
            let variant = self.name()?;
            let payload = if self.consume(Kind::LParen) {
                let mut fields = vec![self.pattern(false)?];
                while self.consume(Kind::Comma) {
                    fields.push(self.pattern(false)?);
                }
                if !self.consume(Kind::RParen) {
                    return Err(self.error("expected ',' or ')' after payload pattern"));
                }
                Some(fields)
            } else {
                None
            };
            PatternKind::Variant { enum_name: name, variant, payload }
        } else if qualified {
            return Err(self.error("expected '::' after enum name"));
        } else {
            PatternKind::Binding(name)
        };
        Ok(Pattern { kind, range: start..self.tokens[self.cursor - 1].end })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nested_variant_payloads_preserve_binding_order_and_ranges() {
        let source = "Outer::Item(first, Inner::Pair(second, third), last)";
        let tree = parse_variant_pattern("case.ae", source).unwrap();
        assert_eq!(tree.range, 0..source.len());
        let PatternKind::Variant { payload: Some(fields), .. } = tree.kind else {
            panic!("expected outer payload");
        };
        let PatternKind::Variant { payload: Some(inner), .. } = &fields[1].kind else {
            panic!("expected nested payload");
        };
        for (field, name) in [
            (&fields[0], "first"), (&inner[0], "second"), (&inner[1], "third"), (&fields[2], "last"),
        ] {
            assert_eq!(field.kind, PatternKind::Binding(name.into()));
            assert_eq!(&source[field.range.clone()], name);
        }
    }

    #[test]
    fn missing_nested_binding_points_at_the_closing_parenthesis() {
        let source = "Outer::Item(Inner::Pair(first, ))";
        let error = parse_variant_pattern("case.ae", source).unwrap_err();
        let span = error.primary.unwrap().span;
        let start = source.find(')').unwrap();
        assert_eq!(span.range, start..start + 1);
    }
}
