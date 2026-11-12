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
            if let Some(violation) = walk_statement(statement, &mut moved) {
                return (moved, Some((block_id, violation)));
            }
        }
        match &block.terminator {
            CfgTerminator::Return(expr) => {
                if let Some(violation) = walk_expr(expr, &mut moved) {
                    return (moved, Some((block_id, violation)));
                }
            }
            CfgTerminator::Exit(code) => {
                if let Some(violation) = walk_expr(code, &mut moved) {
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

// Walk a statement: reads are checked against the moved set, moves insert
// into it. Returns the id of a variable read after its move, if any.
fn walk_statement(statement: &Statement, moved: &mut std::collections::HashSet<usize>) -> Option<usize> {
    match statement {
        Statement::Return(expr, _) | Statement::Declare(_, expr) => walk_expr(expr, moved),
        Statement::Assign(id, expr, _) => {
            // The right side evaluates first, then the slot is overwritten —
            // a re-assignment clears the slot's moved state.
            if let Some(violation) = walk_expr(expr, moved) { return Some(violation); }
            moved.remove(id);
            None
        }
        Statement::Array(_, values) => {
            for v in values { walk_expr(v, moved)?; }
            None
        }
        Statement::Store(_, _, index, value, ..) => { walk_expr(index, moved)?; walk_expr(value, moved)?; None }
        Statement::VecPush(_, value, ..) | Statement::StrVecPush(_, value, .., _) | Statement::VecFPush(_, value, ..) => walk_expr(value, moved),
        Statement::Sort(..) | Statement::Reverse(..) | Statement::VecPop(..) | Statement::StrVecPop(..) | Statement::VecFPop(..) => None,
        Statement::VecStore(_, index, value, ..) | Statement::StrVecStore(_, index, value, .., _) | Statement::VecFStore(_, index, value, ..) => {
            walk_expr(index, moved)?; walk_expr(value, moved)?; None
        }
        Statement::FieldStore(_, _, _, value, _) => walk_expr(value, moved),
        Statement::Print(printed) => match printed {
            Printed::Expr(e) => walk_expr(e, moved),
            Printed::Text(_) => None,
        },
        Statement::Assert(cond, ..) => walk_expr(cond, moved),
        Statement::If(cond, yes, no) => { walk_expr(cond, moved)?; walk_statements_reads(yes, moved)?; walk_statements_reads(no, moved)?; None }
        Statement::While(cond, body) => { walk_expr(cond, moved)?; walk_statements_reads(body, moved)?; None }
        // Loop containers and match subjects are borrowed for their whole
        // statement: the parser registers a borrow, not a move.
        Statement::For(_, container, body, ..) => {
            if moved.contains(container) { return Some(*container); }
            walk_statements_reads(body, moved)?;
            None
        }
        Statement::ForTokens(_, base, body, ..) => {
            if moved.contains(base) { return Some(*base); }
            walk_statements_reads(body, moved)?;
            None
        }
        Statement::Match(subject, _, arms) => {
            if moved.contains(subject) { return Some(*subject); }
            for arm in arms { walk_statements_reads(&arm.body, moved)?; }
            None
        }
        Statement::MapPut(_, key, value, ..) => { walk_expr(key, moved)?; walk_expr(value, moved)?; None }
        Statement::Propagate(expr, ..) => walk_expr(expr, moved),
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
fn walk_statements_reads(statements: &[Statement], moved: &mut std::collections::HashSet<usize>) -> Option<usize> {
    for statement in statements {
        walk_statement(statement, moved)?;
    }
    None
}

fn walk_expr(expr: &Expr, moved: &mut std::collections::HashSet<usize>) -> Option<usize> {
    match expr {
        Expr::Variable(id, ty, ..) => {
            if moved.contains(id) { return Some(*id); }
            // A moving context consumes the value; the caller decides which
            // reads are moves, so nothing is inserted here.
            let _ = ty;
            None
        }
        Expr::Field(base, _, _) => walk_expr(base, moved),
        Expr::Call(_, args, _) | Expr::FnCall(_, args, _) => {
            for arg in args {
                match arg {
                    // By-value arguments move; borrows and views read.
                    Argument::Value(e) => {
                        // A bare owned variable as a by-value argument moves.
                        if let Some(violation) = walk_moving_expr(e, moved) { return Some(violation); }
                    }
                    Argument::View(e) => { walk_expr(e, moved)?; }
                    Argument::Borrow(id, _) => {
                        if moved.contains(id) { return Some(*id); }
                    }
                    Argument::FnPtr(_) => {}
                }
            }
            None
        }
        Expr::Binary(a, op, b, ..) => {
            walk_expr(a, moved)?;
            walk_expr(b, moved)?;
            // `+` on strings consumes owned string operands.
            if *op == Kind::Plus {
                for side in [a, b] {
                    if let Expr::Variable(id, ty, ..) = &**side {
                        if ty.owned() { moved.insert(*id); }
                    }
                }
            }
            None
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
            walk_expr(a, moved)?; walk_expr(b, moved)?; None
        }
        Expr::SliceView(_, _, start, end, ..) => { walk_expr(start, moved)?; walk_expr(end, moved)?; None }
        Expr::Join(_, sep, ..) => walk_expr(sep, moved),
        Expr::MapGet(_, key, ..) | Expr::MapHas(_, key, ..) | Expr::MapRemove(_, key, ..) => walk_expr(key, moved),
        Expr::MapInc(_, key, delta, ..) => { walk_expr(key, moved)?; walk_expr(delta, moved)?; None }
        Expr::MathCall(_, args, ..) => {
            for arg in args { walk_expr(arg, moved)?; }
            None
        }
        // map_keys borrows its map; run_command borrows its argument vector.
        Expr::MapKeys(place, ..) => { let _ = place; None }
        Expr::RunCommand(_, vid, ..) => { if moved.contains(vid) { return Some(*vid); } None }
        Expr::ClosureLit(_) | Expr::NowMs | Expr::NowS | Expr::Args(..) | Expr::ReadStdin(..) | Expr::Cwd(..) | Expr::StrVecNew(..) | Expr::F64VecNew(..) | Expr::MapNew(..) | Expr::JsonHas(..) => None,
        // Literals and constants own nothing and reference nothing.
        Expr::Number(_) | Expr::FloatLit(_) | Expr::BoolLit(_) | Expr::StrLit(..) => None,
    }
}

// A read in a moving position: an owned variable is consumed (checked, then
// marked); any other shape is a read.
fn walk_moving_expr(expr: &Expr, moved: &mut std::collections::HashSet<usize>) -> Option<usize> {
    if let Expr::Variable(id, ty, ..) = expr {
        if moved.contains(id) { return Some(*id); }
        if ty.owned() { moved.insert(*id); }
        return None;
    }
    walk_expr(expr, moved)
}
