//! Parser and single-pass semantic checker: tokens to typed statements.
use super::*;

/// The remedy attached to a use-after-move diagnostic. The move is not tracked
/// to a byte range, so this names both available fixes instead of pointing at a
/// site that may not exist.
pub(super) fn moved_value_hint(diagnostic: ae_span::Diagnostic) -> ae_span::Diagnostic {
    diagnostic.with_suggestion("Owned values move on assignment or when passed by value; read what you need before the move, or build a fresh value.")
}

/// The remedy for `s = f(s)`, where the assignment target is consumed by the
/// call on its own right-hand side.
pub(super) fn moved_self_assignment_hint(diagnostic: ae_span::Diagnostic) -> ae_span::Diagnostic {
    diagnostic.with_suggestion("This call consumes the value being assigned to; bind the result to a new name, or read its fields into locals first.")
}

impl<'a> Parser<'a> {
    pub(super) fn current(&self) -> &Kind { &self.tokens[self.pos].kind }
    pub(super) fn fail(&self, message: &str) -> ae_span::Diagnostic {
        let token = &self.tokens[self.pos];
        ae_syntax::diagnostic_range(&self.file, self.source, token.offset..token.end, message)
    }
    pub(super) fn error_at(&self, offset: usize, message: &str) -> ae_span::Diagnostic {
        let end = self.tokens.binary_search_by_key(&offset, |token| token.offset)
            .ok().map_or(offset, |index| self.tokens[index].end);
        ae_syntax::diagnostic_range(&self.file, self.source, offset..end, message)
    }
    pub(super) fn binding_label(&self, diagnostic: ae_span::Diagnostic, id: usize) -> ae_span::Diagnostic {
        let Some(range) = self.binding_ranges.get(&id) else { return diagnostic };
        let source = ae_span::SourceFile { name: Some(&self.file), text: self.source };
        match source.span(range.clone()) {
            Some(span) => diagnostic.with_secondary(span, "binding declared here"),
            None => diagnostic,
        }
    }
    pub(super) fn immutable_binding_error(&self, diagnostic: ae_span::Diagnostic, id: usize) -> ae_span::Diagnostic {
        let diagnostic = self.binding_label(diagnostic, id);
        if diagnostic.secondary.is_empty() { diagnostic } else {
            diagnostic.with_suggestion("If mutation is intended, declare the binding with `var`.")
        }
    }
    pub(super) fn duplicate_binding_error(&self, name: &str) -> ae_span::Diagnostic {
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
        self.borrows.retain(|&(_, depth, _)| depth <= depth_now);
        self.call_borrows.retain(|&(_, depth)| depth <= depth_now);
        self.mut_borrows.retain(|&(_, depth)| depth <= depth_now);
        Ok(result)
    }
    // Is any borrow of this slot live? Two registers feed the answer:
    // `borrows` holds the persistent borrows (views, token cursors, the
    // containers of for-in loops, match subjects) that last until their
    // scope or loop ends, and `call_borrows` holds the borrows a call
    // argument created, which die when the statement that made the call
    // ends -- the callee has returned by then and nothing retains the
    // pointer (ADR-0184).
    // Whether every borrow on `id` is a view-kind borrow: only then can a
    // write conflict be deferred to the NLL adjudication (the view dies
    // after its last use; container borrows run to the scope's end).
    pub(super) fn all_borrows_are_views(&self, id: usize) -> bool {
        self.is_borrowed(id) && !self.borrows.iter().any(|&(bid, _, kind)| bid == id && kind == BorrowKind::Container)
    }

    // Whether the only active borrows on `id` are persistent view borrows:
    // the deferral precondition for a mut lending call (M2.5). Call-argument
    // borrows and earlier mut lends in the same statement are statement-
    // scoped exclusions the on-sight rule still guards (`c.doubled() +
    // c.inc()` stays rejected), and container borrows keep their scope rule.
    pub(super) fn only_view_borrows_active(&self, id: usize) -> bool {
        !self.call_borrows.iter().any(|&(bid, _)| bid == id)
            && !self.mut_borrows.iter().any(|&(bid, _)| bid == id)
            && self.borrows.iter().any(|&(bid, _, kind)| bid == id && kind == BorrowKind::View)
            && !self.borrows.iter().any(|&(bid, _, kind)| bid == id && kind == BorrowKind::Container)
    }

    pub(super) fn is_borrowed(&self, id: usize) -> bool {
        self.borrows.iter().any(|&(bid, _, _)| bid == id)
            || self.call_borrows.iter().any(|&(bid, _)| bid == id)
            || self.mut_borrows.iter().any(|&(bid, _)| bid == id)
    }

    // Adjudicate the write conflicts this function deferred (M2.5 L1): the
    // graph decides whether a view of the written slot may still be read.
    // A surviving conflict is the positioned rejection; everything else was
    // a write after the view's last use, which now parses.
    // Whether the graph's release scan will see a call through this
    // signature id: the scan reads the four tables every `Expr::Call` id
    // can come from. A deferral whose callee is outside them would be an
    // unsound allowance, so the deferral sites reject on sight instead.
    pub(super) fn signature_in_release_tables(&self, callee: Option<usize>) -> bool {
        let Some(id) = callee else { return false; };
        self.functions.values().any(|s| s.id == id)
            || self.generics.inherent.values().any(|m| m.signature.id == id)
            || self.generics.fn_done.values().any(|(iid, _, _)| *iid == id)
            || self.generics.generic_impl_done.values().any(|(iid, _, _)| *iid == id)
    }

    pub(super) fn adjudicate_deferred_conflicts(&mut self, function: &Function) -> Result<(), ae_span::Diagnostic> {
        let mut_sigs = borrowscope::mut_borrow_positions(self.functions, self.generics);
        let outcome = borrowscope::adjudicate_deferred(function, &self.deferred_write_conflicts, &mut_sigs);
        match outcome {
            Ok(()) => { self.deferred_write_conflicts.clear(); Ok(()) }
            Err(message) => {
                let (_, offset, _, _, _) = self.deferred_write_conflicts[0];
                Err(self.error_at(offset, &message))
            }
        }
    }
    // Views borrow their base: element reads and slice_views of variable
    // bases register the owner so it cannot be modified or moved while a
    // view can still be read.
    pub(super) fn record_borrow_bases(&mut self, e: &Expr) {
        match e {
            Expr::StrVecIndex(id, idx, _, _, _) => {
                if !self.is_borrowed(*id) {
                    self.borrows.push((*id, self.scopes.len(), BorrowKind::Container));
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
                        Argument::Borrow(_, _) | Argument::FnPtr(_) => {}
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
            // A call argument's borrow covered only the statement that made
            // the call; the next statement may write the struct again.
            self.call_borrows.clear();
            self.mut_borrows.clear();
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
                        if matches!(ty, Type::StructRef(_) | Type::StructMut(_)) {
                            return Err(self.fail("cannot store a borrowed struct in a variable; pass it directly to a parameter that borrows"));
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
                    // Kept for diagnostics that must point at the statement's
                    // own name rather than at whatever token follows it.
                    let name_offset = self.tokens[self.pos].offset;
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
                        if self.moved.contains(&id) { return Err(moved_value_hint(self.binding_label(self.fail(&format!("cannot use '{target}' because it was moved")).with_code("E0102"), id))); }
                        if !matches!(ty, Type::Vec | Type::StrVec | Type::VecF) { return Err(self.fail(&format!("{name} expects a vec, found {}", ty.name()))); }
                        if !mutable { return Err(self.immutable_binding_error(self.fail(&format!("cannot modify immutable variable '{target}'")).with_code("E0103"), id)); }
                        if self.is_borrowed(id) { return Err(self.fail(&format!("cannot modify '{target}' while it is borrowed"))); }
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
                        if self.is_borrowed(id) {
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
                        if self.is_borrowed(id) {
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
                        if self.is_borrowed(id) {
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
                        if self.is_borrowed(id) {
                            return Err(self.fail(&format!("cannot assign to '{name}' while it is borrowed")));
                        }
                        self.take(Kind::Equal)?;
                        let moved_before = self.moved.contains(&id);
                        let (value, value_ty) = self.expression(0, true)?; self.take(Kind::Semi)?;
                        if value_ty != ty { return Err(self.fail(&format!("cannot assign a {} value to {} variable", value_ty.name(), ty.name()))); }
                        if self.moved.contains(&id) && !moved_before {
                            return Err(moved_self_assignment_hint(self.binding_label(self.error_at(name_offset, &format!("cannot assign to '{name}' because it was moved in the same statement")).with_code("E0102"), id)));
                        }
                        self.moved.remove(&id);
                        result.push(Statement::Assign(id, value, true));
                    } else if let Type::StructRef(_) = ty {
                        // A struct borrow is read-only (ADR-0184): neither the
                        // pointer nor anything behind it may be written, so
                        // both spellings get their own diagnostic instead of
                        // falling into the scalar-assignment path.
                        if *self.current() == Kind::Dot {
                            return Err(self.fail(&format!("cannot write through '{name}': struct borrows are read-only in this version")));
                        }
                        return Err(self.fail(&format!("cannot assign to '{name}': struct borrows are read-only in this version")));
                    } else if let Type::StructMut(sid) = ty {
                        // A mutable borrow exists to write through (ADR-0185):
                        // field stores go through the pointer. Whole-struct
                        // assignment would have to release the old pointee
                        // before storing, which needs a dedicated statement —
                        // rejected for now rather than guessed at.
                        if *self.current() == Kind::Dot {
                            self.take(Kind::Dot)?;
                            let fname = self.identifier()?;
                            let fty = {
                                let def = self.structs[sid].clone();
                                let fidx = def.fields.iter().position(|(n, _)| *n == fname)
                                    .ok_or_else(|| self.fail(&format!("struct '{}' has no field '{}'", def.name, fname)))?;
                                def.fields[fidx].1
                            };
                            if matches!(fty, Type::Map | Type::MapF) {
                                return Err(self.fail("map fields cannot be written through a '&mut' borrow yet; copy the map out with a by-value receiver instead"));
                            }
                            self.take(Kind::Equal)?;
                            // An owned field write consumes a string/vec
                            // right side — the field owns the bytes after
                            // the write, so the source must forfeit them
                            // (the same rule the scalar assignment uses).
                            let (value, value_ty) = self.expression(0, true)?; self.take(Kind::Semi)?;
                            if value_ty != fty { return Err(self.fail(&format!("field '{}' must be {}", fname, fty.name()))); }
                            result.push(Statement::FieldStore(id, fname, fty, value, true));
                            continue;
                        }
                        return Err(self.fail(&format!("cannot assign to '{name}' itself through a '&mut' borrow; write its fields instead")));
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
                                if self.is_borrowed(id) {
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
                            // allowed once the view is gone (ADR-52) — the
                            // view-kind case is deferred to the graph's NLL
                            // adjudication at the variable granularity the
                            // borrow uses; container borrows reject on sight.
                            if self.is_borrowed(id) && !self.all_borrows_are_views(id) {
                                return Err(self.fail(&format!("cannot modify '{name}' while it is borrowed")));
                            }
                            if self.is_borrowed(id) {
                                self.deferred_write_conflicts.push((id, name_offset, name.clone(), "modify", Some(fname.clone())));
                            }
                            self.take(Kind::Equal)?;
                            // Same consuming rule as the `&mut` write: the
                            // field owns the bytes afterwards.
                            let (value, value_ty) = self.expression(0, true)?; self.take(Kind::Semi)?;
                            if value_ty != fty { return Err(self.fail(&format!("field '{}' must be {}", fname, fty.name()))); }
                            result.push(Statement::FieldStore(id, fname, fty, value, false));
                            continue;
                        }
                        // Whole-struct assignment moves a fresh value in.
                        if self.is_borrowed(id) {
                            return Err(self.fail(&format!("cannot assign to '{name}' while it is borrowed")));
                        }
                        self.take(Kind::Equal)?;
                        let moved_before = self.moved.contains(&id);
                        let (value, value_ty) = self.expression(0, true)?; self.take(Kind::Semi)?;
                        if value_ty != ty { return Err(self.fail(&format!("cannot assign a {} value to {} variable", value_ty.name(), ty.name()))); }
                        if self.moved.contains(&id) && !moved_before {
                            return Err(moved_self_assignment_hint(self.binding_label(self.error_at(name_offset, &format!("cannot assign to '{name}' because it was moved in the same statement")).with_code("E0102"), id)));
                        }
                        self.moved.remove(&id);
                        result.push(Statement::Assign(id, value, true));
                    } else {
                        // A string's borrows can be view-kind borrows, whose
                        // region dies after the view's last use (M2.5 L1):
                        // the conflict is deferred to the graph's
                        // adjudication instead of rejected on sight, so a
                        // write after the last view use parses. Container
                        // borrows keep the on-sight rejection.
                        if self.is_borrowed(id) && !self.all_borrows_are_views(id) {
                            return Err(self.fail(&format!("cannot assign to '{name}' while it is borrowed")));
                        }
                        if self.is_borrowed(id) {
                            self.deferred_write_conflicts.push((id, name_offset, name.clone(), "assign to", None));
                        }
                        self.take(Kind::Equal)?;
                        let moved_before = self.moved.contains(&id);
                        let (value, value_ty) = self.expression(0, true)?; self.take(Kind::Semi)?;
                        if value_ty != ty { return Err(self.fail(&format!("cannot assign a {} value to {} variable '{name}'", value_ty.name(), ty.name()))); }
                        // The right-hand side may not consume the target itself;
                        // assigning to an already-moved variable is a legal
                        // re-initialization, so the move flag clears either way.
                        if self.moved.contains(&id) && !moved_before {
                            return Err(moved_self_assignment_hint(self.binding_label(self.error_at(name_offset, &format!("cannot assign to '{name}' because it was moved in the same statement")).with_code("E0102"), id)));
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
                    if matches!(ty, Type::Closure(_)) { return Err(self.fail("a closure cannot be returned: it has no storage, and its captures live in this frame")); }
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
                        self.borrows.push((v_id, container_depth, BorrowKind::Container));
                        let body = self.block();
                        self.loop_labels.pop();
                        self.loop_scopes.pop();
                        self.scopes.pop().unwrap();
                        self.loop_depth -= 1;
                        self.names.remove(&var_name);
                        self.borrows.retain(|&(id, _, _)| id != v_id);
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
        self.borrows.push((sid, self.scopes.len(), BorrowKind::Container));
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
