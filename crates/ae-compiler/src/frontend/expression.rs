//! Expression parsing and type checking.
use super::*;
use super::parser::moved_value_hint;

impl Parser<'_> {
    // `value.method(...)` with no such method. Naming what the type does have
    // turns a typo into a one-line fix; a type with no impl at all gets the
    // declaration form instead, since there is nothing to list.
    // `Type::name(args)` reaches an associated function — one declared without
    // a receiver. It is the same call a plain function would be, with the type
    // name standing in for the namespace the impl block created.
    fn associated_call(&mut self, type_name: &str, offset: usize) -> Result<(Expr, Type), ae_span::Diagnostic> {
        self.take(Kind::ColonColon)?;
        let method = self.identifier()?;
        if let Some(&sid) = self.struct_names.get(type_name) {
            let def = &self.structs[sid];
            self.require_public("struct", &def.name.clone(), &def.file.clone(), def.public)?;
        } else if let Some(&eid) = self.enum_names.get(type_name) {
            let def = &self.enums[eid];
            self.require_public("enum", def.name, &def.file, def.public)?;
        } else {
            return Err(self.error_at(offset, &format!("'{type_name}' is not a type with an impl block")));
        }
        let key = format!("{type_name}#{method}");
        let Some(def) = self.generics.inherent.get(&key).cloned() else {
            return Err(self.no_such_method(type_name, &method, "associated function"));
        };
        if def.receiver.is_some() {
            return Err(self.error_at(offset, &format!("'{type_name}::{method}' takes a receiver; call it as 'value.{method}(...)' instead")));
        }
        if !def.signature.public {
            return Err(self.error_at(offset, &format!("associated function '{type_name}::{method}' is private")));
        }
        let params = def.signature.params.clone();
        let args = self.call_arguments(&format!("{type_name}::{method}"), &params)?;
        Ok((Expr::Call(def.signature.id, args, def.signature.ret), def.signature.ret))
    }

    fn no_such_method(&self, type_name: &str, method: &str, form: &str) -> ae_span::Diagnostic {
        let prefix = format!("{type_name}#");
        let mut names: Vec<String> = self.generics.inherent.keys()
            .filter_map(|key| key.strip_prefix(&prefix).map(str::to_owned))
            .collect();
        // A generic struct keeps its methods in its impl template, keyed by
        // the declaration name; list them by their own names (ADR-0187).
        if let Some((gid, _)) = self.struct_origin_of(type_name) {
            let def_name = self.generics.struct_defs[gid].name.clone();
            if let Some(gimpl) = self.generics.generic_impls.get(&def_name) {
                names.extend(gimpl.methods.iter().map(|m| m.name.clone()));
            }
        }
        if names.is_empty() {
            return self.fail(&format!("type '{type_name}' has no {form}s; declare one in 'impl {type_name} {{ ... }}'"));
        }
        names.sort();
        names.dedup();
        self.fail(&format!("type '{type_name}' has no {form} '{method}'; it has {}", names.join(", ")))
    }

    // The generic declaration behind a type name, when the name is an
    // instantiation. Accepts the display spelling (Pair[string, i64]) as
    // well as the declaration name (Pair).
    fn struct_origin_of(&self, type_name: &str) -> Option<(usize, Vec<Type>)> {
        if let Some(&(gid, ref args)) = self.generics.struct_origin.values().find(|(gid, _)| {
            self.generics.struct_defs[*gid].name == type_name
        }) {
            return Some((gid, args.clone()));
        }
        for (sid, origin) in &self.generics.struct_origin {
            if self.structs[*sid].name == type_name {
                return Some(origin.clone());
            }
        }
        None
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
            // A closure starts with a pipe — or with `||`, which the lexer
            // keeps as Or: an or-expression is never followed by an arrow, so
            // Or + '->' can only be an empty parameter list (ADR-0188).
            Kind::Pipe => self.closure_literal()?,
            Kind::Or if self.tokens.get(self.pos + 1).map(|t| t.kind.clone()) == Some(Kind::Arrow) => self.closure_literal()?,
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
                    // `Enum::Variant` / `Enum::Variant(payload, ...)`, or
                    // `Type::assoc_fn(...)` from the same impl block
                    // (ADR-0183, ADR-0186). The name after `::` decides: a
                    // variant name is a variant path, anything else reaches
                    // the impl — shallow, deterministic, no guessing.
                    if name == "Self" {
                        let Some(target) = self.impl_self.clone() else {
                            return Err(self.error_at(name_offset, "'Self' is only available inside an impl block"));
                        };
                        let type_name = match target {
                            Type::Struct(sid) => self.structs[sid].name.clone(),
                            Type::Enum(ename, _) => ename.to_owned(),
                            other => return Err(self.error_at(name_offset, &format!("'Self' is {} here and has no associated functions", other.name()))),
                        };
                        // For an enum Self, a variant name after `::` is a
                        // variant path, exactly like a bare enum name; an
                        // associated function resolves through the template.
                        if let Type::Enum(ename, eid) = target {
                            let def = &self.enums[eid];
                            let next_is_variant = match self.tokens.get(self.pos + 1).map(|t| &t.kind) {
                                Some(Kind::Ident(v)) => def.variants.iter().any(|(vn, _)| vn == v),
                                _ => false,
                            };
                            if next_is_variant {
                                self.enum_literal(eid, name_offset)?
                            } else if let Some((gid, origin_args)) = self.generics.enum_origin.get(&eid).cloned() {
                                // `Self::assoc(...)` on a generic enum:
                                // resolve through the declaration's template
                                // with the instantiation's own arguments.
                                self.take(Kind::ColonColon)?;
                                let method = self.identifier()?;
                                let template = self.generics.defs[gid].name.clone();
                                let origin: Vec<Type> = origin_args;
                                let Some(gimpl) = self.generics.generic_impls.get(&template).cloned() else {
                                    return Err(self.error_at(name_offset, &format!("'{}' has no generic impl block", template)));
                                };
                                let Some(mdef) = gimpl.methods.iter().find(|m| m.name == method).cloned() else {
                                    let mut names: Vec<String> = gimpl.methods.iter().map(|m| m.name.clone()).collect();
                                    names.sort();
                                    return Err(self.error_at(name_offset, &format!("the impl for '{template}' has no associated function '{method}'; it has {}", names.join(", "))));
                                };
                                let has_receiver = mdef.receiver.is_some();
                                let instance = self.instantiate_impl_method(&template, &method, eid, gid, &origin, name_offset)?;
                                let (id, params, ret) = instance;
                                let call_params = if has_receiver { params[1..].to_vec() } else { params.clone() };
                                let call_args = self.call_arguments(&format!("{ename}::{method}"), &call_params)?;
                                (Expr::Call(id, call_args, ret), ret)
                            } else {
                                // A plain enum's impl block: the same lookup a
                                // bare `Shape::circle(...)` outside the impl
                                // uses (ADR-0186).
                                self.associated_call(&type_name, name_offset)?
                            }
                        } else {
                            self.associated_call(&type_name, name_offset)?
                        }
                    } else if let Some(&eid) = self.enum_names.get(&name) {
                        let def = &self.enums[eid];
                        let next_is_variant = match self.tokens.get(self.pos + 1).map(|t| &t.kind) {
                            Some(Kind::Ident(v)) => def.variants.iter().any(|(vn, _)| vn == v),
                            _ => false,
                        };
                        if next_is_variant {
                            self.enum_literal(eid, name_offset)?
                        } else {
                            self.associated_call(&name, name_offset)?
                        }
                    } else if self.struct_names.contains_key(&name) {
                        self.associated_call(&name, name_offset)?
                    } else {
                        return Err(self.error_at(name_offset, &format!("'{name}' is not an enum or a type with an impl block; '::' reaches enum variants and associated functions, nothing else")));
                    }
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
                    let eid = self.instantiate_enum(gid, args.clone(), name_offset)?;
                    // `::` after the arguments: a variant name stays a variant
                    // path (enum_literal consumes `::` itself); anything else
                    // resolves through the template's associated functions
                    // (ADR-0196).
                    let next_ident = match self.tokens.get(self.pos + 1).map(|t| t.kind.clone()) {
                        Some(Kind::Ident(v)) => Some(v),
                        _ => None,
                    };
                    let next_is_variant = next_ident.as_deref()
                        .map(|v| self.generics.defs[gid].variants.iter().any(|(vn, _)| vn == v))
                        .unwrap_or(false);
                    if *self.current() == Kind::ColonColon && !next_is_variant {
                        self.pos += 1; // consume `::`
                            self.pos += 1; // skip the method name
                        let method = next_ident.unwrap_or_default();
                        let display = self.enums[eid].name;
                        let template = self.generics.defs[gid].name.clone();
                        let origin: Vec<Type> = args.clone();
                        let Some(gimpl) = self.generics.generic_impls.get(&template).cloned() else {
                            return Err(self.error_at(name_offset, &format!("'{}' has no generic impl block, so it has no associated functions", template)));
                        };
                        let Some(mdef) = gimpl.methods.iter().find(|m| m.name == method).cloned() else {
                            let mut names: Vec<String> = gimpl.methods.iter().map(|m| m.name.clone()).collect();
                            names.sort();
                            return Err(self.error_at(name_offset, &format!("the impl for '{template}' has no associated function '{method}'; it has {}", names.join(", "))));
                        };
                        let has_receiver = mdef.receiver.is_some();
                        let instance = self.instantiate_impl_method(&template, &method, eid, gid, &origin, name_offset)?;
                        let (id, params, ret) = instance;
                        // `call_arguments` starts at '('; assert the shape.
                        if *self.current() != Kind::LParen {
                            return Err(self.error_at(name_offset, &format!("'{method}' expects an argument list")));
                        }
                        let call_params = if has_receiver { params[1..].to_vec() } else { params.clone() };
                        let call_args = self.call_arguments(&format!("{display}::{method}"), &call_params)?;
                        (Expr::Call(id, call_args, ret), ret)
                    } else {
                        self.enum_literal(eid, name_offset)?
                    }
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
                    if *self.current() == Kind::ColonColon {
                        // `Pair[string, i64]::new(...)` (ADR-0187): the type
                        // arguments pick both the layout and the method body.
                        self.pos += 1;
                        let method = self.identifier()?;
                        let display = self.structs[sid].name.clone();
                        let Some(&(origin_gid, ref origin_args)) = self.generics.struct_origin.get(&sid) else {
                            return Err(self.error_at(name_offset, "internal: a generic instantiation has no origin"));
                        };
                        let origin: Vec<Type> = origin_args.clone();
                        let def_name = self.generics.struct_defs[origin_gid].name.clone();
                        let Some(gimpl) = self.generics.generic_impls.get(&def_name).cloned() else {
                            return Err(self.error_at(name_offset, &format!("'{}' has no generic impl block, so it has no associated functions", def_name)));
                        };
                        let Some(mdef) = gimpl.methods.iter().find(|m| m.name == method).cloned() else {
                            let mut names: Vec<String> = gimpl.methods.iter().map(|m| m.name.clone()).collect();
                            names.sort();
                            return Err(self.error_at(name_offset, &format!("the impl for '{def_name}' has no associated function '{method}'; it has {}", names.join(", "))));
                        };
                        let has_receiver = mdef.receiver.is_some();
                        let instance = self.instantiate_impl_method(&def_name, &method, sid, origin_gid, &origin, name_offset)?;
                        let (id, params, ret) = instance;
                        let call_params = if has_receiver { params[1..].to_vec() } else { params.clone() };
                        let call_args = self.call_arguments(&format!("{display}::{method}"), &call_params)?;
                        // A tail value, not an early return: the postfix loop
                        // still owns chaining (Pair[string, i64]::new(..).m()).
                        (Expr::Call(id, call_args, ret), ret)
                    } else if *self.current() == Kind::LBrace {
                        self.struct_literal(sid, name_offset)?
                    } else {
                        return Err(self.error_at(name_offset, &format!("'{}' is a struct; construct it with '{}{{ ... }}'", self.structs[sid].name.clone(), self.structs[sid].name.clone())));
                    }
                } else if name == "Self" && (*self.current() == Kind::LBrace || *self.current() == Kind::ColonColon) {
                    // `Self` in an expression (ADR-0186): the impl's type as a
                    // constructor — `Self { ... }` or `Self::assoc(...)`.
                    let Some(target) = self.impl_self.clone() else {
                        return Err(self.error_at(name_offset, "'Self' is only available inside an impl block"));
                    };
                    match target {
                        Type::Struct(sid) => {
                            if *self.current() == Kind::LBrace {
                                return self.struct_literal(sid, name_offset);
                            }
                            let sname = self.structs[sid].name.clone();
                            return self.associated_call(&sname, name_offset);
                        }
                        Type::Enum(ename, _) => {
                            return self.associated_call(&ename, name_offset);
                        }
                        other => return Err(self.error_at(name_offset, &format!("'Self' is {} here and has no constructor", other.name()))),
                    }
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
                                if self.moved.contains(&id) { return Err(moved_value_hint(self.binding_label(self.error_at(name_offset, &format!("cannot use '{arg_name}' because it was moved")).with_code("E0102"), id))); }
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
                    if !self.borrows.iter().any(|&(id, _, _)| id == vid) {
                        self.borrows.push((vid, depth, BorrowKind::View));
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
                    if !self.borrows.iter().any(|&(id, _, _)| id == vid) {
                        self.borrows.push((vid, depth, BorrowKind::View));
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
                } else if matches!(self.names.get(&name), Some((_, _, Type::Closure(_)))) && *self.current() == Kind::LParen {
                    // A closure value is a name for its generated function:
                    // the call reads the captures at the call site (ADR-0188).
                    let (expr, ty) = self.closure_call(&name, name_offset)?;
                    (expr, ty)
                } else if matches!(self.names.get(&name), Some((_, _, Type::Fn(_)))) && *self.current() == Kind::LParen {
                    // A function-typed value is called through the pointer the
                    // caller passed in (ADR-0189): the signature is interned.
                    let Some((_, _, Type::Fn(sig))) = self.names.get(&name) else { unreachable!() };
                    let (params, ret) = super::closures::fn_sig(*sig);
                    let args = self.call_arguments(&name, &params)?;
                    let (id, _, _) = *self.names.get(&name).unwrap();
                    (Expr::FnCall(id, args, ret), ret)
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
                    if self.moved.contains(&id) { return Err(moved_value_hint(self.binding_label(self.error_at(name_offset, &format!("cannot use '{name}' because it was moved")).with_code("E0102"), id))); }
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
            // Field access works on owned structs and on read-only borrows of
            // them (ADR-0184): the borrowed form reads through the pointer and
            // emits the same `Expr::Field`, whose C emission is member access.
            // Methods resolve on the same shapes plus enums (ADR-0186).
            let (type_name, sid) = match base_ty {
                Type::Struct(sid) => (self.structs[sid].name.clone(), Some(sid)),
                Type::StructRef(sid) | Type::StructMut(sid) => (self.structs[sid].name.clone(), Some(sid)),
                // The enum's id flows to the generic-impl resolution below:
                // enum instantiations resolve methods through their
                // declaration's template, same as structs (ADR-0196).
                Type::Enum(name, eid) => (name.to_owned(), Some(eid)),
                other => return Err(self.fail(&format!("type {} has no fields or methods", other.name()))),
            };
            // `value.method(args)` resolves to the method's ordinary function,
            // with the receiver as argument 0 (M1.2). Nothing is added to the
            // backend: the call it builds is the one a hand-written call builds.
            if *self.current() == Kind::LParen {
                let struct_name = type_name;
                let key = format!("{struct_name}#{fname}");
                // A concrete inherent method, or one instantiation of the
                // type's generic impl block (ADR-0187): the call site cannot
                // tell them apart, and does not need to.
                let call_offset = self.tokens[self.pos].offset;
                let generic = if self.generics.inherent.contains_key(&key) {
                    None
                } else if let Some(struct_sid) = sid {
                    // Struct instantiations and enum instantiations both
                    // resolve methods through their declaration's template.
                    let origin: Option<(usize, Vec<Type>)> = match base_ty {
                        Type::Struct(_) => self.generics.struct_origin.get(&struct_sid).cloned(),
                        Type::Enum(_, _) => self.generics.enum_origin.get(&struct_sid).cloned(),
                        _ => None,
                    };
                    if let Some((gid, origin_args)) = origin {
                        // The gid indexes the table the origin came from.
                        let def_name = match base_ty {
                            Type::Enum(..) => self.generics.defs[gid].name.clone(),
                            _ => self.generics.struct_defs[gid].name.clone(),
                        };
                        if self.generics.generic_impls.contains_key(&def_name) {
                            Some(self.instantiate_impl_method(&def_name, &fname, struct_sid, gid, &origin_args, call_offset)?)
                        } else { None }
                    } else { None }
                } else { None };
                let (call_id, receiver_ty, ret, full_params) = if let Some(method) = self.generics.inherent.get(&key).cloned() {
                    (method.signature.id, method.receiver, method.signature.ret, method.signature.params.clone())
                } else if let Some(instance) = generic {
                    let (id, params, ret) = instance;
                    let def_name = match sid {
                        Some(struct_sid) => match base_ty {
                            Type::Enum(..) => self.generics.enum_origin.get(&struct_sid).map(|&(gid, _)| self.generics.defs[gid].name.clone()),
                            _ => self.generics.struct_origin.get(&struct_sid).map(|&(gid, _)| self.generics.struct_defs[gid].name.clone()),
                        }.unwrap_or_else(|| struct_name.clone()),
                        None => struct_name.clone(),
                    };
                    let has_receiver = self.generics.generic_impls.get(&def_name)
                        .and_then(|def| def.methods.iter().find(|m| m.name == fname))
                        .map(|m| m.receiver.is_some())
                        .unwrap_or(false);
                    if has_receiver {
                        let receiver = params[0];
                        (id, Some(receiver), ret, params)
                    } else {
                        (id, None, ret, params)
                    }
                } else {
                    return Err(self.no_such_method(&struct_name, &fname, "method"));
                };
                let Some(receiver_ty) = receiver_ty else {
                    return Err(self.fail(&format!("'{struct_name}::{fname}' is an associated function, so it takes no receiver; call it as '{struct_name}::{fname}(...)'")));
                };
                let signature_id = call_id;
                let signature_ret = ret;
                // A borrowed receiver (ADR-0184, ADR-0185) auto-borrows at
                // the call site: the caller keeps ownership and only a bare
                // struct variable (or a borrow itself) can be lent, since the
                // borrow must be covered by storage that outlives the call.
                if matches!(receiver_ty, Type::StructRef(_) | Type::StructMut(_)) {
                    let wants_mut = matches!(receiver_ty, Type::StructMut(_));
                    let params = full_params[1..].to_vec();
                    let mut args = self.call_arguments(&format!("{struct_name}.{fname}"), &params)?;
                    match (&base, base_ty) {
                        (Expr::Variable(id, ..), Type::Struct(_)) => {
                            let id = *id;
                            if self.moved.contains(&id) {
                                return Err(moved_value_hint(self.binding_label(self.fail(&format!("cannot borrow the receiver of '{fname}' because it was moved")).with_code("E0102"), id)));
                            }
                            if wants_mut {
                                let mutable = self.names.values().find(|slot| slot.0 == id)
                                    .map(|slot| slot.1)
                                    .ok_or_else(|| self.fail("internal: receiver variable vanished"))?;
                                if !mutable {
                                    return Err(self.fail(&format!("'{fname}' borrows its receiver mutably, but the variable was declared without 'var'")));
                                }
                                if self.is_borrowed(id) {
                                    return Err(self.fail(&format!("cannot borrow the receiver of '{fname}' mutably while it is borrowed in this statement")));
                                }
                                self.mut_borrows.push((id, self.scopes.len()));
                            } else {
                                if self.mut_borrows.iter().any(|&(bid, _)| bid == id) {
                                    return Err(self.fail(&format!("cannot borrow the receiver of '{fname}' while it is mutably borrowed in this statement")));
                                }
                                if !self.is_borrowed(id) {
                                    self.call_borrows.push((id, self.scopes.len()));
                                }
                            }
                            args.insert(0, Argument::Borrow(id, true));
                        }
                        (Expr::Variable(id, ..), Type::StructRef(_)) => {
                            // A borrow parameter forwards its pointer; the
                            // caller's own borrow already covers the reads.
                            if wants_mut {
                                return Err(self.fail(&format!("'{fname}' borrows its receiver mutably, which a read-only borrow cannot grant; call it on the owned variable")));
                            }
                            args.insert(0, Argument::Borrow(*id, false));
                        }
                        (Expr::Variable(id, ..), Type::StructMut(_)) => {
                            // A mutable borrow forwards its pointer for either
                            // receiver form: writing shrinks to reading freely.
                            args.insert(0, Argument::Borrow(*id, false));
                        }
                        _ => return Err(self.fail(&format!("'{fname}' borrows its receiver, so the receiver must be a bare struct variable; bind the value to a variable first"))),
                    }
                    lhs = (Expr::Call(signature_id, args, signature_ret), signature_ret);
                    continue;
                }
                // The receiver is a by-value parameter, so an owned receiver
                // moves here exactly as it would in a plain call. A borrowed
                // struct has no value to move: its storage belongs to the
                // lender, so a by-value method is simply out of reach.
                if matches!(base_ty, Type::StructRef(_) | Type::StructMut(_)) {
                    return Err(self.fail(&format!("'{fname}' takes its receiver by value, but the receiver is a borrow with no value to move; call it on the owned variable")));
                }
                match &base {
                    Expr::Variable(id, ty, _, _) if ty.owned() => {
                        let id = *id;
                        if self.is_borrowed(id) {
                            return Err(self.fail(&format!("cannot move the receiver of '{fname}' while it is borrowed")));
                        }
                        if self.moved.contains(&id) {
                            return Err(moved_value_hint(self.binding_label(self.fail(&format!("cannot use a moved value as the receiver of '{fname}'")).with_code("E0102"), id)));
                        }
                        self.moved.insert(id);
                    }
                    Expr::Field(_, _, fty) if fty.owned() => {
                        return Err(self.fail(&format!("cannot move a field out of a struct to call '{fname}'; bind it first or pass it where a borrow works")));
                    }
                    _ => {}
                }
                let params = full_params[1..].to_vec();
                let mut args = self.call_arguments(&format!("{struct_name}.{fname}"), &params)?;
                args.insert(0, Argument::Value(base));
                lhs = (Expr::Call(signature_id, args, signature_ret), signature_ret);
                continue;
            }
            let Some(sid) = sid else {
                return Err(self.fail(&format!("enum '{}' has no fields; bind its variant payloads with 'match'", type_name)));
            };
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
        // be moved out of their struct. A binary operator changes what the
        // chain means: a string `+` consumes its left operand (so an owned
        // variable is marked here, before the right side can read it again),
        // while every other operator — and the `+` itself when the operand
        // is a field or element view — only reads.
        let operator_follows = matches!(self.current(),
            Kind::Plus | Kind::Minus | Kind::Star | Kind::Slash | Kind::Percent
            | Kind::EqEq | Kind::NotEq | Kind::Less | Kind::LessEq
            | Kind::Greater | Kind::GreaterEq | Kind::And | Kind::Or);
        let plus_follows = *self.current() == Kind::Plus;
        // Inside a string `+` the operands are read, so a field or an
        // element view is copied instead of rejected (concat.ae rule); an
        // owned variable is still consumed, checked and marked.
        let read_only_operand = self.plus_operand_depth > 0;
        if moving {
            let move_target = match (&lhs.0, lhs.1) {
                (_, ty) if !ty.owned() => None,
                (Expr::Variable(id, _, _, _), _) if !operator_follows || plus_follows => {
                    // A move under view borrows is deferred (M2.5 L1): the
                    // view may have died at its last read, in which case the
                    // move is fine. Container borrows reject on sight.
                    if self.is_borrowed(*id) && !self.all_borrows_are_views(*id) {
                        return Err(self.fail("cannot move a value while it is borrowed"));
                    }
                    if self.is_borrowed(*id) {
                        let offset = self.tokens.get(self.pos.saturating_sub(1)).map(|t| t.offset).unwrap_or(0);
                        let name = self.names.iter().find(|(_, (vid, _, _))| vid == id).map(|(n, _)| n.clone()).unwrap_or_else(|| format!("v{id}"));
                        self.deferred_write_conflicts.push((*id, offset, name, "move"));
                    }
                    Some(MoveKind::Var(*id))
                }
                (Expr::Field(_, _, fty), _) if fty.owned() && !operator_follows && !read_only_operand => Some(MoveKind::Field),
                (Expr::StrVecIndex(_, _, _, _, _), _) if !operator_follows && !read_only_operand => Some(MoveKind::StrVecElement),
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
            // A string `+` reads fields and vector elements rather than
            // rejecting them, so the right-hand side is parsed with the
            // plus-operand flag raised: variables are still move-checked,
            // but borrow-shaped operands are copied by the emitter.
            self.plus_operand_depth += 1;
            let parsed = self.expression(prec + 1, op == Kind::Plus);
            self.plus_operand_depth -= 1;
            let (rhs, rhs_ty) = parsed?;
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
                    // and views are borrowed and get copied by the emitter. The
                    // right operand follows the same rule.
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
}
