//! The flow-sensitive move walk over the control-flow graph (M2.5 L1).
//!
//! The parser checks moves as it parses, with branch merges it can express
//! in one pass. This pass re-derives the same verdicts **on the CFG**: each
//! block walks its statements with a moved-set, joins meet by intersection
//! (a variable is definitely moved only if both paths moved it), and loop
//! heads iterate to a fixed point. It runs on programs the parser already
//! accepted, so any violation it finds is a discrepancy between the parser's
//! conservative single-pass rules and the graph's dataflow — reported as an
//! internal error, because one of the two is wrong and the graph is the
//! authority on flow.
//!
//! The move sites mirror the parser's: a bare owned variable is moved by a
//! by-value call argument, a let initializer, an assignment's right side, a
//! `return`, a struct-literal field, an enum payload, and the left or right
//! operand of a string `+` (fields and vector elements are reads that copy;
//! borrows are never moves).
use super::*;
use super::cfg::{build_cfg, CfgFunction, CfgTerminator};

pub(super) fn verify_program_moves(program: &Program) -> Result<(), String> {
    for file in &program.files {
        for function in file {
            let graph = build_cfg(function);
            let walk = MoveWalk::run(&graph);
            if let Some((block, var_id)) = walk {
                return Err(format!(
                    "internal: fn #{} block {} uses a variable after it was moved on every path (v{}); the parser's move rules and the CFG disagree",
                    function.id, block, var_id
                ));
            }
        }
    }
    Ok(())
}

// None when every use respects the moves the graph computes; Some((block,
// var)) on the first use-after-move, for the internal-error message.
struct MoveWalk;

impl MoveWalk {
    fn run(function: &CfgFunction) -> Option<(usize, usize)> {
        let mut state: Vec<std::collections::HashSet<usize>> = vec![std::collections::HashSet::new(); function.blocks.len()];
        let entry = 0;
        // Worklist over blocks in id order; ids are assigned so a block's
        // successors always have higher ids except loop back-edges, which the
        // fixed-point iteration absorbs.
        let mut queue: std::collections::VecDeque<usize> = std::collections::VecDeque::new();
        let mut in_queue = vec![false; function.blocks.len()];
        queue.push_back(entry);
        in_queue[entry] = true;
        while let Some(block_id) = queue.pop_front() {
            in_queue[block_id] = false;
            let incoming: Vec<&std::collections::HashSet<usize>> = predecessors(function, block_id)
                .into_iter()
                .map(|pred| &state[pred])
                .collect();
            let start_state = if incoming.is_empty() {
                std::collections::HashSet::new()
            } else {
                // Definitely moved on every entry path: the intersection.
                let mut meet = incoming[0].clone();
                for other in &incoming {
                    meet.retain(|id| other.contains(id));
                }
                meet
            };
            let (end_state, violation) = MoveWalk::walk_block(function, block_id, start_state.clone());
            if let Some(violation) = violation {
                return Some(violation);
            }
            // Monotonic growth: the moved set only ever gains entries within
            // a block, so re-processing is bounded by the variable count.
            if end_state != state[block_id] {
                state[block_id] = end_state;
                for succ in successors(function, block_id) {
                    if !in_queue[succ] {
                        queue.push_back(succ);
                        in_queue[succ] = true;
                    }
                }
            }
        }
        None
    }

    // Walk one block's statements with the entry state; returns the exit
    // state and the first use-after-move, if any.
    fn walk_block(function: &CfgFunction, block_id: usize, mut moved: std::collections::HashSet<usize>) -> (std::collections::HashSet<usize>, Option<(usize, usize)>) {
        let block = &function.blocks[block_id];
        for statement in &block.statements {
            if let Some(violation) = walk_statement(statement, &mut Checked(&mut moved)) {
                return (moved, Some((block_id, violation)));
            }
        }
        match &block.terminator {
            CfgTerminator::Return(expr) => {
                if let Some(violation) = walk_moving_expr(expr, &mut Checked(&mut moved)) {
                    return (moved, Some((block_id, violation)));
                }
            }
            CfgTerminator::Exit(code) => {
                if let Some(violation) = walk_moving_expr(code, &mut Checked(&mut moved)) {
                    return (moved, Some((block_id, violation)));
                }
            }
            _ => {}
        }
        (moved, None)
    }
}

fn predecessors(function: &CfgFunction, block_id: usize) -> Vec<usize> {
    let mut preds = Vec::new();
    for block in &function.blocks {
        let hits = match &block.terminator {
            CfgTerminator::Goto(target) => *target == block_id,
            CfgTerminator::Branch(_, a, b) => *a == block_id || *b == block_id,
            CfgTerminator::Switch(_, _, targets) => targets.iter().any(|t| *t == block_id),
            CfgTerminator::Return(_) | CfgTerminator::Exit(_) | CfgTerminator::Unreachable => false,
        };
        if hits { preds.push(block.id); }
    }
    preds
}

fn successors(function: &CfgFunction, block_id: usize) -> Vec<usize> {
    match &function.blocks[block_id].terminator {
        CfgTerminator::Goto(target) => vec![*target],
        CfgTerminator::Branch(_, a, b) => vec![*a, *b],
        CfgTerminator::Switch(_, _, targets) => targets.clone(),
        CfgTerminator::Return(_) | CfgTerminator::Exit(_) | CfgTerminator::Unreachable => Vec::new(),
    }
}

// A tracker receives the walk's events. Two instances exist: the checking
// one (the move verifier, which is the parser's counterpart) and the
// recording one (M2.4 drop elaboration, which asks "which slots does this
// statement consume or re-initialize?" without checking anything). Sharing
// the walk keeps the two answers from drifting: there is one reading of
// what counts as a move, not two.
pub(super) trait MoveTracker {
    fn read(&mut self, id: usize) -> Option<usize>;
    fn consume(&mut self, id: usize) -> Option<usize>;
    fn mark(&mut self, id: usize);
    fn revive(&mut self, id: usize);
    // A call argument that lends its slot mutably: the callee may write any
    // field, so this releases the viewed bytes of the slot at this point in
    // evaluation order (M2.5). The default no-op keeps the move walkers
    // unchanged; the borrow adjudication is the consumer.
    fn mut_release(&mut self, _sig: usize, _position: usize, _slot: usize) {}
}

// Checks every read against the moved set; a violation is the id returned.
struct Checked<'a>(&'a mut std::collections::HashSet<usize>);

impl MoveTracker for Checked<'_> {
    fn read(&mut self, id: usize) -> Option<usize> {
        if self.0.contains(&id) { Some(id) } else { None }
    }
    fn consume(&mut self, id: usize) -> Option<usize> {
        if self.0.contains(&id) { return Some(id); }
        self.0.insert(id);
        None
    }
    fn mark(&mut self, id: usize) { self.0.insert(id); }
    fn revive(&mut self, id: usize) { self.0.remove(&id); }
}

// The moves and re-initializations one statement performs, in the order they
// happen (moves first: a statement's right side evaluates before its slot is
// written).
#[derive(Debug, Default)]
pub(super) struct MoveEffects {
    pub(super) moved: Vec<usize>,
    pub(super) revived: Vec<usize>,
}

// Records every slot a walk READS, in evaluation order — the M2.5 L1
// groundwork the NLL borrow regions need: a view's borrow region dies after
// its last read, and last reads are exactly the reads this tracker collects.
struct Reads(std::cell::RefCell<Vec<usize>>);

impl MoveTracker for Reads {
    fn read(&mut self, id: usize) -> Option<usize> { self.0.borrow_mut().push(id); None }
    fn consume(&mut self, id: usize) -> Option<usize> { self.0.borrow_mut().push(id); None }
    fn mark(&mut self, id: usize) { self.0.borrow_mut().push(id); }
    fn revive(&mut self, _id: usize) {}
}

// The slots one statement reads, in evaluation order. A by-value argument of
// an owned type appears as a read (consume) — the caller decides whether a
// read counts as a use for its analysis.
pub(super) fn statement_reads(statement: &Statement) -> Vec<usize> {
    let mut tracker = Reads(std::cell::RefCell::new(Vec::new()));
    let _ = walk_statement(statement, &mut tracker);
    tracker.0.into_inner()
}

// The slots one expression reads, in evaluation order — for terminators,
// whose shapes are expressions the statement walk does not cover.
pub(super) fn expr_reads(expr: &Expr) -> Vec<usize> {
    let mut tracker = Reads(std::cell::RefCell::new(Vec::new()));
    let _ = walk_expr(expr, &mut tracker);
    tracker.0.into_inner()
}

// Collects the events instead of checking them.
struct Recorded(MoveEffects);

impl MoveTracker for Recorded {
    fn read(&mut self, _id: usize) -> Option<usize> { None }
    fn consume(&mut self, id: usize) -> Option<usize> {
        if !self.0.moved.contains(&id) { self.0.moved.push(id); }
        None
    }
    fn mark(&mut self, id: usize) {
        if !self.0.moved.contains(&id) { self.0.moved.push(id); }
    }
    fn revive(&mut self, id: usize) { self.0.revived.push(id); }
}

// The slots a statement consumes by value and the slots it re-initializes.
// Callers that hand this a control statement get the effects of its nested
// lists in reading order; the MIR block graph has lowered those already, so
// the drop pass only asks this about the flat statements in a block.
pub(super) fn statement_move_effects(statement: &Statement) -> MoveEffects {
    let mut recorder = Recorded(MoveEffects::default());
    let _ = walk_statement(statement, &mut recorder);
    if std::env::var_os("AE_DROP_DEBUG").is_some() { eprintln!("DEBUG effects for {:?} = {:?}", format!("{statement:?}").chars().take(40).collect::<String>(), recorder.0); }
    recorder.0
}

// Walk a statement: reads are checked against the moved set, moves insert
// into it. Returns the id of a variable read after its move, if any.
pub(super) fn walk_statement(statement: &Statement, moved: &mut dyn MoveTracker) -> Option<usize> {
    match statement {
        // A return's expression is a moving position: an owned variable in it
        // is consumed by the return.
        Statement::Return(expr, _) => walk_moving_expr(expr, moved),
        // A declaration writes a fresh slot: the binding holds a value again,
        // even when the declaration re-runs on every iteration of a loop. The
        // init is a moving position — `let t = s;` consumes s.
        Statement::Declare(id, expr) => {
            let violation = walk_moving_expr(expr, moved);
            if violation.is_none() { moved.revive(*id); }
            violation
        }
        Statement::Assign(id, expr, _) => {
            // The right side evaluates first, then the slot is overwritten —
            // a re-assignment clears the slot's moved state. The right side
            // is a moving position — `t = s;` consumes s.
            if let Some(violation) = walk_moving_expr(expr, moved) { return Some(violation); }
            moved.revive(*id);
            None
        }
        Statement::Array(id, values) => {
            // Every element walks even after a clean one: `?` here would
            // abandon the rest of the list on the first None.
            let mut violation = None;
            for v in values {
                if let Some(found) = walk_expr(v, moved) { violation = Some(found); }
            }
            moved.revive(*id);
            violation
        }
        Statement::Store(_, _, index, value, ..) => {
            if let Some(violation) = walk_expr(index, moved) { return Some(violation); }
            if let Some(violation) = walk_expr(value, moved) { return Some(violation); }
            None
        }
        Statement::VecPush(_, value, ..) | Statement::StrVecPush(_, value, .., _) | Statement::VecFPush(_, value, ..) => walk_expr(value, moved),
        Statement::Sort(..) | Statement::Reverse(..) | Statement::VecPop(..) | Statement::StrVecPop(..) | Statement::VecFPop(..) => None,
        Statement::VecStore(_, index, value, ..) | Statement::StrVecStore(_, index, value, .., _) | Statement::VecFStore(_, index, value, ..) => {
            if let Some(violation) = walk_expr(index, moved) { return Some(violation); }
            if let Some(violation) = walk_expr(value, moved) { return Some(violation); }
            None
        }
        Statement::FieldStore(_, _, _, value, _) => walk_expr(value, moved),
        Statement::Print(printed) => match printed {
            Printed::Expr(e) => walk_expr(e, moved),
            Printed::Text(_) => None,
        },
        Statement::Assert(cond, ..) => walk_expr(cond, moved),
        Statement::If(cond, yes, no) => {
            if let Some(violation) = walk_expr(cond, moved) { return Some(violation); }
            if let Some(violation) = walk_statements_reads(yes, moved) { return Some(violation); }
            if let Some(violation) = walk_statements_reads(no, moved) { return Some(violation); }
            None
        }
        Statement::While(cond, body) => {
            if let Some(violation) = walk_expr(cond, moved) { return Some(violation); }
            if let Some(violation) = walk_statements_reads(body, moved) { return Some(violation); }
            None
        }
        // Loop containers and match subjects are borrowed for their whole
        // statement: the parser registers a borrow, not a move.
        Statement::For(_, container, body, ..) => {
            if let Some(violation) = moved.read(*container) { return Some(violation); }
            if let Some(violation) = walk_statements_reads(body, moved) { return Some(violation); }
            None
        }
        Statement::ForTokens(_, base, body, ..) => {
            if let Some(violation) = moved.read(*base) { return Some(violation); }
            if let Some(violation) = walk_statements_reads(body, moved) { return Some(violation); }
            None
        }
        Statement::Match(subject, _, arms) => {
            if let Some(violation) = moved.read(*subject) { return Some(violation); }
            for arm in arms {
                if let Some(violation) = walk_statements_reads(&arm.body, moved) { return Some(violation); }
            }
            None
        }
        // CFG-only (M2.3): a payload binding reads the subject it binds from,
        // and binds its own slot (a view, a borrow, or a copy — never a move
        // out of the subject).
        Statement::BindPayload(bid, subject, ..) => {
            if let Some(violation) = moved.read(*subject) { return Some(violation); }
            moved.revive(*bid);
            None
        }
        // Both operands walk even when the key walk finds nothing: `?` here
        // would treat the key's clean None as an early exit and abandon the
        // value's walk (the same truncation the Binary arm's failure log
        // entry documents).
        Statement::MapPut(_, key, value, ..) => {
            let key_walk = walk_expr(key, moved);
            let value_walk = walk_expr(value, moved);
            key_walk.or(value_walk)
        }
        // `?` binds its Ok payload in the slot the statement declared; the
        // releases it carries belong to the error path, which leaves the
        // function, so they say nothing about the continuing path.
        Statement::Propagate(expr, _, _, binding, _) => {
            // The binding revives even when the value walk is clean: `?` on
            // the walk returned the arm's None outright, the revive never
            // ran, the drop pass saw the bound slot as Uninit, and the
            // error-path release of the payload was elided — a real leak the
            // ASan CI run caught in copy_notes.ae (the third `?`-truncation
            // site; see the failure log).
            let violation = walk_expr(expr, moved);
            if let Some((id, _)) = binding { moved.revive(*id); }
            violation
        }
        Statement::Exit(code, _) => walk_expr(code, moved),
        Statement::Break | Statement::Continue(_) => None,
        // Scope-end releases sit where the storage left scope; the parser
        // makes any later read unrepresentable, so they add nothing here.
        Statement::Frees(_) => None,
    }
}

// Statements inside a lowered arm: the arm's body runs after the branch, so
// its reads apply to the current state. Arms that move and continue are the
// join's business (the walk handles them through the graph), so this walk
// checks reads and records moves in the same state the block walk carries —
// a simplification the fixed point corrects for loops.
fn walk_statements_reads(statements: &[Statement], moved: &mut dyn MoveTracker) -> Option<usize> {
    // A statement without a violation returns None — the normal case — so
    // the loop must keep walking, not `?`-return at the first one.
    for statement in statements {
        if let Some(violation) = walk_statement(statement, moved) { return Some(violation); }
    }
    None
}

pub(super) fn walk_expr(expr: &Expr, moved: &mut dyn MoveTracker) -> Option<usize> {
    match expr {
        Expr::Variable(id, ..) => {
            // A moving context consumes the value; the caller decides which
            // reads are moves, so nothing is inserted here.
            moved.read(*id)
        }
        Expr::Field(base, _, _) => walk_expr(base, moved),
        Expr::Call(sig, args, _) => {
            for (position, arg) in args.iter().enumerate() {
                match arg {
                    // By-value arguments move; borrows and views read.
                    Argument::Value(e) => {
                        // A bare owned variable as a by-value argument moves.
                        if let Some(violation) = walk_moving_expr(e, moved) { return Some(violation); }
                    }
                    Argument::View(e) => { if let Some(violation) = walk_expr(e, moved) { return Some(violation); } }
                    Argument::Borrow(id, _) => {
                        // The callee id names a signature, so the adjudication
                        // can tell whether this borrow lends mutably (the
                        // callee's parameter type says so).
                        moved.mut_release(*sig, position, *id);
                        if let Some(violation) = moved.read(*id) { return Some(violation); }
                    }
                    Argument::FnPtr(_) => {}
                }
            }
            None
        }
        Expr::FnCall(_, args, _) => {
            for arg in args {
                match arg {
                    // A `FnCall`'s callee is a variable and fn types cannot
                    // carry `&mut` (ADR-0189), so its borrows are reads only.
                    Argument::Value(e) => {
                        if let Some(violation) = walk_moving_expr(e, moved) { return Some(violation); }
                    }
                    Argument::View(e) => { if let Some(violation) = walk_expr(e, moved) { return Some(violation); } }
                    Argument::Borrow(id, _) => {
                        if let Some(violation) = moved.read(*id) { return Some(violation); }
                    }
                    Argument::FnPtr(_) => {}
                }
            }
            None
        }
        Expr::Binary(a, op, b, ..) => {
            // Both operands walk even after a violation: a plain read
            // returns None, and `?` here would abandon the walk before the
            // second operand — the recorder would miss every move to the
            // right of the first read, and the verifier's flow states would
            // stop there too.
            let left = walk_expr(a, moved);
            let right = walk_expr(b, moved);
            // `+` on strings consumes owned string operands.
            if *op == Kind::Plus {
                for side in [a, b] {
                    if let Expr::Variable(id, ty, ..) = &**side {
                        // `s + s` consumes one binding written twice; the
                        // reads above already passed, so this only records.
                        if ty.owned() { moved.mark(*id); }
                    }
                }
            }
            left.or(right)
        }
        Expr::StructLit(_, values) => {
            for v in values {
                // A bare owned variable moved into a field.
                if let Some(violation) = walk_moving_expr(v, moved) { return Some(violation); }
            }
            None
        }
        Expr::EnumLit(_, _, _, values) => {
            for v in values {
                if let Some(violation) = walk_moving_expr(v, moved) { return Some(violation); }
            }
            None
        }
        Expr::VecLit(values, ..) => {
            for v in values {
                if let Some(violation) = walk_moving_expr(v, moved) { return Some(violation); }
            }
            None
        }
        Expr::Index(_, _, index, ..) | Expr::VecIndex(_, index, ..) | Expr::StrVecIndex(_, index, ..) => walk_expr(index, moved),
        Expr::Len(a, ..) | Expr::Neg(a, ..) | Expr::Not(a) | Expr::ReadFile(a, ..) | Expr::Bytes(a, ..) | Expr::Trim(a, ..) | Expr::Lines(a, ..) | Expr::Utf8Len(a, ..) | Expr::ToString(a, ..) | Expr::FloatToString(a, ..) | Expr::ToF64(a, ..) | Expr::ParseInt(a, ..) | Expr::TryReadFile(a, ..) | Expr::TryListDir(a, ..) | Expr::PathExists(a, ..) | Expr::PathIsDir(a, ..) | Expr::PathName(a, ..) | Expr::PathParent(a, ..) | Expr::TryFileInfo(a, ..) | Expr::TryRemoveFile(a, ..) | Expr::TryRemoveDir(a, ..) | Expr::TryCreateDir(a, ..) | Expr::SetCwd(a, ..) | Expr::Env(a, ..) | Expr::ToI64(a, ..) => walk_expr(a, moved),
        Expr::Tokens(_, _, sep, ..) => walk_expr(sep, moved),
        Expr::StrEq(a, b, _) | Expr::Contains(a, b, ..) | Expr::Split(a, b, ..) | Expr::WriteFile(a, b, ..) | Expr::Slice(a, b, ..) | Expr::TryWriteFile(a, b, ..) | Expr::TryCopyFile(a, b, ..) | Expr::TryRenamePath(a, b, ..) | Expr::PathJoin(a, b, ..) | Expr::StartsWith(a, b) | Expr::EndsWith(a, b) | Expr::CountLinesWith(a, b, ..) | Expr::Find(a, b, ..) | Expr::JsonGet(a, b, ..) | Expr::JsonInt(a, b, ..) | Expr::JsonBool(a, b, ..) | Expr::JsonLen(a, b, ..) | Expr::TryAppendFile(a, b, ..) | Expr::Utf8Prefix(a, b, ..) => {
            let left = walk_expr(a, moved);
            let right = walk_expr(b, moved);
            left.or(right)
        }
        Expr::SliceView(_, _, start, end, ..) => {
            let left = walk_expr(start, moved);
            let right = walk_expr(end, moved);
            left.or(right)
        }
        Expr::Join(_, sep, ..) => walk_expr(sep, moved),
        Expr::MapGet(_, key, ..) | Expr::MapHas(_, key, ..) | Expr::MapRemove(_, key, ..) => walk_expr(key, moved),
        Expr::MapInc(_, key, delta, ..) => {
            let key_walk = walk_expr(key, moved);
            let delta_walk = walk_expr(delta, moved);
            key_walk.or(delta_walk)
        }
        Expr::MathCall(_, args, ..) => {
            let mut violation = None;
            for arg in args {
                if let Some(found) = walk_expr(arg, moved) { violation = Some(found); }
            }
            violation
        }
        // map_keys borrows its map; run_command borrows its argument vector.
        Expr::MapKeys(place, ..) => { let _ = place; None }
        Expr::RunCommand(_, vid, ..) => moved.read(*vid),
        Expr::ClosureLit(_) | Expr::NowMs | Expr::NowS | Expr::Args(..) | Expr::ReadStdin(..) | Expr::Cwd(..) | Expr::StrVecNew(..) | Expr::F64VecNew(..) | Expr::MapNew(..) | Expr::JsonHas(..) => None,
        // Literals and constants own nothing and reference nothing.
        Expr::Number(_) | Expr::FloatLit(_) | Expr::BoolLit(_) | Expr::StrLit(..) => None,
    }
}

// A read in a moving position: an owned variable is consumed (checked, then
// marked); any other shape is a read.
fn walk_moving_expr(expr: &Expr, moved: &mut dyn MoveTracker) -> Option<usize> {
    if let Expr::Variable(id, ty, ..) = expr {
        if ty.owned() { return moved.consume(*id); }
        return moved.read(*id);
    }
    walk_expr(expr, moved)
}
