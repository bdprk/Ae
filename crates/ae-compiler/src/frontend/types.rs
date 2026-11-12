//! Type syntax, definitions, constructors, and generic instantiation.
use super::*;

impl Parser<'_> {
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
                if other == "Self" {
                    // `Self` (ADR-0186) stands for the type the enclosing impl
                    // block targets; outside one, there is nothing to stand for.
                    return self.impl_self.clone().ok_or_else(|| {
                        self.fail("'Self' is only available inside an impl block")
                    });
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
        if std::env::var_os("AE_DEBUG_MIR").is_some() {
            eprintln!("struct literal debug: sid {} name '{}' at token {}", sid, name, self.pos);
        }
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
                Payload::Param(_) | Payload::Nested(_) => unreachable!("a non-generic enum has no type parameters"),
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
        let mut variants: Vec<(String, Vec<Type>)> = Vec::new();
        for (vname, fields) in def.variants.iter() {
            let mut types = Vec::new();
            for p in fields.iter() {
                types.push(self.resolve_payload(p, &args, offset)?);
            }
            variants.push((vname.clone(), types));
        }
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
    // A field type in a generic declaration: a type parameter, a concrete
    // named type, or a *nested instantiation* whose arguments may themselves
    // be the declaration's parameters — `inner: Pair[string, V]` inside
    // `struct Boxed[V]` stays a shape until the outer declaration is
    // instantiated (M1.10).
    pub(super) fn field_type(&mut self, params: &[String]) -> Result<Payload, ae_span::Diagnostic> {
        if let Kind::Ident(name) = self.current() {
            if let Some(index) = params.iter().position(|p| p == name) {
                self.pos += 1;
                return Ok(Payload::Param(index));
            }
            let is_generic_struct = self.generics.struct_names.contains_key(name.as_str());
            let is_generic_enum = self.generics.names.contains_key(name.as_str());
            if (is_generic_struct || is_generic_enum)
                && self.tokens.get(self.pos + 1).map(|t| &t.kind) == Some(&Kind::LBracket) {
                let gid = if is_generic_struct { self.generics.struct_names[name.as_str()] } else { self.generics.names[name.as_str()] };
                self.pos += 1; // the name
                self.pos += 1; // '['
                let mut args = Vec::new();
                loop {
                    args.push(self.payload_argument(params)?);
                    if *self.current() != Kind::Comma { break; }
                    self.pos += 1;
                }
                self.take(Kind::RBracket)?;
                let index = self.generics.nested_shapes.len();
                self.generics.nested_shapes.push(NestedShape { gid, is_struct: is_generic_struct, args });
                return Ok(Payload::Nested(index));
            }
        }
        Ok(Payload::Concrete(self.named_type()?))
    }

    // One argument inside a nested instantiation: the enclosing declaration's
    // parameter, another nested instantiation, or a concrete named type.
    fn payload_argument(&mut self, params: &[String]) -> Result<Payload, ae_span::Diagnostic> {
        if let Kind::Ident(name) = self.current() {
            if let Some(index) = params.iter().position(|p| p == name) {
                self.pos += 1;
                return Ok(Payload::Param(index));
            }
            let is_generic_struct = self.generics.struct_names.contains_key(name.as_str());
            let is_generic_enum = self.generics.names.contains_key(name.as_str());
            if (is_generic_struct || is_generic_enum)
                && self.tokens.get(self.pos + 1).map(|t| &t.kind) == Some(&Kind::LBracket) {
                return self.field_type(params);
            }
        }
        Ok(Payload::Concrete(self.named_type()?))
    }

    // Resolve one payload against an argument list, instantiating nested
    // shapes recursively. Shared by struct fields, enum payloads, and the
    // nested-shape resolver itself.
    fn resolve_payload(&mut self, payload: &Payload, args: &[Type], offset: usize) -> Result<Type, ae_span::Diagnostic> {
        match payload {
            Payload::Concrete(ty) => Ok(*ty),
            Payload::Param(index) => Ok(args[*index]),
            Payload::Nested(index) => {
                let shape = self.generics.nested_shapes[*index].clone();
                let mut resolved = Vec::new();
                for arg in &shape.args {
                    resolved.push(self.resolve_payload(arg, args, offset)?);
                }
                if shape.is_struct {
                    let sid = self.instantiate_struct(shape.gid, resolved, offset)?;
                    Ok(Type::Struct(sid))
                } else {
                    let eid = self.instantiate_enum(shape.gid, resolved, offset)?;
                    Ok(Type::Enum(self.enums[eid].name, eid))
                }
            }
        }
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
        let mut fields: Vec<(String, Type)> = Vec::new();
        for (fname, payload) in def.fields.iter() {
            let ty = self.resolve_payload(payload, &args, offset)?;
            fields.push((fname.clone(), ty));
        }
        let display = format!("{}[{}]", def.name, args.iter().map(|t| t.name()).collect::<Vec<_>>().join(", "));
        let sid = self.structs.len();
        self.structs.push(StructDef { name: display, fields, file: def.file, public: def.public });
        self.generics.struct_done.insert(key, sid);
        // Remember which declaration and arguments produced this layout, so
        // method calls on the instantiation can resolve through its generic
        // impl block (ADR-0187).
        self.generics.struct_origin.insert(sid, (gid, args));
        Ok(sid)
    }
    pub(super) fn parse_struct_def(&mut self, public: bool) -> Result<(), ae_span::Diagnostic> {
        self.pos += 1;
        let name_offset = self.tokens[self.pos].offset;
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
            // A non-generic struct may still carry nested instantiations
            // (`entries: Pair[i64, Pair[string, f64]]`); their arguments are
            // all concrete, so they resolve immediately with no parameters.
            // Resolution happens BEFORE the struct's own id is reserved: a
            // nested instantiation registers its own layout, and taking the
            // id first would hand Ledger's slot to the inner type.
            let mut concrete = Vec::new();
            for (fname, payload) in fields {
                let ty = match payload {
                    Payload::Concrete(ty) => ty,
                    Payload::Nested(_) => self.resolve_payload(&payload, &[], name_offset)?,
                    Payload::Param(_) => unreachable!("a non-generic struct has no type parameters"),
                };
                concrete.push((fname, ty));
            }
            let id = self.structs.len();
            self.struct_names.insert(name.clone(), id);
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
        // `fn(T, U) -> R` (ADR-0189): a function value. Only non-capturing
        // closures can be passed as one; the signature is interned so two
        // closures of the same shape share the type.
        if *self.current() == Kind::Fn {
            self.pos += 1;
            self.take(Kind::LParen)?;
            let mut params = Vec::new();
            if *self.current() != Kind::RParen {
                loop {
                    if params.len() >= 8 { return Err(self.fail("function type parameter limit exceeded (8)")); }
                    params.push(self.named_type()?);
                    if *self.current() != Kind::Comma { break; }
                    self.pos += 1;
                }
            }
            self.take(Kind::RParen)?;
            self.take(Kind::Arrow)?;
            let ret = self.return_annotation()?;
            if matches!(ret, Type::StructRef(_) | Type::StructMut(_) | Type::Fn(_)) {
                return Err(self.fail("a function type cannot return a borrow or another function type"));
            }
            let sig = super::closures::intern_fn_sig(params, ret);
            return Ok(Type::Fn(sig));
        }
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
            // `&mut Type` (ADR-0185) is the exclusive mutable borrow of a
            // user struct: the callee may write fields through the pointer,
            // and the caller may hold no other borrow of the struct while
            // the call runs.
            if *self.current() == Kind::Mut {
                self.pos += 1;
                let borrowed = match self.current() {
                    Kind::Ident(text) => text.clone(),
                    _ => return Err(self.fail("'&mut' must be followed by a struct type")),
                };
                if let Some(&sid) = self.struct_names.get(borrowed.as_str()) {
                    let def = &self.structs[sid];
                    self.require_public("struct", &def.name.clone(), &def.file.clone(), def.public)?;
                    self.pos += 1;
                    return Ok(Type::StructMut(sid));
                }
                if let Some(&gid) = self.generics.struct_names.get(borrowed.as_str()) {
                    let offset = self.tokens[self.pos].offset;
                    self.pos += 1;
                    if *self.current() != Kind::LBracket {
                        return Err(self.fail(&format!("'&mut {borrowed}' is a generic struct; write its type arguments, e.g. &mut {borrowed}[string]")));
                    }
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
                    return Ok(Type::StructMut(sid));
                }
                return Err(self.error_at(amp_offset, &format!(
                    "'&mut {borrowed}' borrows a struct mutably, but '{borrowed}' is {}; only user structs can be borrowed mutably",
                    if self.struct_names.contains_key(borrowed.as_str()) || self.generics.struct_names.contains_key(borrowed.as_str()) { "a struct" } else { "not a struct" })));
            }
            if matches!(self.current(), Kind::Ident(name) if name == "mut") {
                return Err(self.error_at(amp_offset,
                    "'&mut' does not exist yet: struct borrows are read-only, so a method that needs to mutate takes 'self' by value"));
            }
            if matches!(self.current(), Kind::Ident(_)) {
                // `&Struct` (ADR-0184) borrows a user struct read-only: the
                // callee reads fields through a pointer and the borrow's
                // lifetime is the call, the same contract the container
                // borrows already give. A generic instantiation borrows its
                // own instantiation, e.g. `&Pair[string, i64]`.
                let borrowed = match self.current() {
                    Kind::Ident(text) => text.clone(),
                    _ => unreachable!("the enclosing guard checked for an identifier"),
                };
                if let Some(&sid) = self.struct_names.get(borrowed.as_str()) {
                    let def = &self.structs[sid];
                    self.require_public("struct", &def.name.clone(), &def.file.clone(), def.public)?;
                    self.pos += 1;
                    return Ok(Type::StructRef(sid));
                }
                if let Some(&gid) = self.generics.struct_names.get(borrowed.as_str()) {
                    let offset = self.tokens[self.pos].offset;
                    self.pos += 1;
                    if *self.current() != Kind::LBracket {
                        return Err(self.fail(&format!("'&{borrowed}' is a generic struct; write its type arguments, e.g. &{borrowed}[string]")));
                    }
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
                    return Ok(Type::StructRef(sid));
                }
                return Err(self.error_at(amp_offset,
                    "unsupported borrow type; write &string, &vec, &vec[string], &vec[f64], &Struct, &[i64; N], or &T inside a generic function"));
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
        // No borrow can be returned: a borrow of a parameter would outlive the
        // callee's frame, and a borrow of a local would outlive the local.
        // (ADR-0184) Returning a value is always sound, and callers re-borrow.
        if *self.current() == Kind::Amp {
            return Err(self.fail("a function cannot return a borrow; return an owned value and let callers borrow it themselves"));
        }
        self.named_type()
    }
    // First pass collects only signatures. Bodies are checked in the second pass.
    // scan_uses validated the headers; skip them before collecting.
}
