//! Module assembly: one source file into declarations plus per-function
//! statement trees, all ranges absolute to the source.

use ae_span::Diagnostic;
use ae_syntax::declaration::{parse_declaration_tokens, Declaration, DeclarationKind};
use ae_syntax::statement::{parse_statements_until_close, Statement};
use ae_syntax::{Kind, Token};

/// One function unit: a top-level `fn` or a method of an impl body. The
/// declaration keeps parameters, return spelling and body range; `body`
/// holds the statement tree when the function has one.
#[derive(Debug, PartialEq)]
pub struct FnUnit {
    pub declaration: Declaration,
    /// Index into `Module::declarations` when this is an impl method.
    pub impl_owner: Option<usize>,
    /// `None` for a trait method signature or an `extern "C"` declaration
    /// without a body (the grammar never opens those bodies).
    pub body: Option<Vec<Statement>>,
}

#[derive(Debug, PartialEq)]
pub struct Module {
    pub file: String,
    pub source: String,
    pub use_paths: Vec<String>,
    /// Top-level declarations in source order. Trait and impl bodies are
    /// opaque ranges here; impl methods are expanded into `functions`.
    pub declarations: Vec<Declaration>,
    /// Top-level functions and impl methods, in source order.
    pub functions: Vec<FnUnit>,
}

/// Parse one source file into the resolver's module form. Every range in
/// the result is absolute to `source`.
pub fn parse_module(file: &str, source: &str) -> Result<Module, Diagnostic> {
    let tokens = ae_syntax::lex_diagnostic(source)?;
    let headers = ae_syntax::module::parse_use_headers(file, source, &tokens)?;
    let declarations = parse_declaration_tokens(file, source, &tokens, headers.body_start)?;
    let mut functions = Vec::new();
    for index in 0..declarations.len() {
        let declaration = &declarations[index];
        match &declaration.kind {
            DeclarationKind::Function { .. } => {
                let body = body_statements(file, source, &tokens, declaration)?;
                functions.push(FnUnit { declaration: declaration.clone(), impl_owner: None, body });
            }
            DeclarationKind::Impl { .. } => {
                for method in impl_methods(file, source, &tokens, declaration)? {
                    let body = body_statements(file, source, &tokens, &method)?;
                    functions.push(FnUnit { declaration: method, impl_owner: Some(index), body });
                }
            }
            // Struct bodies are fields, enum bodies are variants: both are
            // on the declaration already. Trait bodies stay opaque; their
            // signatures are generic-dispatch surface for the type checker.
            DeclarationKind::Struct { .. } | DeclarationKind::Enum { .. } | DeclarationKind::Trait => {}
        }
    }
    Ok(Module {
        file: file.to_owned(),
        source: source.to_owned(),
        use_paths: headers.paths,
        declarations,
        functions,
    })
}

/// Parse a function body whose declaration carries `body` covering
/// `{ ... }` (`opaque_body` starts the range at the `{`).
fn body_statements(file: &str, source: &str, tokens: &[Token], declaration: &Declaration) -> Result<Option<Vec<Statement>>, Diagnostic> {
    let Some(body) = declaration.body.clone() else {
        return Ok(None);
    };
    let open = tokens.iter().position(|token| token.offset == body.start)
        .expect("the body range starts at the opening brace token");
    let (statements, close) = parse_statements_until_close(file, source, tokens, open + 1)?;
    debug_assert!(tokens[close].offset < body.end, "the body parser stops inside its range");
    Ok(Some(statements))
}

/// Expand an impl body into method declarations. The grammar keeps impl
/// bodies opaque, but a body is exactly a sequence of top-level-shaped `fn`
/// declarations, so the body's token span is re-parsed with the same
/// declaration parser: the tokens' offsets are absolute, so every method
/// range lands where the source has it.
fn impl_methods(file: &str, source: &str, tokens: &[Token], declaration: &Declaration) -> Result<Vec<Declaration>, Diagnostic> {
    let Some(body) = declaration.body.clone() else {
        return Ok(Vec::new());
    };
    let mut inner: Vec<Token> = tokens.iter()
        .filter(|token| token.offset > body.start && token.end < body.end)
        .cloned()
        .collect();
    if inner.is_empty() {
        return Ok(Vec::new());
    }
    inner.push(eof_sentinel(source));
    parse_declaration_tokens(file, source, &inner, 0)
}

/// The declaration parser loops until `Eof`; a body slice has none, so a
/// sentinel at the source end closes the stream.
fn eof_sentinel(source: &str) -> Token {
    Token { kind: Kind::Eof, offset: source.len(), end: source.len(), line: 0, column: 0 }
}
