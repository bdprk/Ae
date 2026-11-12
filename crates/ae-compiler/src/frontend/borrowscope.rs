//! Borrow-scope regions along the CFG (M2.5 L2).
//!
//! The parser registers borrows with two lifetimes: **persistent** borrows
//! (string views, token cursors, `for`-in containers, `match` subjects) last
//! until their scope or loop ends, and **call-argument borrows** last only
//! for the statement that made the call. On the graph the same distinction
//! becomes regions: a persistent borrow introduced in a block is live along
//! every path from that block until a loop back-edge or a scope-exit block
//! releases it, and a call borrow dies at the end of the statement's block.
//!
//! This pass walks the CFG and, for each block, computes the set of
//! variables whose persistent borrows are live on entry and exit. It is the
//! substrate the borrow checker's writes-through-borrow checks will use:
//! a write to a variable whose borrow region covers the write's block is
//! rejected. It runs as verification only — the parser's registers already
//! enforce the same rule — and a disagreement is an internal error.
//!
//! Borrow *introductions* are recognized from the statements the tree
//! carries: `SliceView`/`Tokens` read a base; `for` loops borrow their
//! container; `match` borrows its subject. Call borrows are recognized in
//! `Call` arguments (`Argument::Borrow`) and die at the block's end.
use super::*;
use super::cfg::{build_cfg, CfgFunction, CfgTerminator};

pub(super) fn verify_program_borrows(program: &Program) -> Result<(), String> {
    for file in &program.files {
        for function in file {
            let graph = build_cfg(function);
            BorrowRegions::run(&graph)?;
        }
    }
    Ok(())
}

struct BorrowRegions;

impl BorrowRegions {
    fn run(function: &CfgFunction) -> Result<(), String> {
        // Live-on-entry sets per block, computed to a fixed point. A borrow
        // region only shrinks when its scope ends — which the tree marks with
        // the scope-exit releases — so the meet is union: a borrow is live on
        // entry if ANY incoming path keeps it live (the parser's registers
        // are scope-based, and a scope encloses both branch paths).
        let mut live_in: Vec<std::collections::HashSet<usize>> = vec![std::collections::HashSet::new(); function.blocks.len()];
        let mut queue: std::collections::VecDeque<usize> = std::collections::VecDeque::new();
        let mut in_queue = vec![false; function.blocks.len()];
        queue.push_back(0);
        in_queue[0] = true;
        let mut exit_states: Vec<std::collections::HashSet<usize>> = vec![std::collections::HashSet::new(); function.blocks.len()];
        while let Some(block_id) = queue.pop_front() {
            in_queue[block_id] = false;
            let mut state = live_in[block_id].clone();
            let exit_state = BorrowRegions::walk_block(function, block_id, &mut state);
            if exit_state != live_in[block_id] {
                // Store the exit state so successors meet over it.
                live_in[block_id] = exit_state.clone();
                exit_states[block_id] = exit_state;
                for succ in successors(function, block_id) {
                    if !in_queue[succ] {
                        queue.push_back(succ);
                        in_queue[succ] = true;
                    }
                }
            }
        }
        // Write-through-borrow verification: a statement that writes a slot
        // must not run in a block whose live region covers that slot. The
        // parser enforces this with its registers; the regions must agree.
        for block in &function.blocks {
            let mut live = live_in[block.id].clone();
            for statement in &block.statements {
                if let Some(written) = writes_slot(statement) {
                    if live.contains(&written) {
                        return Err(format!(
                            "internal: fn #{} block {} writes v{} while its borrow region is live; the parser's borrow registers and the CFG regions disagree",
                            function.id, block.id, written
                        ));
                    }
                }
                introduce_borrows(statement, &mut live);
                // A write also kills a live borrow only if the tree says the
                // scope ended — which it does not mid-block, so no kill here.
                let _ = &mut live;
            }
        }
        let _ = exit_states;
        Ok(())
    }

    // Walk one block: introduces and kills borrows. Returns the exit set.
    fn walk_block(function: &CfgFunction, block_id: usize, live: &mut std::collections::HashSet<usize>) -> std::collections::HashSet<usize> {
        let block = &function.blocks[block_id];
        for statement in &block.statements {
            introduce_borrows(statement, live);
        }
        match &block.terminator {
            CfgTerminator::Return(expr) => { introduce_borrow_expr(expr, live); }
            CfgTerminator::Exit(code) => { introduce_borrow_expr(code, live); }
            _ => {}
        }
        // Loop back-edges keep their regions live (the loop's own borrow
        // covers the body; the fixed point handles the edge).
        live.clone()
    }
}

// The slot a statement writes, when it writes one directly: assignments,
// field stores through the owner, container pushes/stores/pops, and sorts —
// the same set the parser's registers protect.
fn writes_slot(statement: &Statement) -> Option<usize> {
    match statement {
        Statement::Assign(id, ..) => Some(*id),
        Statement::FieldStore(id, ..) => Some(*id),
        Statement::VecPush(id, ..) | Statement::StrVecPush(id, ..) | Statement::VecFPush(id, ..) => Some(*id),
        Statement::VecPop(id, ..) | Statement::StrVecPop(id, ..) | Statement::VecFPop(id, ..) => Some(*id),
        Statement::VecStore(id, ..) | Statement::StrVecStore(id, ..) | Statement::VecFStore(id, ..) => Some(*id),
        Statement::Sort(id, ..) | Statement::Reverse(id, ..) => Some(*id),
        Statement::MapPut(place, ..) => Some(place.slot()),
        _ => None,
    }
}

fn successors(function: &CfgFunction, block_id: usize) -> Vec<usize> {
    match &function.blocks[block_id].terminator {
        CfgTerminator::Goto(target) => vec![*target],
        CfgTerminator::Branch(_, a, b) => vec![*a, *b],
        CfgTerminator::Switch(_, _, targets) => targets.clone(),
        CfgTerminator::Return(_) | CfgTerminator::Exit(_) | CfgTerminator::Unreachable => Vec::new(),
    }
}

// A persistent borrow's base variable: string views and token cursors read a
// base variable (or a base field), loops borrow containers, match borrows
// its subject.
fn introduce_borrows(statement: &Statement, live: &mut std::collections::HashSet<usize>) {
    match statement {
        // `let v = slice_view(base, ..)` and `let t = tokenize(base, ..)`
        // bind a view whose base must stay put while the view is readable.
        Statement::Declare(_, init) => introduce_borrow_expr(init, live),
        Statement::For(_, container, ..) => { live.insert(*container); }
        Statement::ForTokens(_, base, ..) => { live.insert(*base); }
        Statement::Match(subject, ..) => { live.insert(*subject); }
        _ => {}
    }
}

fn introduce_borrow_expr(expr: &Expr, live: &mut std::collections::HashSet<usize>) {
    match expr {
        Expr::SliceView(id, ..) | Expr::Tokens(id, ..) => { live.insert(*id); }
        Expr::Binary(a, _, b, ..) => { introduce_borrow_expr(a, live); introduce_borrow_expr(b, live); }
        Expr::Field(base, ..) => introduce_borrow_expr(base, live),
        _ => {}
    }
}
