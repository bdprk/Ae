use std::ops::Range;

use ae_span::Diagnostic;

use crate::literal::parse_literal;
use crate::operator::{parse_infix_tail, prefix_binding_power};
use crate::{diagnostic_range, Kind, Token};

#[derive(Debug, PartialEq)]
pub struct Expression {
    pub kind: ExpressionKind,
    pub range: Range<usize>,
}

#[derive(Debug, PartialEq)]
pub struct FieldInitializer {
    pub name: String,
    pub name_range: Range<usize>,
    pub value: Expression,
}

#[derive(Debug, PartialEq)]
pub enum ExpressionKind {
    Atom(Kind),
    Group(Box<Expression>),
    Propagate(Box<Expression>),
    Array(Vec<Expression>),
    Vector(Vec<Expression>),
    Struct { target: Box<Expression>, fields: Vec<FieldInitializer> },
    Call { callee: Box<Expression>, arguments: Vec<Expression> },
    Index { base: Box<Expression>, index: Box<Expression> },
    Field { base: Box<Expression>, name: String },
    Specialization { base: Box<Expression>, arguments: Vec<Range<usize>> },
    Qualified { base: Box<Expression>, name: String },
    Prefix { operator: Kind, operand: Box<Expression> },
    Binary { left: Box<Expression>, operator: Kind, right: Box<Expression> },
}

/// Parse operator expressions with calls, indexing, and field access.
/// Collection initializers retain their element syntax. The compiler checks
/// name resolution, array placement, and element types.
pub fn parse_operator_expression(file: &str, source: &str) -> Result<Expression, Diagnostic> {
    let tokens = crate::lex_diagnostic(source).map_err(|mut diagnostic| {
        if let Some(label) = &mut diagnostic.primary {
            label.span.file = Some(file.to_owned());
        }
        diagnostic
    })?;
    let mut parser = Parser { file, source, tokens: &tokens, cursor: 0, nodes: 0 };
    let mut expression = parser.expression(0)?;
    // The statement grammar permits a single propagation marker after the
    // entire initializer or discarded expression.
    if parser.current().kind == Kind::Question {
        let range = expression.range.start..parser.current().end;
        parser.cursor += 1;
        expression = Expression { kind: ExpressionKind::Propagate(Box::new(expression)), range };
    }
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

    fn count_node(&mut self) -> Result<(), Diagnostic> {
        if self.nodes == 128 {
            return Err(self.error("expression complexity limit exceeded (128)"));
        }
        self.nodes += 1;
        Ok(())
    }

    fn expression(&mut self, minimum: u8) -> Result<Expression, Diagnostic> {
        self.count_node()?;
        let token = &self.tokens[self.cursor];
        let mut range = token.offset..token.end;
        let kind = if let Some(power) = prefix_binding_power(&token.kind) {
            self.cursor += 1;
            let operand = self.expression(power)?;
            range.end = operand.range.end;
            ExpressionKind::Prefix { operator: token.kind.clone(), operand: Box::new(operand) }
        } else {
            match &token.kind {
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
                Kind::LBracket => {
                    let elements = self.collection(false)?;
                    range.end = self.tokens[self.cursor - 1].end;
                    ExpressionKind::Array(elements)
                }
                Kind::Ident(name) if name == "vec"
                    && self.tokens.get(self.cursor + 1).is_some_and(|token| token.kind == Kind::LBracket) =>
                {
                    self.cursor += 1;
                    let elements = self.collection(true)?;
                    range.end = self.tokens[self.cursor - 1].end;
                    ExpressionKind::Vector(elements)
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
            }
        };
        let left = self.postfix(Expression { kind, range })?;
        parse_infix_tail(self, left, minimum, |parser| &parser.current().kind,
            |parser, left, operator, right_power| {
                parser.cursor += 1;
                let right = parser.expression(right_power)?;
                Ok(Expression {
                    range: left.range.start..right.range.end,
                    kind: ExpressionKind::Binary { left: Box::new(left), operator, right: Box::new(right) },
                })
            })
    }
    fn collection(&mut self, allow_empty: bool) -> Result<Vec<Expression>, Diagnostic> {
        self.cursor += 1;
        let mut elements = Vec::new();
        if self.current().kind == Kind::RBracket {
            if !allow_empty {
                return Err(self.error("array must contain at least one element"));
            }
        } else {
            loop {
                elements.push(self.expression(0)?);
                if self.current().kind != Kind::Comma {
                    break;
                }
                self.cursor += 1;
            }
        }
        if self.current().kind != Kind::RBracket {
            return Err(self.error("expected closing bracket after collection elements"));
        }
        self.cursor += 1;
        Ok(elements)
    }

    fn index(&mut self, base: Expression) -> Result<ExpressionKind, Diagnostic> {
        let index = self.expression(0)?;
        if self.current().kind != Kind::RBracket {
            return Err(self.error("expected closing bracket"));
        }
        self.cursor += 1;
        Ok(ExpressionKind::Index { base: Box::new(base), index: Box::new(index) })
    }

    fn struct_fields(&mut self) -> Result<Vec<FieldInitializer>, Diagnostic> {
        let mut fields = Vec::new();
        while self.current().kind != Kind::RBrace {
            let token = self.current();
            let Kind::Ident(name) = &token.kind else {
                return Err(self.error("expected a struct field name"));
            };
            let name = name.clone();
            let name_range = token.offset..token.end;
            self.cursor += 1;
            if self.current().kind != Kind::Colon {
                return Err(self.error("expected ':' after struct field name"));
            }
            self.cursor += 1;
            let value = self.expression(0)?;
            fields.push(FieldInitializer { name, name_range, value });
            if self.current().kind == Kind::Comma {
                self.cursor += 1;
            }
        }
        self.cursor += 1;
        Ok(fields)
    }

    fn postfix(&mut self, mut base: Expression) -> Result<Expression, Diagnostic> {
        while matches!(self.current().kind, Kind::LParen | Kind::LBracket | Kind::Dot | Kind::ColonColon)
            || (self.current().kind == Kind::LBrace
                && matches!(base.kind, ExpressionKind::Atom(Kind::Ident(_)) | ExpressionKind::Specialization { .. }))
        {
            self.count_node()?;
            let start = base.range.start;
            let opener = &self.tokens[self.cursor].kind;
            self.cursor += 1;
            let kind = match opener {
                Kind::LParen => {
                    let mut arguments = Vec::new();
                    if self.current().kind != Kind::RParen {
                        loop {
                            arguments.push(self.expression(0)?);
                            if self.current().kind != Kind::Comma {
                                break;
                            }
                            self.cursor += 1;
                        }
                    }
                    if self.current().kind != Kind::RParen {
                        return Err(self.error("expected ',' or ')' after argument"));
                    }
                    self.cursor += 1;
                    ExpressionKind::Call { callee: Box::new(base), arguments }
                }
                Kind::LBracket if matches!(&base.kind, ExpressionKind::Atom(Kind::Ident(_))) => {
                    let start = self.cursor - 1;
                    let end = crate::declaration::skip_type_parameter_list(self.tokens, start);
                    if self.tokens.get(end).is_some_and(|token| matches!(token.kind, Kind::ColonColon | Kind::LBrace)) {
                        let arguments = crate::declaration::expression_type_arguments(
                            self.file, self.source, self.tokens, start,
                        )?;
                        self.cursor = end;
                        ExpressionKind::Specialization { base: Box::new(base), arguments }
                    } else {
                        self.index(base)?
                    }
                }
                Kind::LBracket => self.index(base)?,
                Kind::LBrace => {
                    let fields = self.struct_fields()?;
                    ExpressionKind::Struct { target: Box::new(base), fields }
                }
                Kind::Dot | Kind::ColonColon => {
                    let Kind::Ident(name) = &self.current().kind else {
                        let message = if *opener == Kind::Dot {
                            "expected a field or method name"
                        } else {
                            "expected an associated name after '::'"
                        };
                        return Err(self.error(message));
                    };
                    let name = name.clone();
                    self.cursor += 1;
                    if *opener == Kind::Dot {
                        ExpressionKind::Field { base: Box::new(base), name }
                    } else {
                        ExpressionKind::Qualified { base: Box::new(base), name }
                    }
                }
                _ => unreachable!("postfix opener"),
            };
            base = Expression { kind, range: start..self.tokens[self.cursor - 1].end };
        }
        Ok(base)
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
    fn arrays_and_vectors_keep_element_order_and_ranges() {
        let source = "[1 + 2, item]";
        let array = parse_operator_expression("case.ae", source).unwrap();
        let ExpressionKind::Array(elements) = array.kind else {
            panic!("expected array");
        };
        assert_eq!(&source[elements[0].range.clone()], "1 + 2");
        assert_eq!(&source[elements[1].range.clone()], "item");

        let source = "vec[1.5, -2.5]";
        let vector = parse_operator_expression("case.ae", source).unwrap();
        assert_eq!(vector.range, 0..source.len());
        assert!(matches!(vector.kind, ExpressionKind::Vector(elements) if elements.len() == 2));
        assert!(matches!(parse_operator_expression("case.ae", "vec[]").unwrap().kind,
            ExpressionKind::Vector(elements) if elements.is_empty()));
        let error = parse_operator_expression("case.ae", "[]").unwrap_err();
        assert_eq!(error.primary.unwrap().span.range, 1..2);
    }

    #[test]
    fn propagation_wraps_the_whole_statement_operand() {
        let source = "  maybe_find(flag)?";
        let expression = parse_operator_expression("case.ae", source).unwrap();
        assert_eq!(expression.range, 2..source.len());
        let ExpressionKind::Propagate(operand) = expression.kind else {
            panic!("expected a propagation marker");
        };
        assert!(matches!(operand.kind, ExpressionKind::Call { .. }));
        assert_eq!(&source[operand.range], "maybe_find(flag)");

        let error = parse_operator_expression("case.ae", "consume(maybe_find(flag)?)").unwrap_err();
        assert_eq!(error.primary.unwrap().span.range, 24..25);
        assert!(parse_operator_expression("case.ae", "maybe_find(flag)??").is_err());
    }

    #[test]
    fn struct_initializers_preserve_field_order_and_nested_ranges() {
        let source = "Pair[i64, i64] { right: Point { x: 2 }, left: 1 + 3, }";
        let expression = parse_operator_expression("case.ae", source).unwrap();
        assert_eq!(expression.range, 0..source.len());
        let ExpressionKind::Struct { target, fields } = expression.kind else {
            panic!("expected struct literal");
        };
        assert!(matches!(target.kind, ExpressionKind::Specialization { .. }));
        assert_eq!(&source[fields[0].name_range.clone()], "right");
        assert_eq!(&source[fields[0].value.range.clone()], "Point { x: 2 }");
        assert_eq!(fields[1].name, "left");
        assert_eq!(&source[fields[1].value.range.clone()], "1 + 3");

        let error = parse_operator_expression("case.ae", "Point { x 2 }").unwrap_err();
        assert_eq!(error.primary.unwrap().span.range, 10..11);
    }

    #[test]
    fn qualified_specializations_preserve_nested_type_arguments() {
        let source = "Outer[Inner[i64, string], bool]::Value";
        let parsed = parse_operator_expression("case.ae", source).unwrap();
        let ExpressionKind::Qualified { base, .. } = parsed.kind else {
            panic!("expected qualified name");
        };
        let ExpressionKind::Specialization { arguments, .. } = base.kind else {
            panic!("expected specialization");
        };
        let arguments: Vec<_> = arguments.into_iter().map(|range| &source[range]).collect();
        assert_eq!(arguments, ["Inner[i64, string]", "bool"]);
        assert!(matches!(parse_operator_expression("case.ae", "items[i]").unwrap().kind,
            ExpressionKind::Index { .. }));
        assert!(parse_operator_expression("case.ae", "Outer[]::Value").is_err());
    }

    #[test]
    fn qualified_names_remain_distinct_from_field_access() {
        let source = "Result::Ok(value).field";
        let expression = parse_operator_expression("case.ae", source).unwrap();
        let ExpressionKind::Field { base, name } = expression.kind else {
            panic!("expected field access");
        };
        assert_eq!(name, "field");
        let ExpressionKind::Call { callee, arguments } = base.kind else {
            panic!("expected constructor call");
        };
        assert_eq!(arguments.len(), 1);
        let ExpressionKind::Qualified { base, name } = callee.kind else {
            panic!("expected qualified name");
        };
        assert_eq!(name, "Ok");
        assert_eq!(&source[base.range], "Result");

        let error = parse_operator_expression("case.ae", "Result::(value)").unwrap_err();
        assert_eq!(error.primary.unwrap().span.range, 8..9);
    }

    #[test]
    fn postfix_binds_inside_prefix_and_before_multiplication() {
        let source = "-combine(values[0], point.x) * 3";
        let expression = parse_operator_expression("main.ae", source).unwrap();
        assert_eq!(expression.range, 0..source.len());
        let ExpressionKind::Binary { left, operator: Kind::Star, .. } = expression.kind else {
            panic!("expected multiplication");
        };
        let ExpressionKind::Prefix { operator: Kind::Minus, operand } = left.kind else {
            panic!("expected negation");
        };
        let ExpressionKind::Call { callee, arguments } = operand.kind else {
            panic!("expected call");
        };
        assert_eq!(callee.kind, ExpressionKind::Atom(Kind::Ident("combine".into())));
        assert_eq!(arguments.len(), 2);
        assert!(matches!(&arguments[0].kind, ExpressionKind::Index { .. }));
        assert!(matches!(&arguments[1].kind, ExpressionKind::Field { name, .. } if name == "x"));
        assert_eq!(&source[arguments[0].range.clone()], "values[0]");
        assert_eq!(&source[arguments[1].range.clone()], "point.x");

        let error = parse_operator_expression("main.ae", "combine(values[0)").unwrap_err();
        assert_eq!(error.primary.unwrap().span.range, 16..17);
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
