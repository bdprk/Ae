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
pub struct EnumVariant {
    pub name: String,
    pub payload: Vec<Range<usize>>,
    pub range: Range<usize>,
}

#[derive(Debug, PartialEq)]
pub enum DeclarationKind {
    Function {
        parameters: Vec<TypedName>,
        returns: Option<Range<usize>>,
        unsafe_fn: bool,
        abi: Option<String>,
    },
    Struct { fields: Vec<TypedName> },
    Enum { variants: Vec<EnumVariant> },
}

#[derive(Debug, PartialEq)]
pub struct Declaration {
    pub name: String,
    pub type_parameters: Vec<TypeParameter>,
    pub kind: DeclarationKind,
    pub range: Range<usize>,
    pub body: Option<Range<usize>>,
}

/// Read function, struct, and enum declaration boundaries in a source fragment.
/// Type spellings and function bodies are retained as source ranges.
pub fn parse_declarations(file: &str, source: &str) -> Result<Vec<Declaration>, Diagnostic> {
    let tokens = declaration_tokens(file, source)?;
    let imports = crate::module::parse_use_headers(file, source, &tokens)?;
    let mut parser = DeclarationParser { file, source, tokens: &tokens, pos: imports.len() * 3 };
    let mut declarations = Vec::new();
    while parser.current().kind != Kind::Eof {
        declarations.push(parser.declaration()?);
    }
    Ok(declarations)
}

#[derive(Debug)]
pub struct RecoveredDeclarations {
    pub declarations: Vec<Declaration>,
    pub diagnostics: Vec<Diagnostic>,
}

/// Continue at the next top-level declaration after a syntax error.
/// Recovery is bounded to 32 diagnostics and does not enter brace bodies.
pub fn recover_declarations(file: &str, source: &str) -> RecoveredDeclarations {
    const MAX_DIAGNOSTICS: usize = 32;
    let mut result = RecoveredDeclarations { declarations: Vec::new(), diagnostics: Vec::new() };
    let tokens = match declaration_tokens(file, source) {
        Ok(tokens) => tokens,
        Err(error) => {
            result.diagnostics.push(error);
            return result;
        }
    };
    let imports = match crate::module::parse_use_headers(file, source, &tokens) {
        Ok(imports) => imports,
        Err(error) => {
            result.diagnostics.push(error);
            return result;
        }
    };
    let mut parser = DeclarationParser { file, source, tokens: &tokens, pos: imports.len() * 3 };
    while parser.current().kind != Kind::Eof {
        let start = parser.pos;
        match parser.declaration() {
            Ok(declaration) => result.declarations.push(declaration),
            Err(error) => {
                result.diagnostics.push(error);
                if result.diagnostics.len() == MAX_DIAGNOSTICS {
                    break;
                }
                parser.synchronize(start);
            }
        }
    }
    result
}

fn declaration_tokens(file: &str, source: &str) -> Result<Vec<Token>, Diagnostic> {
    crate::lex_diagnostic(source).map_err(|mut error| {
        if let Some(label) = &mut error.primary {
            label.span.file = Some(file.to_owned());
        }
        error
    })
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

    fn synchronize(&mut self, start: usize) {
        // A failed declaration may already have consumed a brace opener.
        // Restart at its first token to skip complete bodies consistently.
        let mut index = start;
        let failed_keyword = start + usize::from(matches!(
            &self.tokens[start].kind, Kind::Ident(name) if name == "pub"
        ));
        loop {
            let token = &self.tokens[index];
            match &token.kind {
                Kind::Eof => break,
                Kind::LBrace => {
                    index = brace_body_end(self.tokens, index + 1).unwrap_or_else(|end| end);
                    continue;
                }
                Kind::Struct | Kind::Enum | Kind::Fn | Kind::Unsafe | Kind::Extern if index > failed_keyword && index >= self.pos => {
                    let next = self.tokens.get(index + 1).map(|next| &next.kind);
                    let starts_declaration = match token.kind {
                        Kind::Fn => matches!(next, Some(Kind::Ident(_))),
                        Kind::Unsafe => matches!(next, Some(Kind::Fn)),
                        Kind::Extern => matches!(next, Some(Kind::Text(_))),
                        _ => true,
                    };
                    if starts_declaration {
                        if index > start && matches!(&self.tokens[index - 1].kind, Kind::Ident(name) if name == "pub") {
                            index -= 1;
                        }
                        break;
                    }
                }
                _ => {}
            }
            index += 1;
        }
        self.pos = index;
    }

    fn enum_variants(&mut self) -> Result<Vec<EnumVariant>, Diagnostic> {
        let mut variants = Vec::new();
        while !self.consume(Kind::RBrace) {
            let start = self.current().offset;
            let name = self.name()?;
            let mut payload = Vec::new();
            if self.consume(Kind::LParen) && !self.consume(Kind::RParen) {
                loop {
                    payload.push(self.ty(&[Kind::Comma, Kind::RParen], false)?);
                    if !self.consume(Kind::Comma) {
                        break;
                    }
                }
                if !self.consume(Kind::RParen) {
                    return Err(self.error("expected ')' after enum payload"));
                }
            }
            variants.push(EnumVariant { name, payload, range: start..self.tokens[self.pos - 1].end });
            self.consume(Kind::Comma);
        }
        Ok(variants)
    }

    fn declaration(&mut self) -> Result<Declaration, Diagnostic> {
        let start = self.current().offset;
        if matches!(&self.current().kind, Kind::Ident(name) if name == "pub") {
            self.pos += 1;
        }
        let unsafe_fn = self.consume(Kind::Unsafe);
        let abi = if !unsafe_fn && self.consume(Kind::Extern) {
            let Kind::Text(abi) = &self.tokens[self.pos].kind else {
                return Err(self.error("expected an ABI string after 'extern'"));
            };
            self.pos += 1;
            Some(abi.clone())
        } else {
            None
        };
        if (unsafe_fn || abi.is_some()) && self.current().kind != Kind::Fn {
            return Err(self.error("expected 'fn' after function qualifier"));
        }
        let keyword = match self.current().kind {
            Kind::Fn => Kind::Fn,
            Kind::Struct => Kind::Struct,
            Kind::Enum => Kind::Enum,
            _ => return Err(self.error("expected a function, struct, or enum declaration")),
        };
        self.pos += 1;
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

        let (kind, body_start) = if keyword == Kind::Fn {
            if !self.consume(Kind::LParen) {
                return Err(self.error("expected '(' before parameters"));
            }
            let parameters = self.typed_names(Kind::RParen, false)?;
            if !self.consume(Kind::RParen) {
                return Err(self.error("expected ')' after parameters"));
            }
            let foreign = abi.is_some();
            let stops = if foreign { &[Kind::LBrace, Kind::Semi][..] } else { &[Kind::LBrace][..] };
            let returns = if self.consume(Kind::Arrow) {
                Some(self.ty(stops, false)?)
            } else {
                None
            };
            let function = DeclarationKind::Function { parameters, returns, unsafe_fn, abi };
            if foreign && self.consume(Kind::Semi) {
                let end = self.tokens[self.pos - 1].end;
                return Ok(Declaration { name, type_parameters, kind: function, range: start..end, body: None });
            }
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
            (function, body_start)
        } else if keyword == Kind::Enum {
            let body_start = self.current().offset;
            if !self.consume(Kind::LBrace) {
                return Err(self.error("expected an enum body"));
            }
            let variants = self.enum_variants()?;
            (DeclarationKind::Enum { variants }, body_start)
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
        Ok(Declaration { name, type_parameters, kind, range: start..end, body: Some(body_start..end) })
    }
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_foreign_prototypes_and_unsafe_definitions() {
        let source = "extern \"C\" fn foreign(value: i64) -> i64; unsafe fn local() -> i64 { return 1; }";
        let declarations = parse_declarations("case.ae", source).unwrap();
        assert!(matches!(&declarations[0].kind,
            DeclarationKind::Function { abi: Some(abi), unsafe_fn: false, .. } if abi == "C"));
        assert_eq!(declarations[0].body, None);
        assert_eq!(&source[declarations[0].range.clone()], "extern \"C\" fn foreign(value: i64) -> i64;");
        assert!(matches!(&declarations[1].kind,
            DeclarationKind::Function { abi: None, unsafe_fn: true, .. }));
        assert_eq!(&source[declarations[1].body.clone().unwrap()], "{ return 1; }");
        assert!(parse_declarations("case.ae", include_str!("../../../tests/run/extern_c_call.ae")).is_ok());
        assert!(parse_declarations("case.ae", include_str!("../../../tests/run/unsafe_fn_call.ae")).is_ok());
    }

    #[test]
    fn enum_payload_spellings_and_variant_boundaries_are_preserved() {
        let source = "pub enum Pair[T] { Empty Value(T, vec[string]), } fn main() {}";
        let declarations = parse_declarations("case.ae", source).unwrap();
        let DeclarationKind::Enum { variants } = &declarations[0].kind else {
            panic!("expected enum declaration");
        };
        assert_eq!(variants.len(), 2);
        assert_eq!(&source[variants[0].range.clone()], "Empty");
        assert_eq!(&source[variants[1].range.clone()], "Value(T, vec[string])");
        assert_eq!(&source[variants[1].payload[0].clone()], "T");
        assert_eq!(&source[variants[1].payload[1].clone()], "vec[string]");
        assert_eq!(declarations[1].name, "main");
        assert!(parse_declarations("case.ae", include_str!("../../../examples/enums.ae")).is_ok());
    }

    #[test]
    fn missing_parameter_delimiter_recovers_at_the_following_declaration() {
        let source = "fn broken(value: i64 { print(1); }\npub struct Next[T] { value: T }\nfn main() {}";
        let recovered = recover_declarations("case.ae", source);
        assert_eq!(recovered.diagnostics.len(), 1);
        let diagnostic = recovered.diagnostics.first().unwrap();
        assert_eq!(parse_declarations("case.ae", source).unwrap_err(), *diagnostic);
        let offset = source.find('{').unwrap();
        assert_eq!(diagnostic.primary.as_ref().unwrap().span.range, offset..offset + 1);
        assert_eq!(recovered.declarations.len(), 2);
        assert_eq!(recovered.declarations[0].name, "Next");
        assert_eq!(recovered.declarations[0].range.start, source.find("pub struct").unwrap());
        assert_eq!(recovered.declarations[1].name, "main");
    }

    #[test]
    fn reads_declarations_after_headers_in_the_module_example() {
        let source = include_str!("../../../examples/modules/main.ae");
        let declarations = parse_declarations("main.ae", source).unwrap();
        assert_eq!(declarations.len(), 1);
        assert_eq!(declarations[0].name, "main");
        assert_eq!(declarations[0].range.start, source.find("fn main").unwrap());
        let recovered = recover_declarations("main.ae", source);
        assert!(recovered.diagnostics.is_empty());
        assert_eq!(recovered.declarations, declarations);
    }

    #[test]
    fn recovery_caps_repeated_declaration_errors() {
        let source = format!("{}fn main() {{}}", "fn broken( { }\n".repeat(40));
        let recovered = recover_declarations("case.ae", &source);
        assert_eq!(recovered.diagnostics.len(), 32);
        assert!(recovered.declarations.is_empty());
        assert!(recovered.diagnostics.windows(2).all(|pair| {
            pair[0].primary.as_ref().unwrap().span.range.start
                < pair[1].primary.as_ref().unwrap().span.range.start
        }));
    }
}
