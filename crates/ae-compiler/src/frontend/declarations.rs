//! Function and trait declaration collection and instantiation.
use super::*;
use super::parser::moved_value_hint;

impl Parser<'_> {
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
    // Inherent `impl Type { fn m(self) ... }` (M1.2, v1). A method is an
    // ordinary function whose parameter 0 is the receiver, so nothing new is
    // needed downstream: the backend already emits calls and moves. What is new
    // is the (type, method) name space that `value.m(...)` resolves through, and
    // the receiver spelling that lets a body read `self.field`.
    fn collect_inherent_impl(&mut self, type_name: String, next_id: &mut usize) -> Result<(), ae_span::Diagnostic> {
        if self.generics.struct_names.contains_key(&type_name) {
            return Err(self.fail(&format!("'{type_name}' is a generic struct; inherent impls on generic types are not supported yet — write a plain function")));
        }
        let sid = *self.struct_names.get(&type_name).ok_or_else(|| {
            self.fail(&format!("impl names unknown type '{type_name}'; declare it with 'struct {type_name} {{ ... }}'"))
        })?;
        // Coherence, same shape as traits: the impl lives with the type, so two
        // files can never give one type two different methods of the same name.
        let owner = self.structs[sid].file.clone();
        if owner != self.file {
            return Err(self.fail(&format!("an inherent impl for '{type_name}' must be in the file that declares it ({owner})")));
        }
        self.take(Kind::LBrace)?;
        while *self.current() != Kind::RBrace {
            if *self.current() == Kind::Eof { return Err(self.fail("expected '}' before end of file")); }
            // `pub` is accepted and means nothing here: a method is exactly as
            // visible as the type it belongs to, so the marker would promise
            // something the type itself does not grant.
            self.take_pub();
            self.take(Kind::Fn)?;
            let method = self.identifier()?;
            if method == "main" { return Err(self.fail("a method cannot be named main")); }
            if RESERVED.contains(&method.as_str()) {
                return Err(self.fail(&format!("'{method}' is a built-in name and cannot be a method")));
            }
            if self.structs[sid].fields.iter().any(|(name, _)| *name == method) {
                return Err(self.fail(&format!("method '{method}' collides with the field of the same name on '{type_name}'; 'value.{method}' has to mean one thing, so rename one")));
            }
            let key = format!("{type_name}#{method}");
            if self.generics.inherent.contains_key(&key) {
                return Err(self.fail(&format!("duplicate method '{method}' for '{type_name}'")));
            }
            if self.generics.inherent.len() >= 256 { return Err(self.fail("method limit exceeded (256 per program)")); }
            self.take(Kind::LParen)?;
            let mut types = vec![Type::Struct(sid)];
            self.take_receiver(&type_name)?;
            while *self.current() == Kind::Comma {
                self.pos += 1;
                if types.len() >= 8 { return Err(self.fail("method parameter limit exceeded (8)")); }
                let _param = self.identifier()?;
                self.take(Kind::Colon)?;
                types.push(self.parameter_type()?);
            }
            self.take(Kind::RParen)?;
            self.take(Kind::Arrow)?;
            let ret = self.parameter_type()?;
            self.generics.next_fn_id = self.generics.next_fn_id.max(*next_id);
            let id = self.generics.next_fn_id;
            self.generics.next_fn_id += 1;
            *next_id = self.generics.next_fn_id;
            self.id_order.push(id);
            self.generics.inherent.insert(key, Signature { id, params: types, ret, file: self.file.clone(), public: true });
            self.skip_balanced_braces()?;
        }
        self.take(Kind::RBrace)
    }

    // The receiver is spelled `self` and nothing else. A type annotation is
    // rejected rather than accepted-and-ignored, because the impl already fixed
    // the type: writing it twice invites the two to disagree.
    fn take_receiver(&mut self, type_name: &str) -> Result<(), ae_span::Diagnostic> {
        if *self.current() == Kind::Amp {
            return Err(self.fail("'&self' is not supported: borrows exist only for string, vec, and map, so a method takes its receiver by value as 'self'"));
        }
        if *self.current() == Kind::RParen {
            return Err(self.fail("an inherent method must take 'self' as its first parameter"));
        }
        let param = self.identifier()?;
        if param != "self" {
            return Err(self.fail(&format!("the first parameter of an inherent method must be 'self', found '{param}'")));
        }
        if *self.current() == Kind::Colon {
            return Err(self.fail(&format!("write the receiver as 'self'; it already has type '{type_name}'")));
        }
        Ok(())
    }

    pub(super) fn collect_impl(&mut self, next_id: &mut usize) -> Result<(), ae_span::Diagnostic> {
        self.pos += 1; // impl
        let first = self.identifier()?;
        // `impl Type { ... }` carries no `for`, so that name is the type itself.
        if *self.current() != Kind::For { return self.collect_inherent_impl(first, next_id); }
        let trait_name = first;
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
            self.take_pub();
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
        let first = self.identifier()?;
        // The same shape decision the signature pass made, asked again from the
        // tokens: `for` means a trait impl, its absence an inherent one.
        let receiver = if *self.current() == Kind::For {
            self.take(Kind::For)?;
            self.parameter_type()?;
            None
        } else {
            let sid = *self.struct_names.get(&first).ok_or_else(|| self.fail(&format!("unknown struct '{first}'")))?;
            Some(Type::Struct(sid))
        };
        self.take(Kind::LBrace)?;
        while *self.current() != Kind::RBrace {
            if *self.current() == Kind::Eof { return Err(self.fail("expected '}' before end of file")); }
            self.take_pub();
            self.take(Kind::Fn)?;
            let _method = self.identifier()?;
            let id = self.id_order[result.len()];
            // The body pass knows the receiver's type; the parameter list does
            // not repeat it, so it is carried in the parser state instead.
            self.receiver = receiver;
            let parsed = self.function_rest(false, id);
            self.receiver = None;
            result.push(parsed?);
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
                // A receiver carries its type from the impl, so the parameter
                // list never writes it (M1.2). Everywhere else `self` is just
                // an unknown name, which keeps the spelling from leaking out of
                // method bodies.
                if param == "self" {
                    let Some(receiver) = self.receiver else {
                        return Err(self.fail("'self' is only available as the first parameter of an inherent impl method"));
                    };
                    if !params.is_empty() { return Err(self.fail("'self' must be the first parameter")); }
                    let var_id = self.next_var; self.next_var += 1;
                    self.names.insert(param, (var_id, mutable, receiver));
                    self.binding_ranges.insert(var_id, binding_range);
                    params.push((var_id, receiver));
                    if *self.current() != Kind::Comma { break; }
                    self.pos += 1;
                    continue;
                }
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
            borrows: Vec::new(), scopes: Vec::new(), subst, trait_bounds, receiver: None, pending: Vec::new(),
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
                    if self.moved.contains(&var) { return Err(moved_value_hint(self.binding_label(self.fail(&format!("cannot borrow '{arg}' because it was moved")).with_code("E0102"), var))); }
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
}
