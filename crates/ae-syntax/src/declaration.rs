use crate::{Kind, Token};

/// Find the token immediately after the brace body whose opening brace has
/// already been consumed. On failure, return the position of the end token.
pub fn brace_body_end(tokens: &[Token], start: usize) -> Result<usize, usize> {
    let mut depth = 1;
    for (index, token) in tokens.iter().enumerate().skip(start) {
        match token.kind {
            Kind::LBrace => depth += 1,
            Kind::RBrace => {
                depth -= 1;
                if depth == 0 {
                    return Ok(index + 1);
                }
            }
            Kind::Eof => return Err(index),
            _ => {}
        }
    }
    Err(tokens.len())
}

use std::ops::Range;

use ae_span::Diagnostic;

#[derive(Debug, PartialEq)]
pub struct TypedName {
    pub name: String,
    pub ty: Range<usize>,
}

#[derive(Debug, PartialEq)]
pub struct TypeParameter {
    pub name: String,
    pub bounds: Vec<String>,
}

#[derive(Debug, PartialEq)]
pub enum DeclarationKind {
    Function { parameters: Vec<TypedName>, returns: Option<Range<usize>> },
    Struct { fields: Vec<TypedName> },
}

#[derive(Debug, PartialEq)]
pub struct Declaration {
    pub name: String,
    pub type_parameters: Vec<TypeParameter>,
    pub kind: DeclarationKind,
    pub range: Range<usize>,
    pub body: Range<usize>,
}

/// Read function and struct declaration boundaries in a source fragment.
/// Type spellings and function bodies are retained as source ranges.
pub fn parse_declarations(file: &str, source: &str) -> Result<Vec<Declaration>, Diagnostic> {
    let tokens = crate::lex_diagnostic(source).map_err(|mut error| {
        if let Some(label) = &mut error.primary {
            label.span.file = Some(file.to_owned());
        }
        error
    })?;
    let mut parser = DeclarationParser { file, source, tokens: &tokens, pos: 0 };
    let mut declarations = Vec::new();
    while parser.current().kind != Kind::Eof {
        declarations.push(parser.declaration()?);
    }
    Ok(declarations)
}

struct DeclarationParser<'a> {
    file: &'a str,
    source: &'a str,
    tokens: &'a [Token],
    pos: usize,
}

impl DeclarationParser<'_> {
    fn current(&self) -> &Token {
        &self.tokens[self.pos]
    }

    fn error(&self, message: &str) -> Diagnostic {
        let token = self.current();
        crate::diagnostic_range(self.file, self.source, token.offset..token.end, message)
    }

    fn consume(&mut self, kind: Kind) -> bool {
        if self.current().kind != kind {
            return false;
        }
        self.pos += 1;
        true
    }

    fn name(&mut self) -> Result<String, Diagnostic> {
        let Kind::Ident(name) = &self.tokens[self.pos].kind else {
            return Err(self.error("expected a declaration name"));
        };
        self.pos += 1;
        Ok(name.clone())
    }

    fn ty(&mut self, stops: &[Kind], fields: bool) -> Result<Range<usize>, Diagnostic> {
        let start = self.pos;
        let mut closing = Vec::new();
        loop {
            let kind = &self.current().kind;
            if closing.is_empty() && (stops.contains(kind)
                || (fields && self.pos > start && self.at_field_name())) {
                break;
            }
            match kind {
                Kind::LParen => closing.push(Kind::RParen),
                Kind::LBracket => closing.push(Kind::RBracket),
                Kind::RParen | Kind::RBracket if closing.last() == Some(kind) => {
                    closing.pop();
                }
                Kind::Fn if matches!(self.tokens.get(self.pos + 1).map(|token| &token.kind), Some(Kind::LParen)) => {}
                Kind::Ident(_) | Kind::Number(_) | Kind::Amp | Kind::Star
                | Kind::Const | Kind::Mut | Kind::Arrow | Kind::Comma => {}
                _ => return Err(self.error("unexpected token in type spelling")),
            }
            self.pos += 1;
        }
        if self.pos == start {
            return Err(self.error("expected a type"));
        }
        Ok(self.tokens[start].offset..self.tokens[self.pos - 1].end)
    }

    fn at_field_name(&self) -> bool {
        matches!(self.current().kind, Kind::Ident(_))
            && matches!(self.tokens.get(self.pos + 1).map(|token| &token.kind), Some(Kind::Colon))
    }

    fn typed_names(&mut self, close: Kind, fields: bool) -> Result<Vec<TypedName>, Diagnostic> {
        let mut names = Vec::new();
        while self.current().kind != close {
            if !fields {
                self.consume(Kind::Var);
            }
            let name = self.name()?;
            if !self.consume(Kind::Colon) {
                return Err(self.error("expected ':' after name"));
            }
            let ty = self.ty(&[Kind::Comma, close.clone()], fields)?;
            names.push(TypedName { name, ty });
            if !self.consume(Kind::Comma) && !(fields && self.at_field_name()) {
                break;
            }
            if !fields && self.current().kind == close {
                return Err(self.error("expected a parameter after ','"));
            }
        }
        Ok(names)
    }

    fn declaration(&mut self) -> Result<Declaration, Diagnostic> {
        let start = self.current().offset;
        if matches!(&self.current().kind, Kind::Ident(name) if name == "pub") {
            self.pos += 1;
        }
        let function = if self.consume(Kind::Fn) {
            true
        } else if self.consume(Kind::Struct) {
            false
        } else {
            return Err(self.error("expected a function or struct declaration"));
        };
        let name = self.name()?;
        let mut type_parameters = Vec::new();
        if self.consume(Kind::LBracket) {
            loop {
                let name = self.name()?;
                let mut bounds = Vec::new();
                if self.consume(Kind::Colon) {
                    loop {
                        bounds.push(self.name()?);
                        if !self.consume(Kind::Plus) {
                            break;
                        }
                    }
                }
                type_parameters.push(TypeParameter { name, bounds });
                if !self.consume(Kind::Comma) {
                    break;
                }
            }
            if !self.consume(Kind::RBracket) {
                return Err(self.error("expected ']' after type parameters"));
            }
        }

        let (kind, body_start) = if function {
            if !self.consume(Kind::LParen) {
                return Err(self.error("expected '(' before parameters"));
            }
            let parameters = self.typed_names(Kind::RParen, false)?;
            if !self.consume(Kind::RParen) {
                return Err(self.error("expected ')' after parameters"));
            }
            let returns = if self.consume(Kind::Arrow) { Some(self.ty(&[Kind::LBrace], false)?) } else { None };
            let body_start = self.current().offset;
            if !self.consume(Kind::LBrace) {
                return Err(self.error("expected a function body"));
            }
            self.pos = match brace_body_end(self.tokens, self.pos) {
                Ok(end) => end,
                Err(end) => {
                    self.pos = end;
                    return Err(self.error("expected '}' before end of file"));
                }
            };
            (DeclarationKind::Function { parameters, returns }, body_start)
        } else {
            let body_start = self.current().offset;
            if !self.consume(Kind::LBrace) {
                return Err(self.error("expected a struct body"));
            }
            let fields = self.typed_names(Kind::RBrace, true)?;
            if !self.consume(Kind::RBrace) {
                return Err(self.error("expected '}' after fields"));
            }
            (DeclarationKind::Struct { fields }, body_start)
        };
        let end = self.tokens[self.pos - 1].end;
        Ok(Declaration { name, type_parameters, kind, range: start..end, body: body_start..end })
    }
}
