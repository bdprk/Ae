use std::ops::Range;

use ae_span::Diagnostic;

use crate::literal::parse_literal;
use crate::operator::{parse_infix_tail, PREFIX_BINDING_POWER};
use crate::{diagnostic_range, Kind, Token};

#[derive(Debug, PartialEq)]
pub struct Expression {
    pub kind: ExpressionKind,
    pub range: Range<usize>,
}

#[derive(Debug, PartialEq)]
pub enum ExpressionKind {
    Atom(Kind),
    Group(Box<Expression>),
    Prefix { operator: Kind, operand: Box<Expression> },
    Binary { left: Box<Expression>, operator: Kind, right: Box<Expression> },
}

/// Parse the operator-expression grammar over literals, names, and groups.
pub fn parse_operator_expression(file: &str, source: &str) -> Result<Expression, Diagnostic> {
    let tokens = crate::lex_diagnostic(source).map_err(|mut diagnostic| {
        if let Some(label) = &mut diagnostic.primary {
            label.span.file = Some(file.to_owned());
        }
        diagnostic
    })?;
    let mut parser = Parser { file, source, tokens: &tokens, cursor: 0, nodes: 0 };
    let expression = parser.expression(0)?;
    if parser.current().kind != Kind::Eof {
        return Err(parser.error("expected end of expression"));
    }
    Ok(expression)
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

    fn expression(&mut self, minimum: u8) -> Result<Expression, Diagnostic> {
        if self.nodes == 128 {
            return Err(self.error("expression complexity limit exceeded (128)"));
        }
        self.nodes += 1;
        let token = &self.tokens[self.cursor];
        let mut range = token.offset..token.end;
        let kind = match &token.kind {
            Kind::Minus | Kind::Bang => {
                self.cursor += 1;
                let operand = self.expression(PREFIX_BINDING_POWER)?;
                range.end = operand.range.end;
                ExpressionKind::Prefix { operator: token.kind.clone(), operand: Box::new(operand) }
            }
            Kind::LParen => {
                self.cursor += 1;
                let inner = self.expression(0)?;
                if self.current().kind != Kind::RParen {
                    return Err(self.error("expected closing parenthesis"));
                }
                range.end = self.current().end;
                self.cursor += 1;
                ExpressionKind::Group(Box::new(inner))
            }
            Kind::Ident(_) => {
                self.cursor += 1;
                ExpressionKind::Atom(token.kind.clone())
            }
            _ if parse_literal(token).is_some() => {
                self.cursor += 1;
                ExpressionKind::Atom(token.kind.clone())
            }
            _ => return Err(self.error("expected an expression")),
        };
        parse_infix_tail(self, Expression { kind, range }, minimum, |parser| &parser.current().kind,
            |parser, left, operator, right_power| {
                parser.cursor += 1;
                let right = parser.expression(right_power)?;
                Ok(Expression {
                    range: left.range.start..right.range.end,
                    kind: ExpressionKind::Binary { left: Box::new(left), operator, right: Box::new(right) },
                })
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escaped_atoms_and_invalid_literals_preserve_source_ranges() {
        let source = r#" "a\n\"b" "#;
        let expression = parse_operator_expression("literal.ae", source).unwrap();
        assert_eq!(expression.range, 1..source.len() - 1);
        assert_eq!(expression.kind, ExpressionKind::Atom(Kind::Text(r#"a\n\"b"#.into())));

        let error = parse_operator_expression("literal.ae", r#" "a\n"#).unwrap_err();
        assert_eq!(error.code.as_deref(), Some("E0003"));
        let span = error.primary.unwrap().span;
        assert_eq!(span.file.as_deref(), Some("literal.ae"));
        assert_eq!(span.range, 1..2);
    }

    #[test]
    fn malformed_and_deep_expressions_stop_at_a_source_boundary() {
        let error = parse_operator_expression("group.ae", "(2 * )").unwrap_err();
        assert_eq!(error.primary.unwrap().span.range, 5..6);

        let source = format!("{}2{}", "(".repeat(128), ")".repeat(128));
        let error = parse_operator_expression("group.ae", &source).unwrap_err();
        assert!(error.message.contains("complexity limit"));
        assert_eq!(error.primary.unwrap().span.range, 128..129);
    }
}
