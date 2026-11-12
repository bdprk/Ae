//! Parser and single-pass semantic checker: tokens to typed statements.
use super::*;
impl<'a> Parser<'a> {
    pub(super) fn current(&self) -> &Kind { &self.tokens[self.pos].kind }
    pub(super) fn fail(&self, message: &str) -> ae_span::Diagnostic {
        let token = &self.tokens[self.pos];
        ae_syntax::diagnostic_range(&self.file, self.source, token.offset..token.end, message)
    }
    fn error_at(&self, offset: usize, message: &str) -> ae_span::Diagnostic {
        let end = self.tokens.binary_search_by_key(&offset, |token| token.offset)
            .ok().map_or(offset, |index| self.tokens[index].end);
        ae_syntax::diagnostic_range(&self.file, self.source, offset..end, message)
    }
    fn binding_label(&self, diagnostic: ae_span::Diagnostic, id: usize) -> ae_span::Diagnostic {
        let Some(range) = self.binding_ranges.get(&id) else { return diagnostic };
        let source = ae_span::SourceFile { name: Some(&self.file), text: self.source };
        match source.span(range.clone()) {
            Some(span) => diagnostic.with_secondary(span, "binding declared here"),
            None => diagnostic,
        }
    }
    fn immutable_binding_error(&self, diagnostic: ae_span::Diagnostic, id: usize) -> ae_span::Diagnostic {
        let diagnostic = self.binding_label(diagnostic, id);
        if diagnostic.secondary.is_empty() { diagnostic } else {
            diagnostic.with_suggestion("If mutation is intended, declare the binding with `var`.")
        }
    }
    fn duplicate_binding_error(&self, name: &str) -> ae_span::Diagnostic {
        let diagnostic = self.fail(&format!("duplicate variable '{name}'")).with_code("E0104");
        match self.names.get(name) {
            Some(&(id, _, _)) => self.binding_label(diagnostic, id),
            None => diagnostic,
        }
    }
    pub(super) fn take(&mut self, expected: Kind) -> Result<(), ae_span::Diagnostic> {
        if *self.current() != expected { return Err(self.fail(&format!("expected {expected:?}, found {:?}", self.current()))); }
        self.pos += 1; Ok(())
    }
    pub(super) fn identifier(&mut self) -> Result<String, ae_span::Diagnostic> {
        match self.current().clone() { Kind::Ident(s) => { self.pos += 1; Ok(s) }, _ => Err(self.fail("expected identifier")) }
    }
    pub(super) fn position(&self) -> (usize, usize) {
        let token = &self.tokens[self.pos];
        (token.line, token.column)
    }
    // Prelude result types are always registered, so naming one cannot fail.
    pub(super) fn prelude_type(&self, name: &str) -> Type {
        match self.enum_names.get(name) {
            Some(&id) => Type::Enum(self.enums[id].name, id),
            None => unreachable!("prelude enum {name} is registered for every program"),
        }
    }
    pub(super) fn expression(&mut self, min: u8, moving: bool) -> Result<(Expr, Type), ae_span::Diagnostic> {
        if self.expression_depth == 0 { self.expression_nodes = 0; }
        if self.expression_depth >= 128 { return Err(self.fail("expression nesting limit exceeded (128)")); }
        self.expression_depth += 1;
        let result = self.expression_inner(min, moving);
        self.expression_depth -= 1;
        result
    }
    pub(super) fn expression_inner(&mut self, min: u8, moving: bool) -> Result<(Expr, Type), ae_span::Diagnostic> {
        self.expression_nodes += 1;
        if self.expression_nodes > 128 { return Err(self.fail("expression complexity limit exceeded (128)")); }
        let mut lhs = match self.current().clone() {
            Kind::Number(n) => { self.pos += 1; (Expr::Number(n), Type::Int) }
            Kind::Float(x) => { self.pos += 1; (Expr::FloatLit(x), Type::F64) }
            Kind::True => { self.pos += 1; (Expr::BoolLit(true), Type::Bool) }
            Kind::False => { self.pos += 1; (Expr::BoolLit(false), Type::Bool) }
            Kind::Text(text) => {
                let (line, column) = self.position();
                self.pos += 1;
                (Expr::StrLit(text, line, column), Type::String)
            }
            Kind::Minus => {
                let (line, column) = self.position();
                self.pos += 1;
                // Operands bind tighter than any binary operator: -a * b is (-a) * b.
                let (operand, ty) = self.expression(6, false)?;
                if !matches!(ty, Type::Int | Type::F64) { return Err(self.fail("unary '-' requires an i64 or f64 operand")); }
                (Expr::Neg(Box::new(operand), line, column), ty)
            }
            Kind::Bang => {
                self.pos += 1;
                let (operand, ty) = self.expression(6, false)?;
                if ty != Type::Bool { return Err(self.fail("'!' requires a boolean operand")); }
                (Expr::Not(Box::new(operand)), Type::Bool)
            }
            Kind::Ident(name) => {
                let name_offset = self.tokens[self.pos].offset;
                let (vline, vcol) = self.position();
                self.pos += 1;
                if name == "vec" && *self.current() == Kind::LBracket {
                    let (line, column) = (vline, vcol);
                    self.pos += 1;
                    let mut values = Vec::new();
                    let mut element_ty: Option<Type> = None;
                    if *self.current() != Kind::RBracket {
                        loop {
                            if values.len() >= 65536 { return Err(self.fail("vector element limit exceeded (65536)")); }
                            let (element, ty) = self.expression(0, false)?;
                            // The first element decides the vector: all i64 is
                            // `vec`, all f64 is `vec[f64]` (ADR-61). Mixing
                            // them is an error — the language has no implicit
                            // conversion, including inside a literal.
                            match element_ty {
                                None => {
                                    if ty == Type::Int || ty == Type::F64 { element_ty = Some(ty); }
                                    else { return Err(self.fail(&format!("vector elements must be i64 or f64, found {}", ty.name()))); }
                                }
                                Some(same) if ty == same => {}
                                Some(same) => return Err(self.fail(&format!("vector elements must all be {}; {} does not convert implicitly", same.name(), ty.name()))),
                            }
                            values.push(element);
                            if *self.current() != Kind::Comma { break; }
                            self.pos += 1;
                        }
                    }
                    // Empty literal defaults to the i64 vector; the f64
                    // elements chose vec[f64] (ADR-61).
                    let vector_ty = match element_ty {
                        Some(Type::F64) => Type::VecF,
                        _ => Type::Vec,
                    };
                    self.take(Kind::RBracket)?;
                    // Bind and fall through to the binary-operator loop.
                    (Expr::VecLit(values, vector_ty, line, column), vector_ty)
                } else if *self.current() == Kind::ColonColon {
                    // Enum::Variant or Enum::Variant(payload, ...)
                    let eid = *self.enum_names.get(&name)
                        .ok_or_else(|| self.error_at(name_offset, &format!("'{name}' is not an enum; variants are written Enum::Variant")))?;
                    self.enum_literal(eid, name_offset)?
                } else if self.generics.names.contains_key(&name) && *self.current() == Kind::LBracket {
                    // Generic enum literal: the arguments pick the layout, so
                    // Option[string]::Some and Option[i64]::Some are different
                    // variants of different types (ADR-21).
                    let gid = self.generics.names[&name];
                    self.pos += 1;
                    let mut args = Vec::new();
                    loop {
                        args.push(self.type_argument()?);
                        if *self.current() != Kind::Comma { break; }
                        self.pos += 1;
                    }
                    self.take(Kind::RBracket)?;
                    let eid = self.instantiate_enum(gid, args, name_offset)?;
                    self.enum_literal(eid, name_offset)?
                } else if self.generics.struct_names.contains_key(&name) && *self.current() == Kind::LBracket {
                    // Generic struct literal: the arguments pick the layout, so
                    // Pair[string, i64] and Pair[i64, i64] are different types.
                    let gid = self.generics.struct_names[&name];
                    self.pos += 1;
                    let mut args = Vec::new();
                    loop {
                        args.push(self.named_type()?);
                        if *self.current() != Kind::Comma { break; }
                        self.pos += 1;
                    }
                    self.take(Kind::RBracket)?;
                    let sid = self.instantiate_struct(gid, args, name_offset)?;
                    self.require_public("struct", &self.structs[sid].name.clone(), &self.structs[sid].file.clone(), self.structs[sid].public)?;
                    if *self.current() != Kind::LBrace {
                        return Err(self.error_at(name_offset, &format!("'{}' is a struct; construct it with '{}{{ ... }}'", self.structs[sid].name.clone(), self.structs[sid].name.clone())));
                    }
                    return self.struct_literal(sid, name_offset);
                } else if let Some(&sid) = self.struct_names.get(&name) {
                    self.require_public("struct", &self.structs[sid].name, &self.structs[sid].file, self.structs[sid].public)?;
                    if *self.current() == Kind::LBrace {
                        // A fresh literal takes no postfix; wrap it in
                        // parentheses for field access.
                        return self.struct_literal(sid, name_offset);
                    } else if *self.current() == Kind::LParen {
                        return Err(self.fail(&format!("'{}' is a struct, not a function", name)));
                    } else {
                        return Err(self.fail(&format!("'{}' is a struct name; construct it with '{} {{ ... }}'", name, name)));
                    }
                } else if name == "len" && *self.current() == Kind::LParen {
                    let (line, col) = (vline, vcol);
                    self.take(Kind::LParen)?;
                    // Fast paths for bare variables: array length is a
                    // compile-time constant; vec length borrows the slot.
                    let mut fast: Option<(Expr, Type)> = None;
                    if let Kind::Ident(arg_name) = self.current().clone() {
                        let after = self.tokens.get(self.pos + 1).map(|t| t.kind.clone());
                        if after == Some(Kind::RParen) {
                            if let Some(&(id, _, ty)) = self.names.get(&arg_name) {
                                if self.moved.contains(&id) { return Err(self.binding_label(self.error_at(name_offset, &format!("cannot use '{arg_name}' because it was moved")).with_code("E0102"), id)); }
                                match ty {
                                    Type::Array(n) => { self.pos += 2; fast = Some((Expr::Number(n as i64), Type::Int)); }
                                    Type::Vec | Type::StrVec | Type::VecRef | Type::StrVecRef | Type::Map => {
                                        self.pos += 2;
                                        fast = Some((Expr::Len(Box::new(Expr::Variable(id, ty, vline, vcol)), line, col), Type::Int));
                                    }
                                    _ => {}
                                }
                            }
                        }
                    }
                    match fast {
                        Some(pair) => pair,
                        None => {
                            let (operand, ty) = self.expression(0, false)?;
                            self.take(Kind::RParen)?;
                            // A token cursor has no length by design: counting
                            // would consume the stream, and the whole point of
                            // the cursor is not materialising the tokens.
                            if ty == Type::Tokens { return Err(self.fail("a token cursor has no len: counting would consume the stream, so use split(text, sep) when the count is needed")); }
                            if !matches!(ty, Type::String | Type::StrView | Type::Vec | Type::StrVec | Type::VecF | Type::VecRef | Type::VecFRef | Type::Map | Type::MapF) { return Err(self.fail("len expects a string, vec, map, or array variable")); }
                            (Expr::Len(Box::new(operand), line, col), Type::Int)
                        }
                    }
                } else if name == "slice_view" && *self.current() == Kind::LParen {
                    let (line, col) = (vline, vcol);
                    self.take(Kind::LParen)?;
                    let (base, base_ty) = self.expression(0, false)?;
                    // The view borrows its base. A bare string variable is the
                    // direct case; a string field of a *variable* struct is
                    // allowed too, and borrows the whole variable: any move,
                    // field write or replacement of that variable would free
                    // the bytes the view points at (ADR-52).
                    let (vid, field) = match (&base, base_ty) {
                        (Expr::Variable(id, Type::String, _, _), Type::String) => (*id, None),
                        (Expr::Field(inner, fname, Type::String), Type::String) => match **inner {
                            Expr::Variable(id, Type::Struct(_), _, _) => (id, Some(fname.clone())),
                            _ => return Err(self.fail("slice_view can follow a string field only on a struct variable, not on a nested expression")),
                        },
                        _ => return Err(self.fail("slice_view expects a string variable or a string field of a struct variable (views borrow their base)")),
                    };
                    if self.moved.contains(&vid) { return Err(self.binding_label(self.fail("cannot use a moved string in slice_view").with_code("E0102"), vid)); }
                    self.take(Kind::Comma)?;
                    let (start, start_ty) = self.expression(0, false)?;
                    if start_ty != Type::Int { return Err(self.fail("slice_view start must be i64")); }
                    self.take(Kind::Comma)?;
                    let (end, end_ty) = self.expression(0, false)?;
                    if end_ty != Type::Int { return Err(self.fail("slice_view end must be i64")); }
                    self.take(Kind::RParen)?;
                    // The base may not move or be reassigned while a view of
                    // it can still be read (until this scope ends).
                    let depth = self.scopes.len();
                    if !self.borrows.iter().any(|&(id, _)| id == vid) {
                        self.borrows.push((vid, depth));
                    }
                    (Expr::SliceView(vid, field, Box::new(start), Box::new(end), line, col), Type::StrView)
                } else if name == "tokenize" && *self.current() == Kind::LParen {
                    let (line, col) = (vline, vcol);
                    self.take(Kind::LParen)?;
                    let (base, base_ty) = self.expression(0, false)?;
                    // The cursor reads through a view of its base, so the base
                    // must be storage that outlives the loop: a string variable
                    // or a string field of a struct variable. A temporary (a
                    // concatenation, a call result) would be freed before the
                    // first iteration, so it is rejected (ADR-53).
                    let (vid, field) = match (&base, base_ty) {
                        (Expr::Variable(id, Type::String, _, _), Type::String) => (*id, None),
                        (Expr::Field(inner, fname, Type::String), Type::String) => match **inner {
                            Expr::Variable(id, Type::Struct(_), _, _) => (id, Some(fname.clone())),
                            _ => return Err(self.fail("tokenize can follow a string field only on a struct variable")),
                        },
                        _ => return Err(self.fail("tokenize expects a string variable or a string field of a struct variable (the cursor borrows its base)")),
                    };
                    if self.moved.contains(&vid) { return Err(self.binding_label(self.fail("cannot tokenize a moved string").with_code("E0102"), vid)); }
                    self.take(Kind::Comma)?;
                    let (sep, sep_ty) = self.expression(0, false)?;
                    if !matches!(sep_ty, Type::String | Type::StrView) { return Err(self.fail("tokenize expects a string or string view separator")); }
                    self.take(Kind::RParen)?;
                    // Same rule as a view: the base may not move or be
                    // reassigned while the cursor can still read it.
                    let depth = self.scopes.len();
                    if !self.borrows.iter().any(|&(id, _)| id == vid) {
                        self.borrows.push((vid, depth));
                    }
                    (Expr::Tokens(vid, field, Box::new(sep), line, col), Type::Tokens)
                } else if name == "count_lines_with" && *self.current() == Kind::LParen {
                    let (line, col) = (vline, vcol);
                    self.take(Kind::LParen)?;
                    let (text, text_ty) = self.expression(0, false)?;
                    if !matches!(text_ty, Type::String | Type::StrView) { return Err(self.fail("count_lines_with expects a string or string view")); }
                    self.take(Kind::Comma)?;
                    let (pat, pat_ty) = self.expression(0, false)?;
                    if !matches!(pat_ty, Type::String | Type::StrView) { return Err(self.fail("count_lines_with expects a string or string view pattern")); }
                    self.take(Kind::RParen)?;
                    (Expr::CountLinesWith(Box::new(text), Box::new(pat), line, col), Type::Int)
                } else if name == "lines" && *self.current() == Kind::LParen {
                    let (line, col) = (vline, vcol);
                    self.take(Kind::LParen)?;
                    let (text, text_ty) = self.expression(0, false)?;
                    if !matches!(text_ty, Type::String | Type::StrView) { return Err(self.fail("lines expects a string or string view")); }
                    self.take(Kind::RParen)?;
                    (Expr::Lines(Box::new(text), line, col), Type::StrVec)
                } else if name == "join" && *self.current() == Kind::LParen {
                    let (line, col) = (vline, vcol);
                    self.take(Kind::LParen)?;
                    // join borrows a bare string-vector variable.
                    let (parts, parts_ty) = self.expression(0, false)?;
                    let vid = match (&parts, parts_ty) {
                        (Expr::Variable(id, Type::StrVec, _, _), Type::StrVec) => *id,
                        _ => return Err(self.fail("join expects a bare vec[string] variable")),
                    };
                    if self.moved.contains(&vid) { return Err(self.binding_label(self.fail("cannot use a moved vector in join").with_code("E0102"), vid)); }
                    self.take(Kind::Comma)?;
                    let (sep, sep_ty) = self.expression(0, false)?;
                    if !matches!(sep_ty, Type::String | Type::StrView) { return Err(self.fail("join expects a string or string view separator")); }
                    self.take(Kind::RParen)?;
                    (Expr::Join(vid, Box::new(sep), line, col), Type::String)
                } else if name == "args" && *self.current() == Kind::LParen {
                    let (line, col) = (vline, vcol);
                    self.take(Kind::LParen)?;
                    self.take(Kind::RParen)?;
                    (Expr::Args(line, col), Type::StrVec)
                } else if name == "try_read_file" && *self.current() == Kind::LParen {
                    let (line, col) = (vline, vcol);
                    let ty = self.prelude_type("ReadResult");
                    self.take(Kind::LParen)?;
                    let (path, path_ty) = self.expression(0, false)?;
                    if !matches!(path_ty, Type::String | Type::StrView) { return Err(self.fail("try_read_file expects a string path")); }
                    self.take(Kind::RParen)?;
                    (Expr::TryReadFile(Box::new(path), ty, line, col), ty)
                } else if name == "try_write_file" && *self.current() == Kind::LParen {
                    let (line, col) = (vline, vcol);
                    let ty = self.prelude_type("WriteResult");
                    self.take(Kind::LParen)?;
                    let (path, path_ty) = self.expression(0, false)?;
                    if !matches!(path_ty, Type::String | Type::StrView) { return Err(self.fail("try_write_file expects a string path")); }
                    self.take(Kind::Comma)?;
                    let (content, content_ty) = self.expression(0, false)?;
                    if !matches!(content_ty, Type::String | Type::StrView) { return Err(self.fail("try_write_file expects a string or string view content")); }
                    self.take(Kind::RParen)?;
                    (Expr::TryWriteFile(Box::new(path), Box::new(content), ty, line, col), ty)
                } else if name == "try_append_file" && *self.current() == Kind::LParen {
                    let (line, col) = (vline, vcol);
                    let ty = self.prelude_type("WriteResult");
                    self.take(Kind::LParen)?;
                    let (path, path_ty) = self.expression(0, false)?;
                    if !matches!(path_ty, Type::String | Type::StrView) { return Err(self.fail("try_append_file expects a string path")); }
                    self.take(Kind::Comma)?;
                    let (content, content_ty) = self.expression(0, false)?;
                    if !matches!(content_ty, Type::String | Type::StrView) { return Err(self.fail("try_append_file expects a string or string view content")); }
                    self.take(Kind::RParen)?;
                    (Expr::TryAppendFile(Box::new(path), Box::new(content), ty, line, col), ty)
                } else if name == "env" && *self.current() == Kind::LParen {
                    let (line, col) = (vline, vcol);
                    let ty = self.prelude_type("ReadResult");
                    self.take(Kind::LParen)?;
                    let (variable, variable_ty) = self.expression(0, false)?;
                    if !matches!(variable_ty, Type::String | Type::StrView) { return Err(self.fail("env expects a string variable name")); }
                    self.take(Kind::RParen)?;
                    (Expr::Env(Box::new(variable), ty, line, col), ty)
                } else if (name == "read_stdin" || name == "cwd") && *self.current() == Kind::LParen {
                    let (line, col) = (vline, vcol);
                    let ty = self.prelude_type("ReadResult");
                    self.take(Kind::LParen)?;
                    self.take(Kind::RParen)?;
                    (if name == "read_stdin" { Expr::ReadStdin(ty, line, col) } else { Expr::Cwd(ty, line, col) }, ty)
                } else if name == "f64vec_new" && *self.current() == Kind::LParen {
                    let (line, col) = (vline, vcol);
                    self.take(Kind::LParen)?;
                    self.take(Kind::RParen)?;
                    (Expr::F64VecNew(line, col), Type::VecF)
                } else if name == "strvec_new" && *self.current() == Kind::LParen {
                    let (line, col) = (vline, vcol);
                    self.take(Kind::LParen)?;
                    self.take(Kind::RParen)?;
                    (Expr::StrVecNew(line, col), Type::StrVec)
                } else if name == "run_command" && *self.current() == Kind::LParen {
                    let (line, col) = (vline, vcol);
                    let ty = self.prelude_type("CommandResult");
                    self.take(Kind::LParen)?;
                    let (program, program_ty) = self.expression(0, false)?;
                    if !matches!(program_ty, Type::String | Type::StrView) { return Err(self.fail("run_command expects a string program name")); }
                    self.take(Kind::Comma)?;
                    // Like join, the argument vector is borrowed, never moved:
                    // the callee only reads the elements during the call.
                    let (args, args_ty) = self.expression(0, false)?;
                    let vid = match (&args, args_ty) {
                        (Expr::Variable(id, Type::StrVec, _, _), Type::StrVec) => *id,
                        _ => return Err(self.fail("run_command expects a bare vec[string] variable for its arguments")),
                    };
                    if self.moved.contains(&vid) { return Err(self.binding_label(self.fail("cannot use a moved vector in run_command").with_code("E0102"), vid)); }
                    self.take(Kind::RParen)?;
                    (Expr::RunCommand(Box::new(program), vid, ty, line, col), ty)
                } else if name == "find" && *self.current() == Kind::LParen {
                    // Byte offset of the first match, as a result value: a -1
                    // sentinel would let callers ignore "not found", while
                    // every other fallible built-in returns a result value
                    // whose error must be handled.
                    let (line, col) = (vline, vcol);
                    let ty = self.prelude_type("IntResult");
                    self.take(Kind::LParen)?;
                    let (haystack, haystack_ty) = self.expression(0, false)?;
                    if !matches!(haystack_ty, Type::String | Type::StrView) { return Err(self.fail("find expects a string to search in")); }
                    self.take(Kind::Comma)?;
                    let (needle, needle_ty) = self.expression(0, false)?;
                    if !matches!(needle_ty, Type::String | Type::StrView) { return Err(self.fail("find expects a string to search for")); }
                    self.take(Kind::RParen)?;
                    (Expr::Find(Box::new(haystack), Box::new(needle), ty, line, col), ty)
                } else if (name == "json_get" || name == "json_int" || name == "json_bool" || name == "json_len" || name == "json_has") && *self.current() == Kind::LParen {
                    let (line, col) = (vline, vcol);
                    let ty = match name.as_str() {
                        "json_get" => self.prelude_type("ReadResult"),
                        "json_has" => Type::Bool,
                        "json_bool" => self.prelude_type("BoolResult"),
                        _ => self.prelude_type("IntResult"),
                    };
                    self.take(Kind::LParen)?;
                    let (doc, doc_ty) = self.expression(0, false)?;
                    if !matches!(doc_ty, Type::String | Type::StrView) { return Err(self.fail(&format!("{name} expects a JSON document string"))); }
                    self.take(Kind::Comma)?;
                    let (path, path_ty) = self.expression(0, false)?;
                    if !matches!(path_ty, Type::String | Type::StrView) { return Err(self.fail(&format!("{name} expects a string path"))); }
                    self.take(Kind::RParen)?;
                    let (doc, path) = (Box::new(doc), Box::new(path));
                    (match name.as_str() {
                        "json_get" => Expr::JsonGet(doc, path, ty, line, col),
                        "json_int" => Expr::JsonInt(doc, path, ty, line, col),
                        "json_bool" => Expr::JsonBool(doc, path, ty, line, col),
                        "json_len" => Expr::JsonLen(doc, path, ty, line, col),
                        _ => Expr::JsonHas(doc, path, line, col),
                    }, ty)
                } else if (name == "now_ms" || name == "now_s") && *self.current() == Kind::LParen {
                    self.take(Kind::LParen)?;
                    self.take(Kind::RParen)?;
                    (if name == "now_ms" { Expr::NowMs } else { Expr::NowS }, Type::Int)
                } else if name == "try_list_dir" && *self.current() == Kind::LParen {
                    let (line, col) = (vline, vcol);
                    let ty = self.prelude_type("DirResult");
                    self.take(Kind::LParen)?;
                    let (path, path_ty) = self.expression(0, false)?;
                    if !matches!(path_ty, Type::String | Type::StrView) { return Err(self.fail("try_list_dir expects a string path")); }
                    self.take(Kind::RParen)?;
                    (Expr::TryListDir(Box::new(path), ty, line, col), ty)
                } else if name == "path_is_dir" && *self.current() == Kind::LParen {
                    let (line, col) = (vline, vcol);
                    self.take(Kind::LParen)?;
                    let (path, path_ty) = self.expression(0, false)?;
                    if !matches!(path_ty, Type::String | Type::StrView) { return Err(self.fail("path_is_dir expects a string path")); }
                    self.take(Kind::RParen)?;
                    (Expr::PathIsDir(Box::new(path), line, col), Type::Bool)
                } else if name == "path_exists" && *self.current() == Kind::LParen {
                    let (line, col) = (vline, vcol);
                    self.take(Kind::LParen)?;
                    let (path, path_ty) = self.expression(0, false)?;
                    if !matches!(path_ty, Type::String | Type::StrView) { return Err(self.fail("path_exists expects a string path")); }
                    self.take(Kind::RParen)?;
                    (Expr::PathExists(Box::new(path), line, col), Type::Bool)
                } else if name == "map_new" && *self.current() == Kind::LParen {
                    let (line, col) = (vline, vcol);
                    self.take(Kind::LParen)?;
                    self.take(Kind::RParen)?;
                    (Expr::MapNew(line, col, MapKind::Int), Type::Map)
                } else if name == "f64map_new" && *self.current() == Kind::LParen {
                    let (line, col) = (vline, vcol);
                    self.take(Kind::LParen)?;
                    self.take(Kind::RParen)?;
                    (Expr::MapNew(line, col, MapKind::Float), Type::MapF)
                } else if matches!(name.as_str(), "map_get" | "map_has" | "map_remove" | "map_inc" | "keys") && *self.current() == Kind::LParen {
                    if name == "map_inc" {
                        // `map_inc(m, key, delta)` is the counting compound:
                        // read-modify-write with missing keys starting from
                        // zero, returning the new value. One lookup instead
                        // of a get plus a put.
                        let (line, col) = (vline, vcol);
                        self.take(Kind::LParen)?;
                        let (map, map_ty) = self.expression(0, false)?;
                        let kind = map_kind_of(map_ty);
                        let place = match (&map, map_ty) {
                            (Expr::Variable(id, ty2, _, _), t) if *ty2 == t && matches!(t, Type::Map | Type::MapF) => MapPlace::Var(*id, kind),
                            (Expr::Field(base, fname, t), _) if matches!(*t, Type::Map | Type::MapF) => match **base {
                                Expr::Variable(id, _, _, _) => MapPlace::Field(id, fname.clone(), kind),
                                _ => return Err(self.fail("map_inc needs a map variable or a field of one")),
                            },
                            _ => return Err(self.fail("map_inc expects a map variable or a field of one")),
                        };
                        if self.moved.contains(&place.slot()) { return Err(self.binding_label(self.fail("cannot use 'map_inc' on a moved map").with_code("E0102"), place.slot())); }
                        self.take(Kind::Comma)?;
                        let (key, key_ty) = self.expression(0, false)?;
                        if !matches!(key_ty, Type::String | Type::StrView) { return Err(self.fail("map_inc expects a string key")); }
                        self.take(Kind::Comma)?;
                        let (delta, delta_ty) = self.expression(0, false)?;
                        if delta_ty != map_element_ty(kind) { return Err(self.fail(&format!("map_inc expects a {} delta", map_element_ty(kind).name()))); }
                        self.take(Kind::RParen)?;
                        (Expr::MapInc(place, Box::new(key), Box::new(delta), line, col), map_element_ty(kind))
                    } else {
                    let (line, col) = (vline, vcol);
                    self.take(Kind::LParen)?;
                    // A bare map variable keeps the borrow explicit: the map is
                    // read (or mutated) only for the duration of this call.
                    let (map, map_ty) = self.expression(0, false)?;
                    let kind = map_kind_of(map_ty);
                    let ty = match name.as_str() {
                        "map_get" => self.prelude_type(if kind == MapKind::Float { "FloatResult" } else { "IntResult" }),
                        "keys" => Type::StrVec,
                        _ => Type::Bool,
                    };
                    let place = match (&map, map_ty) {
                        (Expr::Variable(id, ty2, _, _), t) if *ty2 == t && matches!(t, Type::Map | Type::MapF) => MapPlace::Var(*id, kind),
                        (Expr::Field(base, fname, t), _) if matches!(*t, Type::Map | Type::MapF) => match **base {
                            Expr::Variable(id, _, _, _) => MapPlace::Field(id, fname.clone(), kind),
                            _ => return Err(self.fail(&format!("{name} needs a map variable or a field of one"))),
                        },
                        _ => return Err(self.fail(&format!("{name} expects a map variable or a field of one"))),
                    };
                    if self.moved.contains(&place.slot()) { return Err(self.binding_label(self.fail(&format!("cannot use '{name}' on a moved map")).with_code("E0102"), place.slot())); }
                    if name == "keys" {
                        self.take(Kind::RParen)?;
                        (Expr::MapKeys(place, line, col), ty)
                    } else {
                        self.take(Kind::Comma)?;
                        let (key, key_ty) = self.expression(0, false)?;
                        if !matches!(key_ty, Type::String | Type::StrView) { return Err(self.fail(&format!("{name} expects a string key"))); }
                        self.take(Kind::RParen)?;
                        let builtin = match name.as_str() {
                            "map_get" => Expr::MapGet(place, Box::new(key), ty, line, col),
                            "map_has" => Expr::MapHas(place, Box::new(key), line, col),
                            _ => Expr::MapRemove(place, Box::new(key), line, col),
                        };
                        (builtin, ty)
                    }
                    }
                } else if matches!(name.as_str(), "utf8_len" | "utf8_prefix") && *self.current() == Kind::LParen {
                    let (line, col) = (vline, vcol);
                    self.take(Kind::LParen)?;
                    let (text, text_ty) = self.expression(0, false)?;
                    if !matches!(text_ty, Type::String | Type::StrView) { return Err(self.fail(&format!("{name} expects a string or string view"))); }
                    if name == "utf8_len" {
                        self.take(Kind::RParen)?;
                        (Expr::Utf8Len(Box::new(text), line, col), Type::Int)
                    } else {
                        self.take(Kind::Comma)?;
                        let (count, count_ty) = self.expression(0, false)?;
                        if count_ty != Type::Int { return Err(self.fail("utf8_prefix expects an i64 character count")); }
                        self.take(Kind::RParen)?;
                        (Expr::Utf8Prefix(Box::new(text), Box::new(count), line, col), Type::String)
                    }
                } else if name == "path_join" && *self.current() == Kind::LParen {
                    let (line, col) = (vline, vcol);
                    self.take(Kind::LParen)?;
                    let (dir, dir_ty) = self.expression(0, false)?;
                    if !matches!(dir_ty, Type::String | Type::StrView) { return Err(self.fail("path_join expects a string directory")); }
                    self.take(Kind::Comma)?;
                    let (name, name_ty) = self.expression(0, false)?;
                    if !matches!(name_ty, Type::String | Type::StrView) { return Err(self.fail("path_join expects a string name")); }
                    self.take(Kind::RParen)?;
                    (Expr::PathJoin(Box::new(dir), Box::new(name), line, col), Type::String)
                } else if name == "path_name" && *self.current() == Kind::LParen {
                    let (line, col) = (vline, vcol);
                    self.take(Kind::LParen)?;
                    let (path, path_ty) = self.expression(0, false)?;
                    if !matches!(path_ty, Type::String | Type::StrView) { return Err(self.fail("path_name expects a string path")); }
                    self.take(Kind::RParen)?;
                    (Expr::PathName(Box::new(path), line, col), Type::String)
                } else if name == "path_parent" && *self.current() == Kind::LParen {
                    let (line, col) = (vline, vcol);
                    self.take(Kind::LParen)?;
                    let (path, path_ty) = self.expression(0, false)?;
                    if !matches!(path_ty, Type::String | Type::StrView) { return Err(self.fail("path_parent expects a string path")); }
                    self.take(Kind::RParen)?;
                    (Expr::PathParent(Box::new(path), line, col), Type::String)
                } else if name == "try_file_info" && *self.current() == Kind::LParen {
                    let (line, col) = (vline, vcol);
                    let ty = self.prelude_type("FileResult");
                    self.take(Kind::LParen)?;
                    let (path, path_ty) = self.expression(0, false)?;
                    if !matches!(path_ty, Type::String | Type::StrView) { return Err(self.fail("try_file_info expects a string path")); }
                    self.take(Kind::RParen)?;
                    (Expr::TryFileInfo(Box::new(path), ty, line, col), ty)
                } else if matches!(name.as_str(), "try_copy_file" | "try_rename_path") && *self.current() == Kind::LParen {
                    let (line, col) = (vline, vcol);
                    let ty = self.prelude_type("StatusResult");
                    self.take(Kind::LParen)?;
                    let (from, from_ty) = self.expression(0, false)?;
                    if !matches!(from_ty, Type::String | Type::StrView) { return Err(self.fail(&format!("{name} expects a string source path"))); }
                    self.take(Kind::Comma)?;
                    let (to, to_ty) = self.expression(0, false)?;
                    if !matches!(to_ty, Type::String | Type::StrView) { return Err(self.fail(&format!("{name} expects a string destination path"))); }
                    self.take(Kind::RParen)?;
                    let builtin = if name == "try_copy_file" {
                        Expr::TryCopyFile(Box::new(from), Box::new(to), ty, line, col)
                    } else {
                        Expr::TryRenamePath(Box::new(from), Box::new(to), ty, line, col)
                    };
                    (builtin, ty)
                } else if matches!(name.as_str(), "try_remove_file" | "try_create_dir" | "try_remove_dir" | "set_cwd") && *self.current() == Kind::LParen {
                    let (line, col) = (vline, vcol);
                    let ty = self.prelude_type("StatusResult");
                    self.take(Kind::LParen)?;
                    let (path, path_ty) = self.expression(0, false)?;
                    if !matches!(path_ty, Type::String | Type::StrView) { return Err(self.fail(&format!("{name} expects a string path"))); }
                    self.take(Kind::RParen)?;
                    let builtin = if name == "try_remove_file" {
                        Expr::TryRemoveFile(Box::new(path), ty, line, col)
                    } else if name == "try_remove_dir" {
                        Expr::TryRemoveDir(Box::new(path), ty, line, col)
                    } else if name == "set_cwd" {
                        Expr::SetCwd(Box::new(path), ty, line, col)
                    } else {
                        Expr::TryCreateDir(Box::new(path), ty, line, col)
                    };
                    (builtin, ty)
                } else if name == "parse_int" && *self.current() == Kind::LParen {
                    let (line, col) = (vline, vcol);
                    let ty = self.prelude_type("IntResult");
                    self.take(Kind::LParen)?;
                    let (text, text_ty) = self.expression(0, false)?;
                    if !matches!(text_ty, Type::String | Type::StrView) { return Err(self.fail("parse_int expects a string or string view")); }
                    self.take(Kind::RParen)?;
                    (Expr::ParseInt(Box::new(text), ty, line, col), ty)
                } else if name == "to_string" && *self.current() == Kind::LParen {
                    let (line, col) = (vline, vcol);
                    self.take(Kind::LParen)?;
                    let (value, value_ty) = self.expression(0, false)?;
                    let builtin = match value_ty {
                        Type::Int => Expr::ToString(Box::new(value), line, col),
                        Type::F64 => Expr::FloatToString(Box::new(value), line, col),
                        _ => return Err(self.fail("to_string expects an i64 or f64")),
                    };
                    self.take(Kind::RParen)?;
                    (builtin, Type::String)
                } else if name == "to_f64" && *self.current() == Kind::LParen {
                    let (line, col) = (vline, vcol);
                    self.take(Kind::LParen)?;
                    let (value, value_ty) = self.expression(0, false)?;
                    if value_ty != Type::Int { return Err(self.fail("to_f64 expects an i64")); }
                    self.take(Kind::RParen)?;
                    (Expr::ToF64(Box::new(value), line, col), Type::F64)
                } else if name == "to_i64" && *self.current() == Kind::LParen {
                    let (line, col) = (vline, vcol);
                    let ty = self.prelude_type("IntResult");
                    self.take(Kind::LParen)?;
                    let (value, value_ty) = self.expression(0, false)?;
                    if value_ty != Type::F64 { return Err(self.fail("to_i64 expects an f64")); }
                    self.take(Kind::RParen)?;
                    (Expr::ToI64(Box::new(value), ty, line, col), ty)
                } else if MATH_FNS.iter().any(|(n, _, _)| *n == name.as_str()) && *self.current() == Kind::LParen {
                    // Math builtins (ADR-60): fixed arity, every argument must
                    // already be f64 — the language has no implicit conversion.
                    let index = MATH_FNS.iter().position(|(n, _, _)| *n == name.as_str()).unwrap();
                    let arity = MATH_FNS[index].2;
                    let (line, col) = (vline, vcol);
                    self.take(Kind::LParen)?;
                    let mut args: Vec<Box<Expr>> = Vec::new();
                    for position in 0..arity {
                        if *self.current() == Kind::RParen { return Err(self.fail(&format!("{name} expects {} f64 argument(s)", arity))); }
                        if position > 0 { self.take(Kind::Comma)?; }
                        let (arg, arg_ty) = self.expression(0, false)?;
                        if arg_ty != Type::F64 { return Err(self.fail(&format!("{name} expects an f64 argument; convert integers explicitly with to_f64"))); }
                        args.push(Box::new(arg));
                    }
                    if *self.current() == Kind::Comma { return Err(self.fail(&format!("{name} takes {} argument(s)", arity))); }
                    self.take(Kind::RParen)?;
                    (Expr::MathCall(index as u8, args, line, col), Type::F64)
                } else if matches!(name.as_str(), "read_file" | "write_file" | "bytes" | "contains" | "starts_with" | "ends_with" | "slice" | "split" | "trim") && *self.current() == Kind::LParen {
                    let (line, col) = (vline, vcol);
                    self.pos += 1;
                    let (first, first_ty) = self.expression(0, false)?;
                    if !matches!(first_ty, Type::String | Type::StrView) { return Err(self.fail(&format!("{name} expects a string or string view argument"))); }
                    let builtin = if name == "read_file" {
                        self.take(Kind::RParen)?;
                        Expr::ReadFile(Box::new(first), line, col)
                    } else if name == "bytes" {
                        self.take(Kind::RParen)?;
                        Expr::Bytes(Box::new(first), line, col)
                    } else if name == "trim" {
                        self.take(Kind::RParen)?;
                        Expr::Trim(Box::new(first), line, col)
                    } else if name == "slice" {
                        self.take(Kind::Comma)?;
                        let (start, start_ty) = self.expression(0, false)?;
                        if start_ty != Type::Int { return Err(self.fail("slice start must be i64")); }
                        self.take(Kind::Comma)?;
                        let (end, end_ty) = self.expression(0, false)?;
                        if end_ty != Type::Int { return Err(self.fail("slice end must be i64")); }
                        self.take(Kind::RParen)?;
                        Expr::Slice(Box::new(first), Box::new(start), Box::new(end), line, col)
                    } else if name == "split" {
                        self.take(Kind::Comma)?;
                        let (second, second_ty) = self.expression(0, false)?;
                        if !matches!(second_ty, Type::String | Type::StrView) { return Err(self.fail("split expects a string or string view separator")); }
                        self.take(Kind::RParen)?;
                        Expr::Split(Box::new(first), Box::new(second), line, col)
                    } else if matches!(name.as_str(), "contains" | "starts_with" | "ends_with") {
                        self.take(Kind::Comma)?;
                        let (second, second_ty) = self.expression(0, false)?;
                        if !matches!(second_ty, Type::String | Type::StrView) { return Err(self.fail(&format!("{name} expects a string or string view pattern"))); }
                        self.take(Kind::RParen)?;
                        match name.as_str() {
                            "contains" => Expr::Contains(Box::new(first), Box::new(second), line, col),
                            "starts_with" => Expr::StartsWith(Box::new(first), Box::new(second)),
                            _ => Expr::EndsWith(Box::new(first), Box::new(second)),
                        }
                    } else {
                        self.take(Kind::Comma)?;
                        let (second, second_ty) = self.expression(0, false)?;
                        if !matches!(second_ty, Type::String | Type::StrView) { return Err(self.fail("write_file expects a string or string view content")); }
                        self.take(Kind::RParen)?;
                        Expr::WriteFile(Box::new(first), Box::new(second), line, col)
                    };
                    let ty = match name.as_str() { "write_file" => Type::Int, "bytes" => Type::Vec, "contains" | "starts_with" | "ends_with" => Type::Bool, "split" => Type::StrVec, _ => Type::String };
                    (builtin, ty)
                } else if !self.names.contains_key(&name) && self.generics.fn_names.contains_key(&name) {
                    // Generic call: the type arguments choose the body, so they
                    // are mandatory — the language has no type inference.
                    let gid = self.generics.fn_names[&name];
                    if *self.current() != Kind::LBracket {
                        return Err(self.error_at(name_offset, &format!("'{name}' is a generic function; write its type arguments, e.g. {name}[i64](...)")));
                    }
                    self.pos += 1;
                    let mut type_args = Vec::new();
                    loop {
                        if type_args.len() >= 8 { return Err(self.fail("type argument limit exceeded (8)")); }
                        type_args.push(self.named_type()?);
                        if *self.current() != Kind::Comma { break; }
                        self.pos += 1;
                    }
                    self.take(Kind::RBracket)?;
                    let (id, types, ret) = self.instantiate_fn(gid, type_args, name_offset)?;
                    let args = self.call_arguments(&name, &types)?;
                    (Expr::Call(id, args, ret), ret)
                } else if *self.current() == Kind::LParen {
                    let signature = match self.functions.get(&name).cloned() {
                        Some(signature) => signature,
                        // Not a plain function: a trait method of one of this
                        // generic's constrained parameters (or a genuine
                        // unknown name, which resolve_trait_call reports).
                        None => self.resolve_trait_call(&name, name_offset)?,
                    };
                    if !signature.public && signature.file != PRELUDE && signature.file != self.file {
                        return Err(self.error_at(name_offset, &format!("function '{name}' is private to {} (mark it 'pub' to export it)", signature.file)));
                    }
                    let (id, types, ret) = (signature.id, signature.params.clone(), signature.ret);
                    let args = self.call_arguments(&name, &types)?;
                    (Expr::Call(id, args, ret), ret)
                } else {
                    let (id, _, ty) = *self.names.get(&name).ok_or_else(|| {
                        // A generic enum used as a value: the arguments are
                        // mandatory, so say that instead of "unknown variable".
                        if self.generics.names.contains_key(&name) {
                            return self.error_at(name_offset, &format!("'{name}' is a generic enum; write '{name}[string]::Variant'"));
                        }
                        self.error_at(name_offset, &format!("unknown variable '{name}'")).with_code("E0101")
                    })?;
                    if self.moved.contains(&id) { return Err(self.binding_label(self.error_at(name_offset, &format!("cannot use '{name}' because it was moved")).with_code("E0102"), id)); }
                    if let Type::Array(len) = ty {
                        let (line, col) = self.position(); self.take(Kind::LBracket)?;
                        let (index, index_ty) = self.expression(0, false)?;
                        if index_ty != Type::Int { return Err(self.fail("array index must be an integer")); }
                        self.take(Kind::RBracket)?;
                        (Expr::Index(id, len, Box::new(index), line, col), Type::Int)
                    } else if (ty == Type::StrVec || ty == Type::StrVecRef) && *self.current() == Kind::LBracket {
                        // Reading an element borrows it; only mutation is
                        // rejected while a borrow is live (see the store paths).
                        let (line, col) = self.position(); self.take(Kind::LBracket)?;
                        let (index, index_ty) = self.expression(0, false)?;
                        if index_ty != Type::Int { return Err(self.fail("vector index must be an integer")); }
                        self.take(Kind::RBracket)?;
                        (Expr::StrVecIndex(id, Box::new(index), line, col, ty == Type::StrVecRef), Type::StrView)
                    } else if (ty == Type::Vec || ty == Type::VecRef || ty == Type::VecF || ty == Type::VecFRef) && *self.current() == Kind::LBracket {
                        // Vec variables may also be used bare (moves, len);
                        // indexing is optional. Borrowed vectors are read-only.
                        let (line, col) = self.position(); self.take(Kind::LBracket)?;
                        let (index, index_ty) = self.expression(0, false)?;
                        if index_ty != Type::Int { return Err(self.fail("vector index must be an integer")); }
                        self.take(Kind::RBracket)?;
                        let elem = if ty == Type::VecF || ty == Type::VecFRef { Type::F64 } else { Type::Int };
                        (Expr::VecIndex(id, Box::new(index), line, col, ty == Type::VecRef || ty == Type::VecFRef, elem), elem)
                    } else {
                        (Expr::Variable(id, ty, vline, vcol), ty)
                    }
                }
            }
            Kind::LParen => { self.pos += 1; let inner = self.expression(0, moving)?; self.take(Kind::RParen)?; inner }
            _ => return Err(self.fail("expected expression")),
        };
        // Postfix field access binds tighter than every operator.
        loop {
            if *self.current() != Kind::Dot { break; }
            self.pos += 1;
            let fname = self.identifier()?;
            let (base, base_ty) = lhs;
            let sid = match base_ty { Type::Struct(sid) => sid, other => return Err(self.fail(&format!("type {} has no fields", other.name()))) };
            let fty = {
                let def = &self.structs[sid];
                let fidx = def.fields.iter().position(|(n, _)| *n == fname)
                    .ok_or_else(|| self.fail(&format!("struct '{}' has no field '{}'", def.name, fname)))?;
                def.fields[fidx].1
            };
            lhs = (Expr::Field(Box::new(base), fname, fty), fty);
        }
        // Move bookkeeping happens once the full postfix chain is known:
        // bare owned variables move; owned fields are borrows and can never
        // be moved out of their struct.
        if moving {
            let move_target = match (&lhs.0, lhs.1) {
                (_, ty) if !ty.owned() => None,
                (Expr::Variable(id, _, _, _), _) => {
                    if self.borrows.iter().any(|&(bid, _)| bid == *id) {
                        return Err(self.fail("cannot move a value while it is borrowed"));
                    }
                    Some(MoveKind::Var(*id))
                }
                (Expr::Field(_, _, fty), _) if fty.owned() => Some(MoveKind::Field),
                (Expr::StrVecIndex(_, _, _, _, _), _) => Some(MoveKind::StrVecElement),
                _ => None,
            };
            match move_target {
                Some(MoveKind::Var(id)) => { if !self.moved.contains(&id) { self.moved.insert(id); } }
                Some(MoveKind::Field) => return Err(self.fail("cannot move a field out of a struct; use it where a borrow works")),
                Some(MoveKind::StrVecElement) => return Err(self.fail("cannot move an element out of a vector; use it where a borrow works")),
                None => {}
            }
        }
        loop {
            let (prec, op) = match self.current() {
                Kind::Or => (1u8, Kind::Or), Kind::And => (2, Kind::And),
                Kind::EqEq => (3, Kind::EqEq), Kind::NotEq => (3, Kind::NotEq),
                Kind::Less => (3, Kind::Less), Kind::LessEq => (3, Kind::LessEq),
                Kind::Greater => (3, Kind::Greater), Kind::GreaterEq => (3, Kind::GreaterEq),
                Kind::Plus => (4, Kind::Plus), Kind::Minus => (4, Kind::Minus),
                Kind::Star => (5, Kind::Star), Kind::Slash => (5, Kind::Slash), Kind::Percent => (5, Kind::Percent),
                _ => break,
            };
            if prec < min { break; }
            let (line, column) = self.position();
            self.pos += 1;
            // `+` always consumes its string operands, even in borrow contexts.
            let (rhs, rhs_ty) = self.expression(prec + 1, op == Kind::Plus)?;
            let (lhs_expr, lhs_ty) = lhs;
            let ty = match op {
                Kind::And | Kind::Or => {
                    if lhs_ty != Type::Bool || rhs_ty != Type::Bool { return Err(self.fail(&format!("'{}' requires boolean operands", if op == Kind::And { "&&" } else { "||" }))); }
                    Type::Bool
                }
                Kind::EqEq | Kind::NotEq | Kind::Less | Kind::LessEq | Kind::Greater | Kind::GreaterEq => {
                    let comparable = match (lhs_ty, rhs_ty) {
                        (Type::Int, Type::Int) => true,
                        (Type::F64, Type::F64) => true,
                        (Type::Bool, Type::Bool) => op == Kind::EqEq || op == Kind::NotEq,
                        (Type::String | Type::StrView, Type::String | Type::StrView) => op == Kind::EqEq || op == Kind::NotEq,
                        _ => false,
                    };
                    if !comparable {
                        return Err(self.fail("comparison requires two i64, two f64, or two strings/views with == or !="));
                    }
                    // String/view equality gets a dedicated node so emission
                    // never confuses it with C pointer comparison.
                    if (op == Kind::EqEq || op == Kind::NotEq) && matches!(lhs_ty, Type::String | Type::StrView) {
                        lhs = (Expr::StrEq(Box::new(lhs_expr), Box::new(rhs), op == Kind::NotEq), Type::Bool);
                        continue;
                    }
                    Type::Bool
                }
                Kind::Plus if matches!(lhs_ty, Type::String | Type::StrView) || matches!(rhs_ty, Type::String | Type::StrView) => {
                    // `+` builds a new owned string. Two views are read only and
                    // copied into the result, so no view can be consumed.
                    let stringlike = |ty: Type| matches!(ty, Type::String | Type::StrView);
                    if !stringlike(lhs_ty) || !stringlike(rhs_ty) { return Err(self.fail("'+' requires two strings, two string views, or two integers")); }
                    // The left operand was parsed in the caller's mode; using it
                    // here consumes it, so retroactively record the move. Only a
                    // plain owned variable is consumed: fields, vector elements
                    // and views are borrowed and get copied by the emitter.
                    if lhs_ty == Type::String {
                        if let Expr::Variable(id, ty, _, _) = &lhs_expr {
                            if ty.owned() && !self.moved.contains(id) { self.moved.insert(*id); }
                        }
                    }
                    Type::String
                }
                _ => {
                    if lhs_ty == Type::F64 || rhs_ty == Type::F64 {
                        if lhs_ty != Type::F64 || rhs_ty != Type::F64 {
                            return Err(self.fail("mixed i64/f64 arithmetic is not allowed; convert one side explicitly with to_f64 or to_i64"));
                        }
                        if op == Kind::Percent {
                            return Err(self.fail("'%' is defined for i64 operands only"));
                        }
                        Type::F64
                    } else {
                        if lhs_ty != Type::Int || rhs_ty != Type::Int { return Err(self.fail("arithmetic requires integer operands")); }
                        Type::Int
                    }
                }
            };
            lhs = (Expr::Binary(Box::new(lhs_expr), op, Box::new(rhs), line, column, ty), ty);
        }
        Ok(lhs)
    }
    pub(super) fn named_type(&mut self) -> Result<Type, ae_span::Diagnostic> {
        let name = self.identifier()?;
        // Inside a generic instantiation a type parameter stands for the
        // argument the call site supplied; nothing must reach code generation
        // with a parameter still in it.
        if let Some(&concrete) = self.subst.get(&name) { return Ok(concrete); }
        match name.as_str() {
            "i64" => Ok(Type::Int),
            "f64" => Ok(Type::F64),
            "bool" => Ok(Type::Bool),
            "string" => Ok(Type::String),
            "vec" => {
                // `vec` alone is the i64 vector; `vec[string]` and `vec[f64]`
                // (ADR-61) name the others, so a type annotation can say which
                // it means instead of leaving owned vectors unnameable.
                if *self.current() != Kind::LBracket { return Ok(Type::Vec); }
                self.pos += 1;
                let element = self.identifier()?;
                self.take(Kind::RBracket)?;
                match element.as_str() {
                    "string" => Ok(Type::StrVec),
                    "f64" => Ok(Type::VecF),
                    _ => Err(self.fail("only vec[string] and vec[f64] name an element type; vector elements are i64 by default")),
                }
            }
            "map" => {
                // Two shapes exist (ADR-65); the shape is spelled out so the
                // code says what it means instead of guessing from context.
                self.take(Kind::LBracket)?;
                if self.identifier()? != "string" { return Err(self.fail("map keys must be string")); }
                self.take(Kind::Comma)?;
                let value = self.identifier()?;
                self.take(Kind::RBracket)?;
                match value.as_str() {
                    "i64" => Ok(Type::Map),
                    "f64" => Ok(Type::MapF),
                    _ => Err(self.fail("map values must be i64 or f64")),
                }
            }
            other => {
                if let Some(&gid) = self.generics.names.get(other) {
                    if *self.current() != Kind::LBracket {
                        return Err(self.fail(&format!("'{other}' is a generic enum; write its type arguments, e.g. {other}[string]")));
                    }
                    let offset = self.tokens[self.pos].offset;
                    self.pos += 1;
                    let mut args = Vec::new();
                    loop {
                        args.push(self.type_argument()?);
                        if *self.current() != Kind::Comma { break; }
                        self.pos += 1;
                    }
                    self.take(Kind::RBracket)?;
                    let eid = self.instantiate_enum(gid, args, offset)?;
                    let def = &self.enums[eid];
                    self.require_public("enum", def.name, &def.file.clone(), def.public)?;
                    return Ok(Type::Enum(def.name, eid));
                }
                if let Some(&gid) = self.generics.struct_names.get(other) {
                    if *self.current() != Kind::LBracket {
                        return Err(self.fail(&format!("'{other}' is a generic struct; write its type arguments, e.g. {other}[string]")));
                    }
                    let offset = self.tokens[self.pos].offset;
                    self.pos += 1;
                    let mut args = Vec::new();
                    loop {
                        args.push(self.named_type()?);
                        if *self.current() != Kind::Comma { break; }
                        self.pos += 1;
                    }
                    self.take(Kind::RBracket)?;
                    let sid = self.instantiate_struct(gid, args, offset)?;
                    let def = &self.structs[sid];
                    self.require_public("struct", &def.name.clone(), &def.file.clone(), def.public)?;
                    return Ok(Type::Struct(sid));
                }
                match self.struct_names.get(other) {
                Some(&id) => {
                    let def = &self.structs[id];
                    self.require_public("struct", &def.name.clone(), &def.file.clone(), def.public)?;
                    Ok(Type::Struct(id))
                }
                None => match self.enum_names.get(other) {
                    Some(&id) => {
                        let (name, file, public) = (self.enums[id].name, self.enums[id].file.clone(), self.enums[id].public);
                        self.require_public("enum", name, &file, public)?;
                        Ok(Type::Enum(name, id))
                    }
                    None => Err(self.fail(&format!("unknown type '{}'", other))),
                },
                }
            }
        }
    }
    // `::Variant(payload, ...)` once the enum type is known. Payload values are
    // moved in, so a variant owns whatever it carries.
    pub(super) fn enum_literal(&mut self, eid: usize, offset: usize) -> Result<(Expr, Type), ae_span::Diagnostic> {
        self.take(Kind::ColonColon)?;
        let vname = self.identifier()?;
        let def = self.enums[eid].clone();
        self.require_public("enum", def.name, &def.file, def.public)?;
        let vidx = def.variants.iter().position(|(n, _)| *n == vname)
            .ok_or_else(|| self.error_at(offset, &format!("enum '{}' has no variant '{}'", def.name, vname)))?;
        let fields = def.variants[vidx].1.clone();
        let mut values = Vec::new();
        if *self.current() == Kind::LParen {
            self.pos += 1;
            for (index, field) in fields.iter().enumerate() {
                // String payloads are moved into the variant.
                let (value, value_ty) = self.expression(0, *field == Type::String)?;
                if value_ty != *field {
                    return Err(self.fail(&format!("payload {} of '{}::{}' must be {}", index + 1, def.name, vname, field.name())));
                }
                values.push(value);
                if index + 1 < fields.len() { self.take(Kind::Comma)?; }
            }
            if *self.current() == Kind::Comma {
                return Err(self.fail(&format!("variant '{}::{}' takes {} payload value(s)", def.name, vname, fields.len())));
            }
            self.take(Kind::RParen)?;
        } else if !fields.is_empty() {
            return Err(self.fail(&format!("variant '{}::{}' needs {} payload value(s)", def.name, vname, fields.len())));
        }
        Ok((Expr::EnumLit(def.name, eid, vidx, values), Type::Enum(def.name, eid)))
    }
    // `Name { field: expr, ... }` once the layout is known. Fields may appear
    // in any order but each exactly once; owned fields are moved in.
    pub(super) fn struct_literal(&mut self, sid: usize, offset: usize) -> Result<(Expr, Type), ae_span::Diagnostic> {
        let name = self.structs[sid].name.clone();
        self.take(Kind::LBrace)?;
        let def_fields = self.structs[sid].fields.clone();
        let mut slots: Vec<Option<Expr>> = (0..def_fields.len()).map(|_| None).collect();
        let mut seen = std::collections::HashSet::new();
        while *self.current() != Kind::RBrace {
            let fname = self.identifier()?;
            let fidx = def_fields.iter().position(|(n, _)| *n == fname)
                .ok_or_else(|| self.fail(&format!("struct '{}' has no field '{}'", name, fname)))?;
            if !seen.insert(fname.clone()) { return Err(self.fail(&format!("field '{}' specified twice", fname))); }
            self.take(Kind::Colon)?;
            let (value, vty) = self.expression(0, true)?;
            if vty != def_fields[fidx].1 { return Err(self.fail(&format!("field '{}' must be {}", fname, def_fields[fidx].1.name()))); }
            slots[fidx] = Some(value);
            if *self.current() == Kind::Comma { self.pos += 1; }
        }
        self.take(Kind::RBrace)?;
        let mut values = Vec::new();
        for (i, slot) in slots.into_iter().enumerate() {
            match slot {
                Some(v) => values.push(v),
                None => return Err(self.error_at(offset, &format!("missing field '{}' in '{}' construction", def_fields[i].0, name))),
            }
        }
        Ok((Expr::StructLit(sid, values), Type::Struct(sid)))
    }
    // `enum Name { ... }` or `enum Name[T, U] { ... }`. A generic declaration
    // produces no layout at all; only instantiations do (ADR-21).
    pub(super) fn parse_enum_def(&mut self, public: bool) -> Result<(), ae_span::Diagnostic> {
        self.pos += 1;
        let name = self.identifier()?;
        if RESERVED.contains(&name.as_str()) { return Err(self.fail(&format!("'{}' is a built-in name and cannot be an enum", name))); }
        if self.struct_names.contains_key(&name) || self.generics.struct_names.contains_key(&name) { return Err(self.fail(&format!("'{}' is already a struct", name))); }
        if self.enum_names.contains_key(&name) || self.generics.names.contains_key(&name) { return Err(self.fail(&format!("duplicate enum '{}'", name))); }
        let params = self.type_params()?;
        self.take(Kind::LBrace)?;
        let mut variants: Vec<(String, Vec<Payload>)> = Vec::new();
        let mut seen = std::collections::HashSet::new();
        while *self.current() != Kind::RBrace {
            let vname = self.identifier()?;
            if !seen.insert(vname.clone()) { return Err(self.fail(&format!("duplicate variant '{}' in enum '{}'", vname, name))); }
            let mut fields = Vec::new();
            if *self.current() == Kind::LParen {
                self.pos += 1;
                if *self.current() != Kind::RParen {
                    loop {
                        if fields.len() >= 16 { return Err(self.fail("variant payload limit exceeded (16)")); }
                        let fty = self.variant_payload(&params)?;
                        fields.push(fty);
                        if *self.current() != Kind::Comma { break; }
                        self.pos += 1;
                    }
                }
                self.take(Kind::RParen)?;
            }
            variants.push((vname, fields));
            if *self.current() == Kind::Comma { self.pos += 1; }
        }
        self.take(Kind::RBrace)?;
        if variants.is_empty() { return Err(self.fail(&format!("enum '{}' needs at least one variant", name))); }
        if variants.len() > 256 { return Err(self.fail("enum variant limit exceeded (256)")); }
        if params.is_empty() {
            let id = self.enums.len();
            // Type names are interned once per declaration so Type stays Copy and
            // diagnostics can print the declared name.
            let interned: &'static str = Box::leak(name.clone().into_boxed_str());
            self.enum_names.insert(name.clone(), id);
            let concrete = variants.into_iter().map(|(vname, fields)| (vname, fields.into_iter().map(|p| match p {
                Payload::Concrete(ty) => ty,
                Payload::Param(_) => unreachable!("a non-generic enum has no type parameters"),
            }).collect())).collect();
            self.enums.push(EnumDef { name: interned, variants: concrete, file: self.file.clone(), public, pattern: name });
        } else {
            let gid = self.generics.defs.len();
            self.generics.names.insert(name.clone(), gid);
            self.generics.defs.push(GenericEnum { name, params, variants, file: self.file.clone(), public });
        }
        Ok(())
    }
    // `[T, U]` after a declaration name; empty for a plain definition. Single
    // uppercase letters keep a parameter from being mistaken for a real type.
    // `[T, U]` or `[T: Add + Ord, U: Copy]`. Constraints are parsed here for
    // every generic declaration, but only generic functions may use them.
    pub(super) fn type_params_with_bounds(&mut self) -> Result<Vec<(String, Vec<Bound>, Vec<usize>)>, ae_span::Diagnostic> {
        let mut params: Vec<(String, Vec<Bound>, Vec<usize>)> = Vec::new();
        if *self.current() != Kind::LBracket { return Ok(params); }
        self.pos += 1;
        loop {
            if params.len() >= 8 { return Err(self.fail("type parameter limit exceeded (8)")); }
            let param = self.identifier()?;
            if param.len() != 1 || !param.chars().all(|c| c.is_ascii_uppercase()) { return Err(self.fail("type parameters are single uppercase letters, e.g. 'T'")); }
            if params.iter().any(|(n, _, _)| n == &param) { return Err(self.fail(&format!("duplicate type parameter '{param}'"))); }
            let mut bounds = Vec::new();
            let mut traits = Vec::new();
            if *self.current() == Kind::Colon {
                self.pos += 1;
                loop {
                    let at = self.tokens[self.pos].offset;
                    let label = self.identifier()?;
                    let bound = match label.as_str() {
                        "Add" => Some(Bound::Add), "Eq" => Some(Bound::Eq),
                        "Ord" => Some(Bound::Ord), "Copy" => Some(Bound::Copy),
                        _ => None,
                    };
                    match bound {
                        Some(bound) => {
                            if bounds.contains(&bound) { return Err(self.fail(&format!("duplicate constraint '{label}'"))); }
                            bounds.push(bound);
                        }
                        None => {
                            // A user-declared trait. Traits are collected
                            // before anything else in the file and imports
                            // load first, so the name is either here or
                            // nowhere; a private one from another file is
                            // rejected like a private function would be.
                            let tid = *self.generics.trait_names.get(&label).ok_or_else(|| {
                                self.error_at(at, "unknown constraint; allowed: Add, Eq, Ord, Copy, or a trait declared in this program")
                            })?;
                            let trait_file = self.generics.traits[tid].file.clone();
                            if trait_file != self.file {
                                self.require_public("trait", &label, &trait_file, self.generics.traits[tid].public).map_err(|e| self.error_at(at, &e.to_string()))?;
                            }
                            if traits.contains(&tid) { return Err(self.fail(&format!("duplicate constraint '{label}'"))); }
                            traits.push(tid);
                        }
                    }
                    if *self.current() != Kind::Plus { break; }
                    self.pos += 1;
                }
            }
            params.push((param, bounds, traits));
            if *self.current() != Kind::Comma { break; }
            self.pos += 1;
        }
        self.take(Kind::RBracket)?;
        Ok(params)
    }
    pub(super) fn type_params(&mut self) -> Result<Vec<String>, ae_span::Diagnostic> {
        let params = self.type_params_with_bounds()?;
        if params.iter().any(|(_, bounds, traits)| !bounds.is_empty() || !traits.is_empty()) {
            return Err(self.fail("constraints are only supported on generic functions"));
        }
        Ok(params.into_iter().map(|(name, _, _)| name).collect())
    }
    pub(super) fn variant_payload(&mut self, params: &[String]) -> Result<Payload, ae_span::Diagnostic> {
        if let Kind::Ident(name) = self.current() {
            if let Some(index) = params.iter().position(|p| p == name) {
                self.pos += 1;
                return Ok(Payload::Param(index));
            }
        }
        let fty = self.named_type()?;
        // Payloads stay flat: a variant may carry scalars, strings or a vector,
        // but not a struct or another enum (no layout recursion).
        if !matches!(fty, Type::Int | Type::Bool | Type::F64 | Type::String | Type::Vec | Type::StrVec | Type::VecF) {
            return Err(self.fail("enum variant payloads must be i64, bool, f64, string, vec, vec[string], or a type parameter"));
        }
        Ok(Payload::Concrete(fty))
    }
    // One C layout per (declaration, argument) pair, generated on first use. The
    // budget bounds code growth for a program that instantiates endlessly.
    pub(super) fn instantiate_enum(&mut self, gid: usize, args: Vec<Type>, offset: usize) -> Result<usize, ae_span::Diagnostic> {
        let def = self.generics.defs[gid].clone();
        if args.len() != def.params.len() {
            return Err(self.error_at(offset, &format!("'{}' takes {} type argument(s), found {}", def.name, def.params.len(), args.len())));
        }
        let key = format!("{gid}:{}", args.iter().map(|t| t.name()).collect::<Vec<_>>().join(","));
        if let Some(&eid) = self.generics.done.get(&key) { return Ok(eid); }
        if self.enums.len() >= MAX_ENUM_TYPES {
            return Err(self.error_at(offset, &format!("generic instantiation budget exceeded ({MAX_ENUM_TYPES} enum types) at '{}'", def.name)));
        }
        let variants: Vec<(String, Vec<Type>)> = def.variants.iter().map(|(vname, fields)| {
            let types = fields.iter().map(|p| match p {
                Payload::Concrete(ty) => *ty,
                Payload::Param(index) => args[*index],
            }).collect();
            (vname.clone(), types)
        }).collect();
        let display = format!("{}[{}]", def.name, args.iter().map(|t| t.name()).collect::<Vec<_>>().join(", "));
        let interned: &'static str = Box::leak(display.into_boxed_str());
        let eid = self.enums.len();
        self.enums.push(EnumDef { name: interned, variants, file: def.file, public: def.public, pattern: def.name });
        self.generics.done.insert(key, eid);
        Ok(eid)
    }
    // A concrete type argument. Structs and enums are rejected for the same
    // reason variant payloads are: no recursion and no hidden layout.
    pub(super) fn type_argument(&mut self) -> Result<Type, ae_span::Diagnostic> {
        let ty = self.named_type()?;
        if !matches!(ty, Type::Int | Type::Bool | Type::F64 | Type::String | Type::Vec | Type::StrVec | Type::VecF) {
            return Err(self.fail("type arguments must be i64, bool, f64, string, vec, or vec[string]"));
        }
        Ok(ty)
    }
    // A field type in a generic declaration: a type parameter, or any named
    // type (which may itself be an instantiation such as `Option[string]`).
    pub(super) fn field_type(&mut self, params: &[String]) -> Result<Payload, ae_span::Diagnostic> {
        if let Kind::Ident(name) = self.current() {
            if let Some(index) = params.iter().position(|p| p == name) {
                self.pos += 1;
                return Ok(Payload::Param(index));
            }
        }
        Ok(Payload::Concrete(self.named_type()?))
    }
    // One C layout per (declaration, argument) pair, generated on first use.
    pub(super) fn instantiate_struct(&mut self, gid: usize, args: Vec<Type>, offset: usize) -> Result<usize, ae_span::Diagnostic> {
        let def = self.generics.struct_defs[gid].clone();
        if args.len() != def.params.len() {
            return Err(self.error_at(offset, &format!("'{}' takes {} type argument(s), found {}", def.name, def.params.len(), args.len())));
        }
        let key = format!("{gid}:{}", args.iter().map(|t| t.name()).collect::<Vec<_>>().join(","));
        if let Some(&sid) = self.generics.struct_done.get(&key) { return Ok(sid); }
        if self.structs.len() >= MAX_STRUCT_TYPES {
            return Err(self.error_at(offset, &format!("generic instantiation budget exceeded ({MAX_STRUCT_TYPES} struct types) at '{}'", def.name)));
        }
        let fields: Vec<(String, Type)> = def.fields.iter().map(|(fname, payload)| {
            let ty = match payload {
                Payload::Concrete(ty) => *ty,
                Payload::Param(index) => args[*index],
            };
            (fname.clone(), ty)
        }).collect();
        let display = format!("{}[{}]", def.name, args.iter().map(|t| t.name()).collect::<Vec<_>>().join(", "));
        let sid = self.structs.len();
        self.structs.push(StructDef { name: display, fields, file: def.file, public: def.public });
        self.generics.struct_done.insert(key, sid);
        Ok(sid)
    }
    pub(super) fn parse_struct_def(&mut self, public: bool) -> Result<(), ae_span::Diagnostic> {
        self.pos += 1;
        let name = self.identifier()?;
        if RESERVED.contains(&name.as_str()) { return Err(self.fail(&format!("'{}' is a built-in name and cannot be a struct", name))); }
        if self.struct_names.contains_key(&name) || self.generics.struct_names.contains_key(&name) { return Err(self.fail(&format!("duplicate struct '{}'", name))); }
        if self.enum_names.contains_key(&name) || self.generics.names.contains_key(&name) { return Err(self.fail(&format!("'{}' is already an enum", name))); }
        let params = self.type_params()?;
        self.take(Kind::LBrace)?;
        let mut fields: Vec<(String, Payload)> = Vec::new();
        let mut seen = std::collections::HashSet::new();
        while *self.current() != Kind::RBrace {
            let fname = self.identifier()?;
            if !seen.insert(fname.clone()) { return Err(self.fail(&format!("duplicate field '{}' in struct '{}'", fname, name))); }
            self.take(Kind::Colon)?;
            let fty = self.field_type(&params)?;
            fields.push((fname, fty));
            if *self.current() == Kind::Comma { self.pos += 1; }
        }
        self.take(Kind::RBrace)?;
        if params.is_empty() {
            let id = self.structs.len();
            self.struct_names.insert(name.clone(), id);
            let concrete = fields.into_iter().map(|(fname, payload)| (fname, match payload {
                Payload::Concrete(ty) => ty,
                Payload::Param(_) => unreachable!("a non-generic struct has no type parameters"),
            })).collect();
            self.structs.push(StructDef { name, fields: concrete, file: self.file.clone(), public });
        } else {
            let gid = self.generics.struct_defs.len();
            self.generics.struct_names.insert(name.clone(), gid);
            self.generics.struct_defs.push(GenericStruct { name, params, fields, file: self.file.clone(), public });
        }
        Ok(())
    }
    // Arrays are borrowed read-only; strings and scalars pass by value.
    pub(super) fn parameter_type(&mut self) -> Result<Type, ae_span::Diagnostic> {
        if *self.current() == Kind::Amp {
            let amp_offset = self.tokens[self.pos].offset;
            self.pos += 1;
            // `&T` is resolved when a generic function is instantiated: the
            // type parameter already stands for one concrete type, so this is
            // just a spelling for one of the three borrows below. No lifetime
            // or new borrow kind is introduced (ADR-35).
            let parameter: Option<(String, Type)> = match self.current() {
                Kind::Ident(name) => self.subst.get(name).map(|&ty| (name.clone(), ty)),
                _ => None,
            };
            if let Some((name, concrete)) = parameter {
                self.pos += 1;
                return match concrete {
                    Type::String => Ok(Type::StrView),
                    Type::Vec => Ok(Type::VecRef),
                    Type::StrVec => Ok(Type::StrVecRef),
                    Type::VecF => Ok(Type::VecFRef),
                    _ => Err(self.error_at(amp_offset, &format!(
                        "'&{name}' is '{}' here, which has no read-only borrow; only string, vec, vec[string] and vec[f64] can be borrowed",
                        concrete.name()))),
                };
            }
            if matches!(self.current(), Kind::Ident(name) if name == "string") {
                // &string borrows a string read-only for the duration of the
                // call: no move, no copy, and it binds as an ordinary view.
                self.pos += 1;
                return Ok(Type::StrView);
            }
            if matches!(self.current(), Kind::Ident(name) if name == "vec") {
                self.pos += 1;
                // &vec[string] and &vec[f64] (ADR-61) borrow those vectors
                // read-only.
                if *self.current() == Kind::LBracket {
                    self.pos += 1;
                    let element = self.identifier()?;
                    self.take(Kind::RBracket)?;
                    match element.as_str() {
                        "string" => return Ok(Type::StrVecRef),
                        "f64" => return Ok(Type::VecFRef),
                        _ => return Err(self.fail("only &vec[string] and &vec[f64] are supported; vector elements are i64 by default")),
                    }
                }
                return Ok(Type::VecRef);
            }
            if matches!(self.current(), Kind::Ident(_)) {
                return Err(self.error_at(amp_offset,
                    "unsupported borrow type; write &string, &vec, &vec[string], &vec[f64], &[i64; N], or &T inside a generic function"));
            }
            self.take(Kind::LBracket)?;
            if self.identifier()? != "i64" { return Err(self.fail("array element type must be i64")); }
            self.take(Kind::Semi)?;
            let len = match self.current() {
                Kind::Number(n) if *n >= 1 && *n <= 4096 => *n as usize,
                _ => return Err(self.fail("array parameter length must be between 1 and 4096")),
            };
            self.pos += 1; self.take(Kind::RBracket)?;
            Ok(Type::Array(len))
        } else {
            self.named_type()
        }
    }
    pub(super) fn return_annotation(&mut self) -> Result<Type, ae_span::Diagnostic> {
        self.named_type()
    }
    // First pass collects only signatures. Bodies are checked in the second pass.
    // scan_uses validated the headers; skip them before collecting.
    pub(super) fn skip_uses(&mut self) {
        while *self.current() == Kind::Use { self.pos += 3; }
    }
    // `pub` marks an item as importable from other files. It is consumed by the
    // declaration passes, never by expression parsing.
    pub(super) fn take_pub(&mut self) -> bool {
        let is_pub = matches!(self.current(), Kind::Ident(name) if name == "pub");
        if is_pub { self.pos += 1; }
        is_pub
    }
    // Cross-file access check: an item defined in another file must be exported.
    // Prelude names are public by construction, and a file always sees its own.
    pub(super) fn check_export_types(&self, name: &str, params: &[Type], ret: Type) -> Result<(), ae_span::Diagnostic> {
        for ty in params.iter().copied().chain(std::iter::once(ret)) {
            let (kind, type_name, public) = match ty {
                Type::Struct(id) => ("struct", self.structs[id].name.clone(), self.structs[id].public),
                Type::Enum(_, id) => ("enum", self.enums[id].name.to_owned(), self.enums[id].public),
                _ => continue,
            };
            if !public { return Err(self.fail(&format!("public function '{name}' exposes private {kind} '{type_name}'"))); }
        }
        Ok(())
    }
    pub(super) fn require_public(&self, kind: &str, name: &str, file: &str, public: bool) -> Result<(), ae_span::Diagnostic> {
        if public || file == PRELUDE || file == self.file { return Ok(()); }
        Err(self.fail(&format!("{kind} '{name}' is private to {file} (mark it 'pub' to export it)")))
    }
    // Second pass skips struct bodies (already collected, must not re-register).
    pub(super) fn skip_struct_def(&mut self) {
        self.pos += 2; // 'struct' Name
        self.skip_type_params();
        self.pos += 1; // the opening brace
        let mut depth = 1usize;
        while depth > 0 {
            match self.current() {
                Kind::LBrace => depth += 1,
                Kind::RBrace => depth -= 1,
                Kind::Eof => return,
                _ => {}
            }
            self.pos += 1;
        }
    }
    // Enum bodies are collected in the first pass, skipped in the second.
    // Step over `[T, U]` when a declaration is skipped in the second pass.
    pub(super) fn skip_type_params(&mut self) {
        if *self.current() != Kind::LBracket { return; }
        let mut depth = 0usize;
        while *self.current() != Kind::Eof {
            match self.current() {
                Kind::LBracket => depth += 1,
                Kind::RBracket => { depth -= 1; if depth == 0 { self.pos += 1; return; } }
                _ => {}
            }
            self.pos += 1;
        }
    }
    // Skip `(params) -> Ret { body }` without interpreting it. Used for a
    // generic declaration, whose body is parsed at each instantiation instead.
    pub(super) fn skip_function_body(&mut self) -> Result<usize, ae_span::Diagnostic> {
        while *self.current() != Kind::LBrace {
            if *self.current() == Kind::Eof { return Err(self.fail("expected '{' before end of file")); }
            self.pos += 1;
        }
        self.pos += 1;
        let mut depth = 1usize;
        while depth > 0 {
            match self.current() {
                Kind::LBrace => depth += 1,
                Kind::RBrace => depth -= 1,
                Kind::Eof => return Err(self.fail("expected '}' before end of file")),
                _ => {}
            }
            self.pos += 1;
        }
        Ok(self.pos)
    }
    pub(super) fn skip_enum_def(&mut self) {
        self.pos += 2; // 'enum' Name
        self.skip_type_params();
        self.pos += 1; // the opening brace
        let mut depth = 1usize;
        while depth > 0 {
            match self.current() {
                Kind::LBrace => depth += 1,
                Kind::RBrace => depth -= 1,
                Kind::Eof => return,
                _ => {}
            }
            self.pos += 1;
        }
    }
    pub(super) fn collect_functions(&mut self, next_id: &mut usize) -> Result<(), ae_span::Diagnostic> {
        // Traits are registered before anything else so a bound in the same
        // file can name a trait declared later; cross-file traits already
        // exist because imports load first. The scan walks tokens directly:
        // a `{` inside a string literal is part of its Text token, so brace
        // depth over tokens is exact.
        self.pos = 0;
        let mut depth = 0usize;
        let mut i = 0;
        while i < self.tokens.len() - 1 {
            match self.tokens[i].kind {
                Kind::LBrace => depth += 1,
                // The scan runs before anything validates the file, so a
                // malformed `}` at depth zero must not panic (the
                // malformed-input test feeds exactly such files).
                Kind::RBrace => depth = depth.saturating_sub(1),
                Kind::Trait if depth == 0 => {
                    self.pos = i;
                    self.parse_trait_def()?;
                    i = self.pos;
                    continue;
                }
                _ => {}
            }
            i += 1;
        }
        self.pos = 0;
        self.skip_uses();
        let mut seen = std::collections::HashSet::new();
        while *self.current() != Kind::Eof {
            let public = self.take_pub();
            if *self.current() == Kind::Struct { self.parse_struct_def(public)?; continue; }
            if *self.current() == Kind::Enum { self.parse_enum_def(public)?; continue; }
            if *self.current() == Kind::Trait { self.skip_trait_def()?; continue; }
            if *self.current() == Kind::Impl { self.collect_impl(next_id)?; continue; }
            self.take(Kind::Fn)?;
            let name = self.identifier()?;
            if !seen.insert(name.clone()) { return Err(self.fail(&format!("duplicate function '{name}'"))); }
            if *self.current() == Kind::LBracket {
                // A generic function has no signature of its own: only its
                // instantiations exist, and each gets an id when constructed.
                if name == "main" { return Err(self.fail("main cannot be generic")); }
                let declared_at = self.tokens[self.pos].offset;
                let declared = self.type_params_with_bounds()?;
                let params: Vec<String> = declared.iter().map(|(n, _, _)| n.clone()).collect();
                let bounds: Vec<Vec<Bound>> = declared.iter().map(|(_, b, _)| b.clone()).collect();
                let trait_bounds: Vec<Vec<usize>> = declared.into_iter().map(|(_, _, t)| t).collect();
                let start = self.pos;
                let end = self.skip_function_body()?;
                audit_generic_fn(&self.file, self.source, self.tokens.as_ref(), start, end, &name, &params, &bounds, declared_at)?;
                let gid = self.generics.fn_defs.len();
                self.generics.fn_names.insert(name.clone(), gid);
                self.generics.fn_defs.push(GenericFn { name, params, bounds, trait_bounds, start, tokens: Rc::clone(&self.tokens), source: Rc::clone(&self.shared_source), file: self.file.clone(), public });
                continue;
            }
            self.take(Kind::LParen)?;
            let mut types = Vec::new();
            if *self.current() != Kind::RParen {
                loop {
                    if types.len() >= 64 { return Err(self.fail("function parameter limit exceeded (64)")); }
                    if *self.current() == Kind::Var { self.pos += 1; }
                    let parameter = self.identifier()?;
                    if RESERVED.contains(&parameter.as_str()) { return Err(self.fail(&format!("'{parameter}' is a built-in name and cannot be a parameter"))); }
                    self.take(Kind::Colon)?;
                    types.push(self.parameter_type()?);
                    if *self.current() != Kind::Comma { break; }
                    self.pos += 1;
                }
            }
            self.take(Kind::RParen)?;
            if RESERVED.contains(&name.as_str()) { return Err(self.fail(&format!("'{name}' is a built-in and cannot be redefined"))); }
            // Ids are unique across all files of a program, and share one
            // space with generic instantiations, so keep the counter in the
            // shared state and fold the per-file value in.
            self.generics.next_fn_id = self.generics.next_fn_id.max(*next_id);
            let id = self.generics.next_fn_id;
            self.generics.next_fn_id += 1;
            *next_id = self.generics.next_fn_id;
            self.id_order.push(id);
            if name == "main" {
                if !types.is_empty() { return Err(self.fail("main cannot have parameters")); }
            } else {
                self.take(Kind::Arrow)?;
                let ret = self.return_annotation()?;
                // A public signature may not name a type importers cannot see;
                // catching it here beats a puzzling error at every call site.
                if public { self.check_export_types(&name, &types, ret)?; }
                let signature = Signature { id, params: types, ret, file: self.file.clone(), public };
                if self.functions.insert(name.clone(), signature).is_some() {
                    return Err(self.fail(&format!("duplicate function '{name}' (already defined in an imported file)")));
                }
            }
            self.take(Kind::LBrace)?;
            let mut depth = 1usize;
            while depth > 0 {
                match self.current() {
                    Kind::LBrace => depth += 1,
                    Kind::RBrace => depth -= 1,
                    Kind::Eof => return Err(self.fail("expected '}' before end of file")),
                    _ => {}
                }
                self.pos += 1;
            }
        }
        // main presence is checked once per program, not per file.
        self.pos = 0;
        self.skip_uses();
        Ok(())
    }
    // ---- traits and impls (ADR-47) ------------------------------------------
    // A trait method is only callable inside a generic body whose constrained
    // parameter it dispatches on: resolution maps (trait, method, substituted
    // parameter type) to the impl's mangled signature, and the call emits the
    // impl function's id like any other call — static dispatch with no new
    // runtime mechanism.

    // Parses a trait's payload type: the trait's own `T`, or a concrete type.
    pub(super) fn trait_payload(&mut self, what: &str) -> Result<Payload, ae_span::Diagnostic> {
        if matches!(self.current(), Kind::Amp) {
            return Err(self.fail(&format!("{what} cannot borrow T in this version; take T by value or use a concrete type")));
        }
        if let Kind::Ident(text) = self.current() {
            if text == "T" {
                self.pos += 1;
                return Ok(Payload::Param(0));
            }
        }
        Ok(Payload::Concrete(self.parameter_type()?))
    }

    pub(super) fn parse_trait_def(&mut self) -> Result<(), ae_span::Diagnostic> {
        // The token scan landed on the `trait` keyword; a `pub` sits directly
        // before it (`pub` is an identifier-shaped keyword consumed by the
        // declaration passes only).
        let public = self.pos > 0 && matches!(&self.tokens[self.pos - 1].kind, Kind::Ident(name) if name == "pub");
        self.pos += 1; // trait
        let name = self.identifier()?;
        if self.generics.trait_names.contains_key(&name) {
            return Err(self.fail(&format!("duplicate trait '{name}'")));
        }
        if self.generics.traits.len() >= 64 { return Err(self.fail("trait limit exceeded (64 per program)")); }
        self.take(Kind::LBrace)?;
        let mut methods: Vec<TraitMethod> = Vec::new();
        while *self.current() != Kind::RBrace {
            if *self.current() == Kind::Eof { return Err(self.fail("expected '}' before end of file")); }
            self.take(Kind::Fn)?;
            let method = self.identifier()?;
            if method == "main" { return Err(self.fail("a trait method cannot be named main")); }
            if methods.iter().any(|m: &TraitMethod| m.name == method) {
                return Err(self.fail(&format!("duplicate method '{method}' in trait '{name}'")));
            }
            self.take(Kind::LParen)?;
            let mut params = Vec::new();
            if *self.current() != Kind::RParen {
                loop {
                    if params.len() >= 8 { return Err(self.fail("trait method parameter limit exceeded (8)")); }
                    let _param = self.identifier()?;
                    self.take(Kind::Colon)?;
                    params.push(self.trait_payload("trait method parameter")?);
                    if *self.current() != Kind::Comma { break; }
                    self.pos += 1;
                }
            }
            self.take(Kind::RParen)?;
            if params.is_empty() || !matches!(params[0], Payload::Param(0)) {
                return Err(self.fail("the first parameter of a trait method must be the trait's type T"));
            }
            self.take(Kind::Arrow)?;
            let returns = self.trait_payload("trait method return")?;
            self.take(Kind::Semi)?;
            methods.push(TraitMethod { name: method, params, returns });
        }
        self.take(Kind::RBrace)?;
        let tid = self.generics.traits.len();
        self.generics.trait_names.insert(name.clone(), tid);
        self.generics.traits.push(TraitDef { name, methods, file: self.file.clone(), public });
        Ok(())
    }

    pub(super) fn skip_trait_def(&mut self) -> Result<(), ae_span::Diagnostic> {
        while *self.current() != Kind::LBrace {
            if *self.current() == Kind::Eof { return Err(self.fail("expected '{' in trait declaration")); }
            self.pos += 1;
        }
        self.skip_balanced_braces()
    }

    pub(super) fn skip_balanced_braces(&mut self) -> Result<(), ae_span::Diagnostic> {
        self.take(Kind::LBrace)?;
        let mut depth = 1usize;
        while depth > 0 {
            match self.current() {
                Kind::LBrace => depth += 1,
                Kind::RBrace => depth -= 1,
                Kind::Eof => return Err(self.fail("expected '}' before end of file")),
                _ => {}
            }
            self.pos += 1;
        }
        Ok(())
    }

    // Pass one for `impl Trait for Type { fn ... }`: register each method's
    // mangled signature and assign its id, so pass two (program) can parse
    // the bodies in file order against `id_order`.
    pub(super) fn collect_impl(&mut self, next_id: &mut usize) -> Result<(), ae_span::Diagnostic> {
        self.pos += 1; // impl
        let trait_name = self.identifier()?;
        let tid = *self.generics.trait_names.get(&trait_name)
            .ok_or_else(|| self.fail(&format!("impl names unknown trait '{trait_name}'")))?;
        self.take(Kind::For)?;
        if let Kind::Ident(text) = self.current() {
            if text == "T" { return Err(self.fail("an impl names the concrete type, not the trait's parameter T")); }
        }
        let ty = self.parameter_type()?;
        let trait_file = self.generics.traits[tid].file.clone();
        let type_file = match ty {
            Type::Struct(id) => Some(self.structs[id].file.clone()),
            Type::Enum(_, id) => Some(self.enums[id].file.clone()),
            _ => None,
        };
        // Coherence, v1 shape: the impl lives where the trait or the type
        // lives, so two files can never hand the same (trait, type) pair
        // conflicting behaviour.
        if trait_file != self.file && type_file.as_deref() != Some(self.file.as_str()) {
            let where_must = type_file.unwrap_or_else(|| format!("the file declaring the trait ({trait_file})"));
            return Err(self.fail(&format!(
                "impl of '{trait_name}' for '{}' must be in the file that declares the trait ({trait_file}) or the type ({}); this file may not add it",
                ty.name(), where_must)));
        }
        if !self.generics.impls.insert((tid, ty.name())) {
            return Err(self.fail(&format!("duplicate impl of '{trait_name}' for '{}'", ty.name())));
        }
        if self.generics.impls.len() > 256 { return Err(self.fail("impl limit exceeded (256 per program)")); }
        self.take(Kind::LBrace)?;
        while *self.current() != Kind::RBrace {
            if *self.current() == Kind::Eof { return Err(self.fail("expected '}' before end of file")); }
            self.take(Kind::Fn)?;
            let method = self.identifier()?;
            if method == "main" { return Err(self.fail("an impl method cannot be named main")); }
            let mid = self.generics.traits[tid].methods.iter().position(|m| m.name == method)
                .ok_or_else(|| self.fail(&format!("trait '{trait_name}' has no method '{method}'")))?;
            let expected = self.generics.traits[tid].methods[mid].clone();
            let substitute = |payload: &Payload| match payload {
                Payload::Param(0) => ty,
                Payload::Concrete(t) => *t,
                _ => unreachable!("trait payloads only use parameter 0"),
            };
            self.take(Kind::LParen)?;
            let mut types = Vec::new();
            if *self.current() != Kind::RParen {
                loop {
                    if types.len() >= 8 { return Err(self.fail("impl method parameter limit exceeded (8)")); }
                    if let Kind::Ident(text) = self.current() {
                        if text == "T" { return Err(self.fail("an impl method signature uses the concrete type, not the trait's parameter T")); }
                    }
                    let _param = self.identifier()?;
                    self.take(Kind::Colon)?;
                    types.push(self.parameter_type()?);
                    if *self.current() != Kind::Comma { break; }
                    self.pos += 1;
                }
            }
            self.take(Kind::RParen)?;
            let expected_params: Vec<Type> = expected.params.iter().map(substitute).collect();
            if types != expected_params {
                let want = expected_params.iter().map(|t| t.name()).collect::<Vec<_>>().join(", ");
                let got = types.iter().map(|t| t.name()).collect::<Vec<_>>().join(", ");
                return Err(self.fail(&format!("impl method '{method}' must take ({want}) to match trait '{trait_name}', found ({got})")));
            }
            self.take(Kind::Arrow)?;
            let ret = self.parameter_type()?;
            if ret != substitute(&expected.returns) {
                return Err(self.fail(&format!("impl method '{method}' must return {} to match trait '{trait_name}', found {}", substitute(&expected.returns).name(), ret.name())));
            }
            self.generics.next_fn_id = self.generics.next_fn_id.max(*next_id);
            let id = self.generics.next_fn_id;
            self.generics.next_fn_id += 1;
            *next_id = self.generics.next_fn_id;
            self.id_order.push(id);
            let mangled = format!("{}#{}#{}", trait_name, method, ty.name());
            let signature = Signature { id, params: types, ret, file: self.file.clone(), public: true };
            if self.functions.insert(mangled, signature).is_some() {
                return Err(self.fail(&format!("duplicate impl method '{method}' for this type")));
            }
            self.skip_balanced_braces()?;
        }
        self.take(Kind::RBrace)
    }

    // Pass two: parse the impl method bodies in the same file order the
    // signature pass used, so `id_order` lines up exactly like it does for
    // the file's own functions.
    pub(super) fn parse_impl_bodies(&mut self, result: &mut Vec<Function>) -> Result<(), ae_span::Diagnostic> {
        self.pos += 1; // impl
        self.identifier()?;
        self.take(Kind::For)?;
        self.parameter_type()?;
        self.take(Kind::LBrace)?;
        while *self.current() != Kind::RBrace {
            if *self.current() == Kind::Eof { return Err(self.fail("expected '}' before end of file")); }
            self.take(Kind::Fn)?;
            let _method = self.identifier()?;
            let id = self.id_order[result.len()];
            result.push(self.function_rest(false, id)?);
        }
        self.take(Kind::RBrace)
    }

    // Resolves a call that is not a plain function: a trait method of one of
    // the enclosing generic's constrained parameters. The dispatch type is
    // the parameter's substituted type, fixed when the generic was
    // instantiated, so the impl lookup cannot be ambiguous at runtime.
    pub(super) fn resolve_trait_call(&mut self, method: &str, offset: usize) -> Result<Signature, ae_span::Diagnostic> {
        let mut found: Vec<(String, usize, usize)> = Vec::new();
        for (param, tids) in &self.trait_bounds {
            for &tid in tids {
                if let Some(mid) = self.generics.traits[tid].methods.iter().position(|m| m.name == method) {
                    found.push((param.clone(), tid, mid));
                }
            }
        }
        if found.is_empty() {
            return Err(self.error_at(offset, &format!("unknown function '{method}'")));
        }
        if found.len() > 1 {
            return Err(self.error_at(offset, &format!("method '{method}' is ambiguous; several constrained parameters of this generic declare it")));
        }
        let (param, tid, mid) = found.remove(0);
        let ty = self.subst.get(&param).cloned()
            .ok_or_else(|| self.error_at(offset, &format!("'{method}' dispatches on '{param}', which has no type argument here")))?;
        let key = format!("{}#{}#{}", self.generics.traits[tid].name, self.generics.traits[tid].methods[mid].name, ty.name());
        self.functions.get(&key).cloned().ok_or_else(|| {
            self.error_at(offset, &format!("no impl of trait '{}' for '{}'", self.generics.traits[tid].name, ty.name()))
        })
    }

    pub(super) fn program(&mut self) -> Result<Vec<Function>, ae_span::Diagnostic> {
        let mut result = Vec::new();
        let mut main_found = false;
        while *self.current() != Kind::Eof {
            self.take_pub();
            if *self.current() == Kind::Struct { self.skip_struct_def(); continue; }
            if *self.current() == Kind::Enum { self.skip_enum_def(); continue; }
            if *self.current() == Kind::Trait { self.skip_trait_def()?; continue; }
            if *self.current() == Kind::Impl { self.parse_impl_bodies(&mut result)?; continue; }
            self.take(Kind::Fn)?;
            let name = self.identifier()?;
            // A generic declaration has no body of its own: the signature pass
            // recorded it and an instantiation parses it later (ADR-31).
            if *self.current() == Kind::LBracket { self.skip_function_body()?; continue; }
            let is_main = name == "main";
            if is_main && main_found { return Err(self.fail("duplicate main function")); }
            if is_main { main_found = true; }
            let id = self.id_order[result.len()];
            result.push(self.function_rest(is_main, id)?);
        }
        // Instantiations discovered while parsing bodies have no place in the
        // file's own order, but they must still be emitted.
        result.append(&mut self.pending);
        // main presence is checked once per program, not per file.
        Ok(result)
    }
    // `(params) -> Ret { body }` for one function, starting at the parameter
    // list. Shared by the file's own functions and by generic instantiations,
    // which parse the same tokens with type parameters substituted.
    pub(super) fn function_rest(&mut self, is_main: bool, id: usize) -> Result<Function, ae_span::Diagnostic> {
        self.take(Kind::LParen)?;
        // Variable ids restart per function, so move tracking must too.
        self.names.clear(); self.binding_ranges.clear(); self.next_var = 0; self.moved.clear(); self.scopes.clear(); self.borrows.clear();
        let mut params = Vec::new();
        if *self.current() != Kind::RParen {
            loop {
                if params.len() >= 64 { return Err(self.fail("function parameter limit exceeded (64)")); }
                // `var` marks the parameter slot writable. The value was moved
                // in at the call site, so mutating this copy cannot be seen by
                // the caller; that difference is what keeps it safe.
                let mutable = *self.current() == Kind::Var;
                if mutable { self.pos += 1; }
                let binding_range = self.tokens[self.pos].offset..self.tokens[self.pos].end;
                let param = self.identifier()?;
                if RESERVED.contains(&param.as_str()) { return Err(self.fail(&format!("'{param}' is a built-in name and cannot be a parameter"))); }
                if self.names.contains_key(&param) { return Err(self.fail(&format!("duplicate parameter '{param}'"))); }
                self.take(Kind::Colon)?;
                let ty = self.parameter_type()?;
                let var_id = self.next_var; self.next_var += 1;
                self.names.insert(param, (var_id, mutable, ty));
                self.binding_ranges.insert(var_id, binding_range);
                params.push((var_id, ty));
                if *self.current() != Kind::Comma { break; }
                self.pos += 1;
            }
        }
        self.take(Kind::RParen)?;
        let returns_type;
        if is_main {
            if !params.is_empty() { return Err(self.fail("main cannot have parameters")); }
            returns_type = Type::Int;
        } else {
            self.take(Kind::Arrow)?;
            returns_type = self.return_annotation()?;
        }
        self.in_main = is_main;
        self.return_type = returns_type;
        // Parameter scope: lives for the whole body so return paths can
        // release owned parameters.
        self.scopes.push(params.iter().map(|&(var_id, ty)| (var_id, ty)).collect());
        let body = self.block();
        let params_scope = self.scopes.pop().unwrap();
        let mut body = body?;
        let param_owned: Vec<(usize, Type)> = params_scope.iter().filter(|(_, ty)| ty.owned()).copied().collect();
        if !param_owned.is_empty() { body.push(Statement::Frees(param_owned)); }
        if !is_main && !returns(&body) { return Err(self.fail("function may exit without returning a value")); }
        Ok(Function { id, params, body, is_main, returns: returns_type })
    }
    // Parse a generic function's own tokens once per argument list. The
    // sub-parser shares this program's maps and id space, so a nested
    // instantiation behaves exactly like one written at the top level.
    pub(super) fn instantiate_fn(&mut self, gid: usize, args: Vec<Type>, offset: usize) -> Result<Instance, ae_span::Diagnostic> {
        let def = self.generics.fn_defs[gid].clone();
        if args.len() != def.params.len() {
            return Err(self.error_at(offset, &format!("'{}' takes {} type argument(s), found {}", def.name, def.params.len(), args.len())));
        }
        self.require_public("function", &def.name, &def.file, def.public)?;
        for (index, bounds) in def.bounds.iter().enumerate() {
            for &bound in bounds {
                if !bound.allows(args[index]) {
                    return Err(self.error_at(offset, &format!(
                        "type argument '{}' for '{}' does not satisfy '{}'; that constraint allows {}",
                        args[index].name(), def.name, bound.name(), bound.allowed_types())));
                }
            }
        }
        for (index, traits) in def.trait_bounds.iter().enumerate() {
            for &tid in traits {
                let trait_name = self.generics.traits[tid].name.clone();
                if !self.generics.impls.contains(&(tid, args[index].name())) {
                    return Err(self.error_at(offset, &format!(
                        "no impl of trait '{}' for '{}'; required by '{}'",
                        trait_name, args[index].name(), def.name)));
                }
            }
        }
        let key = format!("{gid}:{}", args.iter().map(|t| t.name()).collect::<Vec<_>>().join(","));
        if let Some(instance) = self.generics.fn_done.get(&key) { return Ok(instance.clone()); }
        if self.generics.fn_in_progress.contains(&key) {
            return Err(self.error_at(offset, &format!("'{}' is already being instantiated with these type arguments; the expansion would never finish", def.name)));
        }
        self.generics.fn_in_progress.insert(key.clone());
        let id = self.generics.next_fn_id;
        self.generics.next_fn_id += 1;
        let mut subst = HashMap::new();
        let mut trait_bounds = HashMap::new();
        for (index, param) in def.params.iter().enumerate() {
            subst.insert(param.clone(), args[index]);
            trait_bounds.insert(param.clone(), def.trait_bounds[index].clone());
        }
        let mut sub = Parser {
            file: def.file.clone(), source: &*def.source, tokens: Rc::clone(&def.tokens), shared_source: Rc::clone(&def.source), pos: def.start,
            names: HashMap::new(), binding_ranges: HashMap::new(), next_var: 0, expression_depth: 0, expression_nodes: 0, block_depth: 0,
            loop_depth: 0, loop_scopes: Vec::new(), loop_labels: Vec::new(), next_label: 0,
            functions: &mut *self.functions, structs: &mut *self.structs, struct_names: &mut *self.struct_names,
            enums: &mut *self.enums, enum_names: &mut *self.enum_names, generics: &mut *self.generics,
            id_order: Vec::new(), in_main: false, return_type: Type::Int, moved: std::collections::HashSet::new(),
            borrows: Vec::new(), scopes: Vec::new(), subst, trait_bounds, pending: Vec::new(),
        };
        let parsed = sub.function_rest(false, id);
        let mut nested = std::mem::take(&mut sub.pending);
        drop(sub);
        let function = parsed?;
        let instance: Instance = (id, function.params.iter().map(|&(_, ty)| ty).collect(), function.returns);
        self.pending.push(function);
        self.pending.append(&mut nested);
        self.generics.fn_done.insert(key.clone(), instance.clone());
        self.generics.fn_in_progress.remove(&key);
        Ok(instance)
    }
    // The argument list of a call, starting at '(' (ADR-31 shares it between
    // ordinary calls and generic instantiations).
    pub(super) fn call_arguments(&mut self, name: &str, types: &[Type]) -> Result<Vec<Argument>, ae_span::Diagnostic> {
        self.pos += 1;
        let mut args = Vec::new();
        if *self.current() != Kind::RParen {
            loop {
                if args.len() >= 64 { return Err(self.fail("function argument limit exceeded (64)")); }
                let expected = *types.get(args.len()).ok_or_else(|| self.fail(&format!("too many arguments to '{name}'")))?;
                if expected == Type::StrView {
                    let (value, ty) = self.expression(0, false)?;
                    if !matches!(ty, Type::String | Type::StrView) {
                        return Err(self.fail(&format!("argument must be a string or string view, found {}", ty.name())));
                    }
                    args.push(Argument::View(value));
                } else if expected == Type::VecRef || expected == Type::StrVecRef || expected == Type::VecFRef {
                    self.take(Kind::Amp)?;
                    let arg = self.identifier()?;
                    let (var, _, actual) = *self.names.get(&arg).ok_or_else(|| self.fail(&format!("unknown variable '{arg}'")).with_code("E0101"))?;
                    let owned_shape = match expected {
                        Type::VecRef => Type::Vec,
                        Type::StrVecRef => Type::StrVec,
                        _ => Type::VecF,
                    };
                    let borrowed_shape = match expected {
                        Type::VecRef => Type::VecRef,
                        Type::StrVecRef => Type::StrVecRef,
                        _ => Type::VecFRef,
                    };
                    if actual != owned_shape && actual != borrowed_shape {
                        return Err(self.fail(&format!("argument must be {}, found {}", owned_shape.name(), actual.name())));
                    }
                    if self.moved.contains(&var) { return Err(self.binding_label(self.fail(&format!("cannot borrow '{arg}' because it was moved")).with_code("E0102"), var)); }
                    // An owned vector needs its address taken; a parameter that
                    // is already `const ae_vec *` is forwarded unchanged.
                    // The flag means "take the address", not "is a vector".
                    args.push(Argument::Borrow(var, actual == owned_shape));
                } else if let Type::Array(len) = expected {
                    self.take(Kind::Amp)?;
                    let arg = self.identifier()?;
                    let (var, _, actual) = *self.names.get(&arg).ok_or_else(|| self.fail(&format!("unknown array '{arg}'")))?;
                    if actual != Type::Array(len) { return Err(self.fail(&format!("argument requires array of length {len}"))); }
                    args.push(Argument::Borrow(var, false));
                } else {
                    // By-value arguments always consume their value:
                    // the callee releases the parameter, so the caller
                    // must not use the slot afterwards even when the
                    // surrounding statement is not a move context.
                    let (value, ty) = self.expression(0, true)?;
                    if ty != expected { return Err(self.fail(&format!("argument {} to '{name}' must be {}", args.len() + 1, expected.name()))); }
                    args.push(Argument::Value(value));
                }
                if *self.current() != Kind::Comma { break; }
                self.pos += 1;
            }
        }
        self.take(Kind::RParen)?;
        if args.len() != types.len() { return Err(self.fail(&format!("function '{name}' expects {} arguments, got {}", types.len(), args.len()))); }
        Ok(args)
    }
    pub(super) fn block(&mut self) -> Result<Vec<Statement>, ae_span::Diagnostic> {
        if self.block_depth >= 64 { return Err(self.fail("block nesting limit exceeded (64)")); }
        self.block_depth += 1;
        self.scopes.push(Vec::new());
        let result = self.block_inner();
        let scope = self.scopes.pop().unwrap();
        self.block_depth -= 1;
        let mut result = result?;
        // Owned values declared in this block are released at its end.
        // Releases are idempotent (slots are zeroed), so overlapping paths
        // stay safe.
        let owned: Vec<(usize, Type)> = scope.iter().filter(|(_, ty)| ty.owned()).copied().collect();
        if !owned.is_empty() { result.push(Statement::Frees(owned)); }
        // Moved markers for names that left scope are no longer meaningful,
        // and borrows recorded in this scope end with it.
        let live: std::collections::HashSet<usize> = self.names.values().map(|&(id, _, _)| id).collect();
        self.moved.retain(|id| live.contains(id));
        let depth_now = self.scopes.len();
        self.borrows.retain(|&(_, depth)| depth <= depth_now);
        Ok(result)
    }
    // Views borrow their base: element reads and slice_views of variable
    // bases register the owner so it cannot be modified or moved while a
    // view can still be read.
    pub(super) fn record_borrow_bases(&mut self, e: &Expr) {
        match e {
            Expr::StrVecIndex(id, idx, _, _, _) => {
                if !self.borrows.iter().any(|&(bid, _)| bid == *id) {
                    self.borrows.push((*id, self.scopes.len()));
                }
                self.record_borrow_bases(idx);
            }
            Expr::SliceView(_, _, a, b, _, _) => { self.record_borrow_bases(a); self.record_borrow_bases(b); }
            Expr::StructLit(_, values) => { for v in values { self.record_borrow_bases(v); } }
            Expr::EnumLit(_, _, _, values) => { for v in values { self.record_borrow_bases(v); } }
            Expr::Binary(a, _, b, _, _, _) => { self.record_borrow_bases(a); self.record_borrow_bases(b); }
            Expr::Neg(a, _, _) | Expr::Not(a) | Expr::Len(a, _, _) => self.record_borrow_bases(a),
            Expr::Index(_, _, a, _, _) => self.record_borrow_bases(a),
            Expr::VecIndex(_, a, _, _, _, _) => self.record_borrow_bases(a),
            Expr::Call(_, args, _) => {
                for arg in args {
                    match arg {
                        Argument::Value(e) => self.record_borrow_bases(e),
                        Argument::View(e) => self.record_borrow_bases(e),
                        Argument::Borrow(_, _) => {}
                    }
                }
            }
            Expr::Contains(a, b, _, _) | Expr::Split(a, b, _, _) | Expr::StrEq(a, b, _) | Expr::Find(a, b, _, _, _) => { self.record_borrow_bases(a); self.record_borrow_bases(b); }
            Expr::WriteFile(a, b, _, _) | Expr::Slice(_, a, b, _, _) => { self.record_borrow_bases(a); self.record_borrow_bases(b); }
            Expr::MapGet(_, key, _, _, _) | Expr::MapHas(_, key, _, _) | Expr::MapRemove(_, key, _, _) => self.record_borrow_bases(key),
            Expr::Join(_, sep, _, _) => self.record_borrow_bases(sep),
            Expr::Field(base, _, _) => self.record_borrow_bases(base),
            _ => {}
        }
    }
    pub(super) fn live_owned(&self) -> Vec<(usize, Type)> {
        self.scopes.iter().flatten().filter(|(_, ty)| ty.owned()).map(|&(id, ty)| (id, ty)).collect()
    }
    // A loop body runs again, so a value moved out of a variable declared
    // outside the loop must be re-initialized before the body ends; otherwise
    // the next iteration would read a moved-from handle. Without a fixed point
    // over the body (ADR-20) the move is rejected instead of guessed at.
    pub(super) fn close_loop_moves(&mut self, base: usize, before: std::collections::HashSet<usize>, offset: usize) -> Result<(), ae_span::Diagnostic> {
        let after = std::mem::replace(&mut self.moved, before.clone());
        let outer: Vec<usize> = self.scopes[..base.min(self.scopes.len())].iter().flatten().map(|&(id, _)| id).collect();
        for id in &after {
            // Values already moved before the loop are not this body's doing.
            if before.contains(id) || !outer.contains(id) { continue; }
            let name = self.names.iter().find(|(_, (vid, _, _))| vid == id).map(|(n, _)| n.clone()).unwrap_or_else(|| "?".to_owned());
            return Err(self.error_at(offset, &format!("cannot move '{name}' inside a loop body: the next iteration would read it after the move (re-initialize it before the body ends)")));
        }
        // Moves of loop-local values die with the body; nothing to merge.
        Ok(())
    }
    // break/continue leave the loop body: only owned values declared inside it die.
    pub(super) fn loop_local_owned(&self) -> Vec<(usize, Type)> {
        let base = *self.loop_scopes.last().unwrap_or(&0);
        self.scopes[base.min(self.scopes.len())..].iter().flatten().filter(|(_, ty)| ty.owned()).map(|&(id, ty)| (id, ty)).collect()
    }
    pub(super) fn block_inner(&mut self) -> Result<Vec<Statement>, ae_span::Diagnostic> {
        self.take(Kind::LBrace)?;
        let mut declared = Vec::new();
        let mut result = Vec::new();
        while *self.current() != Kind::RBrace {
            match self.current().clone() {
                Kind::Let | Kind::Var => {
                    let mutable = *self.current() == Kind::Var; self.pos += 1;
                    let binding_range = self.tokens[self.pos].offset..self.tokens[self.pos].end;
                    let name = self.identifier()?;
                    if RESERVED.contains(&name.as_str()) { return Err(self.fail(&format!("'{name}' is a built-in name and cannot be a variable"))); }
                    if self.struct_names.contains_key(&name) { return Err(self.fail(&format!("'{name}' names a struct and cannot be a variable"))); }
                    if self.enum_names.contains_key(&name) { return Err(self.fail(&format!("'{name}' names an enum and cannot be a variable"))); }
                    if self.names.contains_key(&name) { return Err(self.duplicate_binding_error(&name)); }
                    self.take(Kind::Equal)?;
                    let id = self.next_var; self.next_var += 1;
                    let (stmt, ty) = if *self.current() == Kind::LBracket {
                        self.pos += 1;
                        let mut values = Vec::new();
                        if *self.current() == Kind::RBracket { return Err(self.fail("array must contain at least one element")); }
                        loop {
                            if values.len() == 4096 { return Err(self.fail("array element limit exceeded (4096)")); }
                            let (element, element_ty) = self.expression(0, false)?;
                            if element_ty != Type::Int { return Err(self.fail("array elements must be i64")); }
                            values.push(element);
                            if *self.current() != Kind::Comma { break; }
                            self.pos += 1;
                        }
                        self.take(Kind::RBracket)?; self.take(Kind::Semi)?;
                        let len = values.len(); (Statement::Array(id, values), Type::Array(len))
                    } else {
                        // The initializer consumes string values.
                        let (value, ty) = self.expression(0, true)?;
                        // `let x = call()?;` binds the Ok payload and returns the
                        // error to the caller instead of continuing.
                        if *self.current() == Kind::Question {
                            let (statement, bound_ty) = self.propagate_statement(value, ty, Some(id))?;
                            self.pos += 1;
                            self.take(Kind::Semi)?;
                            let bound_ty = bound_ty.expect("a binding propagation always produces a payload type");
                            self.names.insert(name.clone(), (id, mutable, bound_ty));
                            self.binding_ranges.insert(id, binding_range);
                            self.scopes.last_mut().unwrap().push((id, bound_ty));
                            declared.push(name);
                            result.push(statement);
                            continue;
                        }
                        self.take(Kind::Semi)?;
                        // A borrow has no storage of its own: it lives only as a
                        // parameter or a pattern binding, never as a variable.
                        if matches!(ty, Type::VecRef | Type::StrVecRef) {
                            return Err(self.fail("cannot store a borrowed vector in a variable; pass it directly to a parameter that borrows"));
                        }
                        (Statement::Declare(id, value), ty)
                    };
                    self.names.insert(name.clone(), (id, mutable, ty));
                    self.binding_ranges.insert(id, binding_range);
                    self.scopes.last_mut().unwrap().push((id, ty));
                    declared.push(name);
                    if let Statement::Declare(_, init) = &stmt { self.record_borrow_bases(init); }
                    result.push(stmt);
                }
                Kind::Ident(name) => {
                    // `exit(code);` terminates the process. It releases the
                    // live owned values first, so the allocation account stays
                    // balanced and buffered output still flushes.
                    if name == "exit" && self.tokens.get(self.pos + 1).map(|t| t.kind.clone()) == Some(Kind::LParen) {
                        self.pos += 2;
                        let (code, code_ty) = self.expression(0, false)?;
                        if code_ty != Type::Int { return Err(self.fail("exit expects an i64 status code")); }
                        self.take(Kind::RParen)?; self.take(Kind::Semi)?;
                        result.push(Statement::Exit(code, self.live_owned()));
                        continue;
                    }
                    // Built-in mutation statements come before assignment so
                    // they stay unambiguous: push(v, x); pop(v);
                    if matches!(name.as_str(), "push" | "pop" | "sort" | "reverse") && self.tokens.get(self.pos + 1).map(|t| t.kind.clone()) == Some(Kind::LParen) {
                        let (line, col) = self.position();
                        self.pos += 2;
                        let target = self.identifier()?;
                        let (id, mutable, ty) = *self.names.get(&target).ok_or_else(|| self.fail(&format!("unknown variable '{target}'")).with_code("E0101"))?;
                        if self.moved.contains(&id) { return Err(self.binding_label(self.fail(&format!("cannot use '{target}' because it was moved")).with_code("E0102"), id)); }
                        if !matches!(ty, Type::Vec | Type::StrVec | Type::VecF) { return Err(self.fail(&format!("{name} expects a vec, found {}", ty.name()))); }
                        if !mutable { return Err(self.immutable_binding_error(self.fail(&format!("cannot modify immutable variable '{target}'")).with_code("E0103"), id)); }
                        if self.borrows.iter().any(|&(bid, _)| bid == id) { return Err(self.fail(&format!("cannot modify '{target}' while it is borrowed"))); }
                        // Ordering in place: qsort for i64, byte order for
                        // strings, so sorted output is identical on both
                        // platforms. The vector keeps its length.
                        if name == "sort" || name == "reverse" {
                            if ty == Type::VecF { return Err(self.fail(&format!("{name} is not supported for vec[f64]; NaN has no total order"))); }
                            self.take(Kind::RParen)?; self.take(Kind::Semi)?;
                            let strings = ty == Type::StrVec;
                            result.push(if name == "sort" { Statement::Sort(id, strings, line, col) } else { Statement::Reverse(id, strings, line, col) });
                            continue;
                        }
                        if name == "push" {
                            self.take(Kind::Comma)?;
                            let (value, value_ty) = self.expression(0, ty == Type::StrVec)?;
                            // A borrow is fine: the element is a copy, so a view
                            // can be stored without outliving its source.
                            let ok = match ty {
                                Type::Vec => value_ty == Type::Int,
                                Type::VecF => value_ty == Type::F64,
                                _ => matches!(value_ty, Type::String | Type::StrView),
                            };
                            if !ok { return Err(self.fail(&format!("vector elements must be {}", match ty { Type::Vec => "i64", Type::VecF => "f64", _ => "string" }))); }
                            self.take(Kind::RParen)?; self.take(Kind::Semi)?;
                            if ty == Type::StrVec {
                                result.push(Statement::StrVecPush(id, value, line, col, value_ty == Type::StrView));
                            } else if ty == Type::VecF {
                                result.push(Statement::VecFPush(id, value, line, col));
                            } else {
                                result.push(Statement::VecPush(id, value, line, col));
                            }
                        } else {
                            self.take(Kind::RParen)?; self.take(Kind::Semi)?;
                            if ty == Type::StrVec {
                                result.push(Statement::StrVecPop(id, line, col));
                            } else if ty == Type::VecF {
                                result.push(Statement::VecFPop(id, line, col));
                            } else {
                                result.push(Statement::VecPop(id, line, col));
                            }
                        }
                        continue;
                    }
                    if !self.names.contains_key(&name) {
                        // A call used only for its effect. The value must be
                        // propagated with '?', so a failure is never dropped.
                        let (value, ty) = self.expression(0, true)?;
                        if *self.current() != Kind::Question {
                            return Err(self.fail("a statement that only calls a function must end with '?' to propagate its result"));
                        }
                        let (statement, _) = self.propagate_statement(value, ty, None)?;
                        self.pos += 1;
                        self.take(Kind::Semi)?;
                        result.push(statement);
                        continue;
                    }
                    let (id, mutable, ty) = *self.names.get(&name).ok_or_else(|| self.fail(&format!("unknown variable '{name}'")).with_code("E0101"))?;
                    if !mutable { return Err(self.immutable_binding_error(self.fail(&format!("cannot assign to immutable variable '{name}'")).with_code("E0103"), id)); }
                    self.pos += 1;
                    if (ty == Type::Map || ty == Type::MapF) && *self.current() == Kind::LBracket {
                        // `m[key] = value;` inserts or overwrites. The key is
                        // copied by the runtime, so a view or temporary is fine.
                        if self.borrows.iter().any(|&(bid, _)| bid == id) {
                            return Err(self.fail(&format!("cannot modify '{name}' while it is borrowed")));
                        }
                        let (line, col) = self.position();
                        self.take(Kind::LBracket)?;
                        let (key, key_ty) = self.expression(0, false)?;
                        if !matches!(key_ty, Type::String | Type::StrView) { return Err(self.fail("map keys must be strings")); }
                        self.take(Kind::RBracket)?; self.take(Kind::Equal)?;
                        let (value, value_ty) = self.expression(0, false)?; self.take(Kind::Semi)?;
                        if value_ty != map_element_ty(map_kind_of(ty)) { return Err(self.fail(&format!("map values must be {}", map_element_ty(map_kind_of(ty)).name()))); }
                        result.push(Statement::MapPut(MapPlace::Var(id, map_kind_of(ty)), key, value, line, col));
                    } else if let Type::Array(len) = ty {
                        let (line, col) = self.position(); self.take(Kind::LBracket)?;
                        let (index, index_ty) = self.expression(0, false)?;
                        if index_ty != Type::Int { return Err(self.fail("array index must be an integer")); }
                        self.take(Kind::RBracket)?; self.take(Kind::Equal)?;
                        let (value, value_ty) = self.expression(0, false)?; self.take(Kind::Semi)?;
                        if value_ty != Type::Int { return Err(self.fail("array elements must be i64")); }
                        result.push(Statement::Store(id, len, index, value, line, col));
                    } else if ty == Type::StrVec && *self.current() == Kind::LBracket {
                        // Overwriting an element releases the old string, so a
                        // live borrow of any element blocks it.
                        if self.borrows.iter().any(|&(bid, _)| bid == id) {
                            return Err(self.fail(&format!("cannot modify '{name}' while it is borrowed")));
                        }
                        let (line, col) = self.position(); self.take(Kind::LBracket)?;
                        let (index, index_ty) = self.expression(0, false)?;
                        if index_ty != Type::Int { return Err(self.fail("vector index must be an integer")); }
                        self.take(Kind::RBracket)?; self.take(Kind::Equal)?;
                        let (value, value_ty) = self.expression(0, true)?; self.take(Kind::Semi)?;
                        if !matches!(value_ty, Type::String | Type::StrView) { return Err(self.fail("vector elements must be string")); }
                        result.push(Statement::StrVecStore(id, index, value, line, col, value_ty == Type::StrView));
                    } else if (ty == Type::Vec || ty == Type::VecF) && *self.current() == Kind::LBracket {
                        if self.borrows.iter().any(|&(bid, _)| bid == id) {
                            return Err(self.fail(&format!("cannot modify '{name}' while it is borrowed")));
                        }
                        let (line, col) = self.position(); self.take(Kind::LBracket)?;
                        let (index, index_ty) = self.expression(0, false)?;
                        if index_ty != Type::Int { return Err(self.fail("vector index must be an integer")); }
                        self.take(Kind::RBracket)?; self.take(Kind::Equal)?;
                        let (value, value_ty) = self.expression(0, false)?; self.take(Kind::Semi)?;
                        if ty == Type::VecF {
                            if value_ty != Type::F64 { return Err(self.fail("vector elements must be f64")); }
                            result.push(Statement::VecFStore(id, index, value, line, col));
                        } else {
                            if value_ty != Type::Int { return Err(self.fail("vector elements must be i64")); }
                            result.push(Statement::VecStore(id, index, value, line, col));
                        }
                    } else if let Type::Enum(_, _) = ty {
                        // Assigning an enum releases the old payload first.
                        if self.borrows.iter().any(|&(bid, _)| bid == id) {
                            return Err(self.fail(&format!("cannot assign to '{name}' while it is borrowed")));
                        }
                        self.take(Kind::Equal)?;
                        let moved_before = self.moved.contains(&id);
                        let (value, value_ty) = self.expression(0, true)?; self.take(Kind::Semi)?;
                        if value_ty != ty { return Err(self.fail(&format!("cannot assign a {} value to {} variable", value_ty.name(), ty.name()))); }
                        if self.moved.contains(&id) && !moved_before {
                            return Err(self.fail(&format!("cannot assign to '{name}' because it was moved in the same statement")));
                        }
                        self.moved.remove(&id);
                        result.push(Statement::Assign(id, value, true));
                    } else if let Type::Struct(sid) = ty {
                        if *self.current() == Kind::Dot {
                            self.take(Kind::Dot)?;
                            let fname = self.identifier()?;
                            let fty = {
                                let def = self.structs[sid].clone();
                                let fidx = def.fields.iter().position(|(n, _)| *n == fname)
                                    .ok_or_else(|| self.fail(&format!("struct '{}' has no field '{}'", def.name, fname)))?;
                                def.fields[fidx].1
                            };
                            if (fty == Type::Map || fty == Type::MapF) && *self.current() == Kind::LBracket {
                                // `s.counts[key] = n;` writes through the field,
                                // so the record itself stays in place.
                                if self.borrows.iter().any(|&(bid, _)| bid == id) {
                                    return Err(self.fail(&format!("cannot modify '{name}' while it is borrowed")));
                                }
                                let (line, col) = self.position();
                                self.take(Kind::LBracket)?;
                                let (key, key_ty) = self.expression(0, false)?;
                                if !matches!(key_ty, Type::String | Type::StrView) { return Err(self.fail("map keys must be strings")); }
                                self.take(Kind::RBracket)?; self.take(Kind::Equal)?;
                                let (value, value_ty) = self.expression(0, false)?; self.take(Kind::Semi)?;
                                if value_ty != map_element_ty(map_kind_of(fty)) { return Err(self.fail(&format!("map values must be {}", map_element_ty(map_kind_of(fty)).name()))); }
                                result.push(Statement::MapPut(MapPlace::Field(id, fname, map_kind_of(fty)), key, value, line, col));
                                continue;
                            }
                            // A view into one of the fields points at the bytes
                            // this write would release, so the write is only
                            // allowed once the view is gone (ADR-52).
                            if self.borrows.iter().any(|&(bid, _)| bid == id) {
                                return Err(self.fail(&format!("cannot modify '{name}' while it is borrowed")));
                            }
                            self.take(Kind::Equal)?;
                            let (value, value_ty) = self.expression(0, false)?; self.take(Kind::Semi)?;
                            if value_ty != fty { return Err(self.fail(&format!("field '{}' must be {}", fname, fty.name()))); }
                            result.push(Statement::FieldStore(id, fname, fty, value));
                            continue;
                        }
                        // Whole-struct assignment moves a fresh value in.
                        if self.borrows.iter().any(|&(bid, _)| bid == id) {
                            return Err(self.fail(&format!("cannot assign to '{name}' while it is borrowed")));
                        }
                        self.take(Kind::Equal)?;
                        let moved_before = self.moved.contains(&id);
                        let (value, value_ty) = self.expression(0, true)?; self.take(Kind::Semi)?;
                        if value_ty != ty { return Err(self.fail(&format!("cannot assign a {} value to {} variable", value_ty.name(), ty.name()))); }
                        if self.moved.contains(&id) && !moved_before {
                            return Err(self.fail(&format!("cannot assign to '{name}' because it was moved in the same statement")));
                        }
                        self.moved.remove(&id);
                        result.push(Statement::Assign(id, value, true));
                    } else {
                        if self.borrows.iter().any(|&(bid, _)| bid == id) {
                            return Err(self.fail(&format!("cannot assign to '{name}' while it is borrowed")));
                        }
                        self.take(Kind::Equal)?;
                        let moved_before = self.moved.contains(&id);
                        let (value, value_ty) = self.expression(0, true)?; self.take(Kind::Semi)?;
                        if value_ty != ty { return Err(self.fail(&format!("cannot assign a {} value to {} variable '{name}'", value_ty.name(), ty.name()))); }
                        // The right-hand side may not consume the target itself;
                        // assigning to an already-moved variable is a legal
                        // re-initialization, so the move flag clears either way.
                        if self.moved.contains(&id) && !moved_before {
                            return Err(self.fail(&format!("cannot assign to '{name}' because it was moved in the same statement")));
                        }
                        self.moved.remove(&id);
                        // Every owned slot releases its old value before the new
                        // one moves in; scalars have nothing to release.
                        result.push(Statement::Assign(id, value, ty.owned()));
                    }
                }
                Kind::Return => {
                    if self.in_main { return Err(self.fail("return is not supported in main")); }
                    self.pos += 1;
                    let (value, ty) = self.expression(0, true)?; self.take(Kind::Semi)?;
                    if ty != self.return_type { return Err(self.fail(&format!("return type must be {}", self.return_type.name()))); }
                    // Every live owned value that is not returned is released.
                    result.push(Statement::Return(value, self.live_owned()));
                }
                Kind::If => result.push(self.if_statement()?),
                Kind::Match => result.push(self.match_statement()?),
                Kind::While => {
                    let loop_offset = self.tokens[self.pos].offset;
                    self.pos += 1;
                    let (condition, ty) = self.expression(0, false)?;
                    if ty != Type::Bool { return Err(self.fail("condition must be a boolean expression")); }
                    if self.loop_depth >= 64 { return Err(self.fail("loop nesting limit exceeded (64)")); }
                    self.loop_depth += 1;
                    let base = self.scopes.len();
                    let moved_before = self.moved.clone();
                    self.loop_scopes.push(base);
                    self.loop_labels.push(None);
                    let body = self.block();
                    self.loop_labels.pop();
                    self.loop_scopes.pop();
                    self.loop_depth -= 1;
                    let body = body?;
                    self.close_loop_moves(base, moved_before, loop_offset)?;
                    result.push(Statement::While(condition, body));
                }
                Kind::For => {
                    let loop_offset = self.tokens[self.pos].offset;
                    self.pos += 1;
                    let var_name = self.identifier()?;
                    if RESERVED.contains(&var_name.as_str()) { return Err(self.fail(&format!("'{var_name}' is a built-in name and cannot be a loop variable"))); }
                    if self.names.contains_key(&var_name) { return Err(self.duplicate_binding_error(&var_name)); }
                    self.take(Kind::In)?;
                    let (first, first_ty) = self.expression(0, false)?;
                    if first_ty == Type::Tokens {
                        // `for token in cursor` streams the tokens as read-only
                        // views: no vector, no per-token allocation, and the
                        // loop body sees one token at a time (ADR-53). Each
                        // loop restarts at the beginning of the base text.
                        let cursor = match &first {
                            Expr::Variable(id, Type::Tokens, _, _) => *id,
                            _ => return Err(self.fail("for-in over a token cursor needs a bare cursor variable: bind it first, e.g. let toks = tokens(text, \" \");")),
                        };
                        if self.loop_depth >= 64 { return Err(self.fail("loop nesting limit exceeded (64)")); }
                        let (line, col) = self.position();
                        let var = self.next_var; self.next_var += 1;
                        let label = self.next_label; self.next_label += 1;
                        self.loop_depth += 1;
                        let base = self.scopes.len();
                        let moved_before = self.moved.clone();
                        self.scopes.push(vec![(var, Type::StrView)]);
                        self.loop_scopes.push(self.scopes.len());
                        self.loop_labels.push(Some(label));
                        self.names.insert(var_name.clone(), (var, false, Type::StrView));
                        let body = self.block();
                        self.loop_labels.pop();
                        self.loop_scopes.pop();
                        self.scopes.pop().unwrap();
                        self.loop_depth -= 1;
                        self.names.remove(&var_name);
                        let body = body?;
                        self.close_loop_moves(base, moved_before, loop_offset)?;
                        result.push(Statement::ForTokens(cursor, var, body, label, line, col));
                        continue;
                    }
                    if *self.current() != Kind::DotDot {
                        // `for x in vec-variable` iterates owned elements by
                        // value: desugars to a hidden range loop plus an
                        // immutable element declaration per iteration.
                        // Borrowed string vectors iterate the same way, with
                        // element access going through the pointer.
                        let (v_id, elem_ty, by_ref) = match (&first, first_ty) {
                            (Expr::Variable(id, Type::Vec, _, _), Type::Vec) => (*id, Type::Int, false),
                            (Expr::Variable(id, Type::StrVec, _, _), Type::StrVec) => (*id, Type::StrView, false),
                            (Expr::Variable(id, Type::StrVecRef, _, _), Type::StrVecRef) => (*id, Type::StrView, true),
                            (Expr::Variable(id, Type::VecRef, _, _), Type::VecRef) => (*id, Type::Int, false),
                            (Expr::Variable(id, Type::VecF, _, _), Type::VecF) => (*id, Type::F64, false),
                            (Expr::Variable(id, Type::VecFRef, _, _), Type::VecFRef) => (*id, Type::F64, false),
                            _ => return Err(self.fail("for-in needs a bare vec variable or an int range (start..end)")),
                        };
                        if self.loop_depth >= 64 { return Err(self.fail("loop nesting limit exceeded (64)")); }
                        let (line, col) = self.position();
                        let end_var = self.next_var; self.next_var += 1;
                        let counter = self.next_var; self.next_var += 1;
                        let var = self.next_var; self.next_var += 1;
                        let label = self.next_label; self.next_label += 1;
                        self.loop_depth += 1;
                        let base = self.scopes.len();
                        let moved_before = self.moved.clone();
                        self.scopes.push(vec![(var, elem_ty), (counter, Type::Int), (end_var, Type::Int)]);
                        self.loop_scopes.push(self.scopes.len());
                        self.loop_labels.push(Some(label));
                        self.names.insert(var_name.clone(), (var, false, elem_ty));
                        // The container is borrowed for the whole loop.
                        let container_depth = self.scopes.len();
                        self.borrows.push((v_id, container_depth));
                        let body = self.block();
                        self.loop_labels.pop();
                        self.loop_scopes.pop();
                        self.scopes.pop().unwrap();
                        self.loop_depth -= 1;
                        self.names.remove(&var_name);
                        self.borrows.retain(|&(id, _)| id != v_id);
                        let mut body = body?;
                        if self.moved.contains(&v_id) { return Err(self.fail(&format!("cannot move the vector being iterated (a move of it happens in this loop)"))); }
                        self.close_loop_moves(base, moved_before, loop_offset)?;
                        // Per-iteration element binding, then the user's body.
                        let element = match elem_ty {
                            Type::StrView => Expr::StrVecIndex(v_id, Box::new(Expr::Variable(counter, Type::Int, line, col)), line, col, by_ref),
                            _ => Expr::VecIndex(v_id, Box::new(Expr::Variable(counter, Type::Int, line, col)), line, col, first_ty == Type::VecRef || first_ty == Type::VecFRef, elem_ty),
                        };
                        body.insert(0, Statement::Declare(var, element));
                        result.push(Statement::Declare(end_var, Expr::Len(Box::new(first), line, col)));
                        result.push(Statement::Declare(counter, Expr::Number(0)));
                        result.push(Statement::For(counter, end_var, body, label, line, col));
                        continue;
                    }
                    if first_ty != Type::Int { return Err(self.fail("range bounds must be i64")); }
                    self.take(Kind::DotDot)?;
                    let (end, end_ty) = self.expression(0, false)?;
                    if end_ty != Type::Int { return Err(self.fail("range bounds must be i64")); }
                    if self.loop_depth >= 64 { return Err(self.fail("loop nesting limit exceeded (64)")); }
                    let (line, col) = self.position();
                    // Hidden storage: bounds evaluate once, in order.
                    let end_var = self.next_var; self.next_var += 1;
                    let var = self.next_var; self.next_var += 1;
                    let label = self.next_label; self.next_label += 1;
                    self.loop_depth += 1;
                    let base = self.scopes.len();
                    let moved_before = self.moved.clone();
                    self.scopes.push(vec![(var, Type::Int)]);
                    self.loop_scopes.push(self.scopes.len());
                    self.loop_labels.push(Some(label));
                    self.names.insert(var_name.clone(), (var, false, Type::Int));
                    let body = self.block();
                    self.loop_labels.pop();
                    self.loop_scopes.pop();
                    let for_scope = self.scopes.pop().unwrap();
                    self.loop_depth -= 1;
                    self.names.remove(&var_name);
                    let body = body?;
                    self.close_loop_moves(base, moved_before, loop_offset)?;
                    debug_assert!(for_scope.iter().all(|(_, ty)| *ty != Type::String));
                    // Bounds evaluate once, left to right, before any iteration.
                    result.push(Statement::Declare(var, first));
                    result.push(Statement::Declare(end_var, end));
                    result.push(Statement::For(var, end_var, body, label, line, col));
                }
                Kind::Break | Kind::Continue => {
                    let keyword = if *self.current() == Kind::Break { "break" } else { "continue" };
                    if self.loop_depth == 0 { return Err(self.fail(&format!("'{keyword}' outside of a loop"))); }
                    self.pos += 1; self.take(Kind::Semi)?;
                    let owned = self.loop_local_owned();
                    if !owned.is_empty() { result.push(Statement::Frees(owned)); }
                    result.push(if keyword == "break" { Statement::Break } else { Statement::Continue(self.loop_labels.last().copied().flatten()) });
                }
                Kind::Assert => {
                    let (line, col) = self.position();
                    self.pos += 1; self.take(Kind::LParen)?;
                    let (condition, ty) = self.expression(0, false)?;
                    if ty != Type::Bool { return Err(self.fail("assert requires a boolean condition")); }
                    self.take(Kind::RParen)?; self.take(Kind::Semi)?;
                    result.push(Statement::Assert(condition, line, col));
                }
                Kind::Print => {
                    self.pos += 1; self.take(Kind::LParen)?;
                    let is_bare_text = matches!(self.current(), Kind::Text(_))
                        && matches!(self.tokens.get(self.pos + 1).map(|t| &t.kind), Some(Kind::RParen));
                    let printed = if is_bare_text {
                        if let Kind::Text(text) = self.current().clone() { self.pos += 1; Printed::Text(text) } else { unreachable!() }
                    } else {
                        // print borrows its value; only `+` consumes strings.
                        let (value, ty) = self.expression(0, false)?;
                        if !matches!(ty, Type::Int | Type::Bool | Type::F64 | Type::String | Type::StrView) {
                            return Err(self.fail(&format!("cannot print a value of type {}", ty.name())));
                        }
                        Printed::Expr(value)
                    };
                    self.take(Kind::RParen)?; self.take(Kind::Semi)?;
                    result.push(Statement::Print(printed));
                }
                _ => return Err(self.fail("expected declaration, assignment, print, or '}'")),
            };
        }
        self.take(Kind::RBrace)?;
        for name in declared { self.names.remove(&name); }
        Ok(result)
    }
    pub(super) fn if_statement(&mut self) -> Result<Statement, ae_span::Diagnostic> {
        self.pos += 1;
        let (condition, ty) = self.expression(0, false)?;
        if ty != Type::Bool { return Err(self.fail("condition must be a boolean expression")); }
        let snapshot = self.moved.clone();
        let yes = self.block()?;
        let after_yes = std::mem::replace(&mut self.moved, snapshot.clone());
        let no = if *self.current() == Kind::Else {
            self.pos += 1;
            // `else if` chains recurse instead of nesting braces.
            if *self.current() == Kind::If { vec![self.if_statement()?] } else { self.block()? }
        } else { Vec::new() };
        // A variable moved in either branch is treated as moved after the if.
        for id in after_yes { self.moved.insert(id); }
        Ok(Statement::If(condition, yes, no))
    }
    // `expr?` propagates the error variant of a result enum: on `Err` every
    // live owned value is released and the enum itself is returned, so a caller
    // never observes a half-finished result. On `Ok` the payload is bound (or
    // released when the statement only wanted the side effect).
    pub(super) fn propagate_statement(&mut self, value: Expr, ty: Type, binding: Option<usize>) -> Result<(Statement, Option<Type>), ae_span::Diagnostic> {
        let eid = match ty {
            Type::Enum(_, eid) => eid,
            _ => return Err(self.fail(&format!("'?' needs a result or option value, found {}", ty.name()))),
        };
        let def = self.enums[eid].clone();
        // Both two-variant shapes can be propagated: a result hands its Err
        // back, an Option hands its None back. Neither variant has to be a
        // prelude enum - a user enum with the same shape behaves identically.
        let result_shaped = def.variants.len() == 2
            && def.variants[0].0 == "Ok" && def.variants[1].0 == "Err"
            && def.variants[1].1 == [Type::String];
        let option_shaped = def.variants.len() == 2
            && def.variants[0].0 == "Some" && def.variants[1].0 == "None"
            && def.variants[1].1.is_empty() && def.variants[0].1.len() == 1;
        if !result_shaped && !option_shaped {
            return Err(self.fail(&format!("'?' needs an enum shaped {{ Ok(...), Err(string) }} or {{ Some(T), None }}, and '{}' is not one", def.name)));
        }
        if self.in_main || self.return_type != ty {
            return Err(self.fail(&format!("'?' needs the enclosing function to return {}, so the failure can be handed on", def.name)));
        }
        let fields = def.variants[0].1.clone();
        let bound = match binding {
            Some(id) => {
                if fields.len() != 1 {
                    return Err(self.fail(&format!("'{}::{}' carries {} values; a binding `let` needs exactly one (use a bare statement to discard)", def.name, def.variants[0].0, fields.len())));
                }
                Some((id, fields[0]))
            }
            None => None,
        };
        let bound_ty = bound.map(|(_, ty)| ty);
        Ok((Statement::Propagate(value, ty, fields, bound, self.live_owned()), bound_ty))
    }
    // `match subject { Enum::Variant(a, b) => { ... } ... }` is a statement.
    // Coverage must be exhaustive and each variant may appear once, so there is
    // no fall-through path to reason about. The subject is borrowed for the
    // whole match: arms can read payloads but never move or overwrite it.
    pub(super) fn match_statement(&mut self) -> Result<Statement, ae_span::Diagnostic> {
        self.pos += 1;
        let (subject, subject_ty) = self.expression(0, false)?;
        let (sid, eid) = match (&subject, subject_ty) {
            (Expr::Variable(id, Type::Enum(_, _), _, _), Type::Enum(_, k)) => (*id, k),
            _ => return Err(self.fail("match needs a bare enum variable as its subject")),
        };
        if self.moved.contains(&sid) { return Err(self.fail("cannot match a value that was moved")); }
        let def = self.enums[eid].clone();
        self.take(Kind::LBrace)?;
        let mut arms: Vec<MatchArm> = Vec::new();
        let mut seen = vec![false; def.variants.len()];
        self.borrows.push((sid, self.scopes.len()));
        while *self.current() != Kind::RBrace {
            let pattern = self.identifier()?;
            // An instantiated enum is matched by its declaration name:
            // `Option::Some(v)` covers Option[string] and Option[i64] alike.
            if pattern != def.pattern {
                return Err(self.fail(&format!("pattern must name the matched enum ('{}::Variant')", def.pattern)));
            }
            self.take(Kind::ColonColon)?;
            let vname = self.identifier()?;
            let vidx = def.variants.iter().position(|(n, _)| *n == vname)
                .ok_or_else(|| self.fail(&format!("enum '{}' has no variant '{}'", def.name, vname)))?;
            if seen[vidx] { return Err(self.fail(&format!("variant '{}::{}' is matched twice", def.name, vname))); }
            seen[vidx] = true;
            let fields = def.variants[vidx].1.clone();
            self.scopes.push(Vec::new());
            let mut bindings = Vec::new();
            let mut bound: Vec<String> = Vec::new();
            if *self.current() == Kind::LParen {
                if fields.is_empty() { return Err(self.fail(&format!("variant '{}::{}' has no payload", def.name, vname))); }
                self.pos += 1;
                for (index, field) in fields.iter().copied().enumerate() {
                    let bname = self.identifier()?;
                    if RESERVED.contains(&bname.as_str()) { return Err(self.fail(&format!("'{bname}' is a built-in name and cannot be a pattern binding"))); }
                    if self.names.contains_key(&bname) { return Err(self.duplicate_binding_error(&bname)); }
                    let id = self.next_var; self.next_var += 1;
                    // String payloads bind as read-only views and vec payloads
                    // as read-only borrows: nothing can be moved out of the
                    // matched enum.
                    let bty = match field {
                        Type::String => Type::StrView,
                        Type::Vec => Type::VecRef,
                        Type::StrVec => Type::StrVecRef,
                        Type::VecF => Type::VecFRef,
                        other => other,
                    };
                    self.names.insert(bname.clone(), (id, false, bty));
                    self.scopes.last_mut().unwrap().push((id, bty));
                    bound.push(bname);
                    bindings.push((id, field, index));
                    if index + 1 < fields.len() { self.take(Kind::Comma)?; }
                }
                if *self.current() == Kind::Comma {
                    return Err(self.fail(&format!("variant '{}::{}' binds {} payload value(s)", def.name, vname, fields.len())));
                }
                self.take(Kind::RParen)?;
            } else if !fields.is_empty() {
                return Err(self.fail(&format!("variant '{}::{}' needs {} binding(s)", def.name, vname, fields.len())));
            }
            self.take(Kind::FatArrow)?;
            let body = self.block()?;
            self.scopes.pop().unwrap();
            for name in bound { self.names.remove(&name); }
            arms.push(MatchArm { variant: vidx, bindings, body });
        }
        self.take(Kind::RBrace)?;
        self.borrows.pop();
        if let Some(missing) = seen.iter().position(|covered| !covered) {
            return Err(self.fail(&format!("match is not exhaustive: '{}::{}' is not handled", def.name, def.variants[missing].0)));
        }
        Ok(Statement::Match(sid, subject_ty, arms))
    }
}
