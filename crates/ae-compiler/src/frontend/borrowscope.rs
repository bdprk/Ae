//! Borrow-scope regions along the CFG (M2.5).
//!
//! The parser registers borrows with two lifetimes: **persistent** borrows
//! (string views, token cursors, `for`-in containers, `match` subjects) last
//! until their scope or loop ends, and **call-argument borrows** last only
//! for the statement that made the call. On the graph the same distinction
//! becomes regions, one shape per borrow kind:
//!
//! - **Scope regions** (union meets, fixed point) for the borrows whose
//!   whole-statement shape the tree carries: a `for`'s container and a
//!   `match`'s subject are read by the loop's own dispatch for as long as
//!   the loop runs, so the region is the parser's scope rule expressed on
//!   the graph.
//! - **NLL regions** (backwards liveness of the view slot) for the borrows
//!   that hang off a view binding: `let v = slice_view(base, ..)` and
//!   `let cur = tokens(base, ..)`. A view is never reassigned, so its
//!   region is exactly the points where the view may still be read — the
//!   borrow dies after its last use, which is the M2.5 L1 relaxation
//!   computed here and consumed once enforcement moves onto the graph.
//!
//! This pass is verification only — the parser's registers already enforce
//! the strict (scope) rule, and every accepted program therefore keeps
//! every write outside every region under either shape. A disagreement is
//! an internal error. The NLL regions are a subset of the scope regions by
//! construction, so the check below stays silent where the parser was
//! silent; the pass runs inside every compile as a regression tripwire.
//!
//! Borrow *introductions* are recognized from the statements the tree
//! carries: `SliceView`/`Tokens` read a base; `for` loops borrow containers;
//! `match` borrows its subject. Call borrows are recognized in `Call`
//! arguments (`Argument::Borrow`) and die at the block's end.
use std::collections::HashMap;

use super::*;
use super::cfg::{build_cfg, CfgFunction, CfgTerminator};
use super::movewalk::{expr_reads, statement_move_effects, statement_reads};

// Adjudicate the parser's deferred write-under-view-borrow conflicts (M2.5
// L1): the parser no longer rejects a write to a string whose view may have
// died, it records the candidate and the finished function is checked here
// against the NLL regions. A write whose view may still be read later on
// some path is the real rejection — the write releases the bytes the view
// points at; a write after the view's last use is the relaxation.
//
// Every write to the base in the CFG is a deferred one (the parser defers
// all of them when only view borrows are active), so one covered write
// anywhere is reported at the first recorded conflict's site.
pub(super) fn adjudicate_deferred(function: &Function, conflicts: &[(usize, usize, String, &'static str)]) -> Result<(), String> {
    if conflicts.is_empty() { return Ok(()); }
    let graph = build_cfg(function);
    let view_borrows: Vec<(usize, usize)> = graph.blocks.iter()
        .flat_map(|block| block.statements.iter())
        .filter_map(view_borrow_introduced)
        .collect();
    let introductions: HashMap<usize, (usize, usize)> = graph.blocks.iter()
        .flat_map(|block| {
            block.statements.iter().enumerate()
                .filter_map(|(index, statement)| view_borrow_introduced(statement).map(|(view, _)| (view, (block.id, index))))
        })
        .collect();
    let nll = NllRegions::compute(&graph, &view_borrows);
    let dom = dominators(&graph);
    for block in &graph.blocks {
        let mut suffix: Vec<std::collections::HashSet<usize>> = vec![std::collections::HashSet::new(); block.statements.len() + 1];
        for index in (0..block.statements.len()).rev() {
            let mut here = suffix[index + 1].clone();
            here.extend(statement_reads(&block.statements[index]));
            suffix[index] = here;
        }
        suffix[block.statements.len()].extend(match &block.terminator {
            CfgTerminator::Return(expr) | CfgTerminator::Exit(expr) => expr_reads(expr),
            CfgTerminator::Branch(cond, ..) => expr_reads(cond),
            CfgTerminator::Switch(subject, ..) => vec![*subject],
            _ => Vec::new(),
        });
        let live_out: std::collections::HashSet<usize> = successors(&graph, block.id)
            .iter()
            .flat_map(|succ| nll.live_in.get(succ).into_iter().flatten().copied())
            .collect();
        for (index, statement) in block.statements.iter().enumerate() {
            // Both kinds of sites re-home or release the bytes a view points
            // at: a write releases them in place; a move hands them to a new
            // owner that may die before the view does. Either way the view
            // must be dead by then — introduced before the statement
            // dominates it and read on no later path.
            let mut targets = std::collections::HashSet::new();
            if let Some(written) = writes_slot(statement) { targets.insert(written); }
            targets.extend(statement_move_effects(statement).moved);
            if targets.is_empty() { continue; }
            let mut live_at_write = suffix[index + 1].clone();
            live_at_write.extend(&live_out);
            let covered = view_borrows.iter()
                .any(|&(view, base)| targets.contains(&base)
                    && view_covers_write(view, block.id, index, &introductions, &dom, &live_at_write));
            if covered {
                let (_, _, name, verb) = &conflicts[0];
                return Err(format!("cannot {verb} '{name}' while a view binding of it may still be read"));
            }
        }
    }
    Ok(())
}

pub(super) fn verify_program_borrows(program: &Program) -> Result<(), String> {
    for file in &program.files {
        for function in file {
            let graph = build_cfg(function);
            BorrowRegions::run(&graph)?;
        }
    }
    Ok(())
}

// The view borrows one statement introduces: `let v = slice_view(base, ..)`
// and `let cur = tokens(base, ..)` bind a view of the base slot. The base of
// a field view (`slice_view(p.name, ..)`) is the struct itself, at the
// granularity the parser's registers use (ADR-52).
fn view_borrow_introduced(statement: &Statement) -> Option<(usize, usize)> {
    match statement {
        Statement::Declare(view, Expr::SliceView(base, ..)) => Some((*view, *base)),
        Statement::Declare(view, Expr::Tokens(base, ..)) => Some((*view, *base)),
        _ => None,
    }
}

// A write conflicts with a view only when the view was introduced BEFORE the
// write on every path to it — the introduction dominates the write — and the
// view may still be read later. A view created after the write looks at the
// new bytes, never at the ones the write releases; a write is not on the
// path from an introduction to a read unless the introduction dominates it,
// because a read without a dominating introduction is a definite-assignment
// rejection the parser already makes.
fn view_covers_write(
    view: usize,
    write_block: usize,
    write_index: usize,
    introductions: &HashMap<usize, (usize, usize)>,
    dom: &[std::collections::HashSet<usize>],
    live_at_write: &std::collections::HashSet<usize>,
) -> bool {
    if !live_at_write.contains(&view) { return false; }
    match introductions.get(&view) {
        Some(&(intro_block, intro_index)) if intro_block == write_block => intro_index < write_index,
        Some(&(intro_block, _)) => dom[write_block].contains(&intro_block),
        None => false,
    }
}

struct BorrowRegions;

impl BorrowRegions {
    fn run(function: &CfgFunction) -> Result<(), String> {
        // The whole-statement borrows keep their union-meet regions: a
        // borrow is live on entry to a block if ANY incoming path keeps it
        // live, because a scope encloses both branch paths.
        let scope_regions = ScopeRegions::compute(function);
        // The view borrows get NLL regions: backwards liveness of the view
        // slot, which never dies by assignment (views are declare-only), so
        // a view is live exactly where some later path may still read it.
        let view_borrows: Vec<(usize, usize)> = function.blocks.iter()
            .flat_map(|block| block.statements.iter())
            .filter_map(view_borrow_introduced)
            .collect();
        let nll = NllRegions::compute(function, &view_borrows);
        let introductions: HashMap<usize, (usize, usize)> = function.blocks.iter()
            .flat_map(|block| {
                block.statements.iter().enumerate()
                    .filter_map(|(index, statement)| view_borrow_introduced(statement).map(|(view, _)| (view, (block.id, index))))
            })
            .collect();
        let dom = dominators(function);

        // Write-through-borrow verification: a statement that writes a slot
        // must not run inside a region covering that slot. The parser
        // enforces this with its registers (the strict scope rule); the
        // regions computed here must cover the same writes and more.
        for block in &function.blocks {
            // View slots read later in this block, by statement index:
            // suffix[i] holds the views read by statements i..end and by
            // the terminator, so a write at statement i conflicts with
            // suffix[i + 1] — the write's own right side reads first.
            let mut suffix: Vec<std::collections::HashSet<usize>> = vec![std::collections::HashSet::new(); block.statements.len() + 1];
            for index in (0..block.statements.len()).rev() {
                let mut here = suffix[index + 1].clone();
                here.extend(statement_reads(&block.statements[index]));
                suffix[index] = here;
            }
            suffix[block.statements.len()].extend(match &block.terminator {
                CfgTerminator::Return(expr) | CfgTerminator::Exit(expr) => expr_reads(expr),
                CfgTerminator::Branch(cond, ..) => expr_reads(cond),
                CfgTerminator::Switch(subject, ..) => vec![*subject],
                _ => Vec::new(),
            });

            let scope_live = &scope_regions.live_in[block.id];
            // Views live on entry to any successor: reads later on paths
            // that leave this block before it ends.
            let live_out: std::collections::HashSet<usize> = successors(function, block.id)
                .iter()
                .flat_map(|succ| nll.live_in.get(succ).into_iter().flatten().copied())
                .collect();
            for (index, statement) in block.statements.iter().enumerate() {
                let mut targets = std::collections::HashSet::new();
                if let Some(written) = writes_slot(statement) { targets.insert(written); }
                targets.extend(statement_move_effects(statement).moved);
                if targets.is_empty() { continue; }
                // The strict rule the parser enforces: no write inside a
                // scope region.
                for written in &targets {
                    if scope_live.contains(written) {
                        return Err(format!(
                            "internal: fn #{} block {} writes v{} while its scope borrow region is live; the parser's borrow registers and the CFG regions disagree",
                            function.id, block.id, written
                        ));
                    }
                }
                // The L1 rule computed here: no write or move while a view
                // of the slot may still be read later on some path — for a
                // view whose introduction dominates the statement. A view
                // created after the statement looks at the new bytes.
                let mut live_at_write = suffix[index + 1].clone();
                live_at_write.extend(&live_out);
                let covering: Vec<&(usize, usize)> = view_borrows.iter()
                    .filter(|&&(view, base)| targets.contains(&base)
                        && view_covers_write(view, block.id, index, &introductions, &dom, &live_at_write))
                    .collect();
                if let Some(&(view, _)) = covering.first() {
                    return Err(format!(
                        "internal: fn #{} block {} touches v{} while its view v{} may still be read; the parser's move rules and the NLL regions disagree",
                        function.id, block.id,
                        view_borrows.iter().find(|&&(v, _)| v == *view).map(|&(_, b)| b).unwrap_or(0),
                        view
                    ));
                }
            }
        }
        Ok(())
    }
}

// Whole-statement borrows: per-block live-on-entry sets, to a fixed point
// with union meets. Only `for` containers, token-loop bases, and match
// subjects take part — the borrows whose extent the tree's own shape fixes.
struct ScopeRegions {
    live_in: Vec<std::collections::HashSet<usize>>,
}

impl ScopeRegions {
    fn compute(function: &CfgFunction) -> ScopeRegions {
        let mut live_in: Vec<std::collections::HashSet<usize>> = vec![std::collections::HashSet::new(); function.blocks.len()];
        let mut queue: std::collections::VecDeque<usize> = std::collections::VecDeque::new();
        let mut in_queue = vec![false; function.blocks.len()];
        queue.push_back(0);
        in_queue[0] = true;
        while let Some(block_id) = queue.pop_front() {
            in_queue[block_id] = false;
            let mut state = live_in[block_id].clone();
            for statement in &function.blocks[block_id].statements {
                introduce_borrows(statement, &mut state);
            }
            // The terminator carries no introduction of its own: a return's
            // expression may read a borrowed base, but the region already
            // covers the block, and this walk is over introductions only.
            let exit_state = state;
            if exit_state != live_in[block_id] {
                live_in[block_id] = exit_state.clone();
                for succ in successors(function, block_id) {
                    let merged = &mut live_in[succ];
                    let before = merged.len();
                    merged.extend(exit_state.iter());
                    if merged.len() != before && !in_queue[succ] {
                        queue.push_back(succ);
                        in_queue[succ] = true;
                    }
                }
            }
        }
        ScopeRegions { live_in }
    }
}

// NLL regions for view borrows: the blocks where a view slot may still be
// read. Views are declare-only, so backwards liveness has no kills — a view
// is live on entry to a block if the block reads it or some successor does.
struct NllRegions {
    // Block id -> view slots live on ENTRY (read later on some path).
    live_in: HashMap<usize, std::collections::HashSet<usize>>,
}

impl NllRegions {
    fn compute(function: &CfgFunction, view_borrows: &[(usize, usize)]) -> NllRegions {
        let views: std::collections::HashSet<usize> = view_borrows.iter().map(|&(view, _)| view).collect();
        let mut live_in: Vec<std::collections::HashSet<usize>> = vec![std::collections::HashSet::new(); function.blocks.len()];
        let mut changed = true;
        let mut rounds = function.blocks.len() + 1;
        while changed && rounds > 0 {
            rounds -= 1;
            changed = false;
            // Reverse order converges faster along forward edges; the
            // rounds bound guards against a lowering bug, not the limit.
            for block in function.blocks.iter().rev() {
                let mut state = std::collections::HashSet::new();
                for statement in &block.statements {
                    state.extend(statement_reads(statement).into_iter().filter(|id| views.contains(id)));
                }
                match &block.terminator {
                    CfgTerminator::Return(expr) | CfgTerminator::Exit(expr) => state.extend(expr_reads(expr).into_iter().filter(|id| views.contains(id))),
                    CfgTerminator::Branch(cond, ..) => state.extend(expr_reads(cond).into_iter().filter(|id| views.contains(id))),
                    _ => {}
                }
                for succ in successors(function, block.id) {
                    state.extend(live_in[succ].iter());
                }
                if state != live_in[block.id] {
                    live_in[block.id] = state;
                    changed = true;
                }
            }
        }
        let by_id = function.blocks.iter().map(|block| (block.id, live_in[block.id].clone())).collect();
        NllRegions { live_in: by_id }
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

// The dominators of every block: block b's dominator set is {b} plus the
// blocks every path from the entry passes through. The iterative algorithm
// over the CFG's small block counts is plenty. Unreachable blocks keep the
// full set — nothing dominates them meaningfully and nothing reaches them.
fn dominators(function: &CfgFunction) -> Vec<std::collections::HashSet<usize>> {
    let count = function.blocks.len();
    let mut preds = vec![Vec::new(); count];
    for block in function.blocks.iter() {
        for succ in successors(function, block.id) {
            if succ < count { preds[succ].push(block.id); }
        }
    }
    let full: std::collections::HashSet<usize> = (0..count).collect();
    let mut dom: Vec<std::collections::HashSet<usize>> = vec![full.clone(); count];
    if count > 0 {
        dom[0] = std::collections::HashSet::from([0]);
        let mut changed = true;
        while changed {
            changed = false;
            for id in 1..count {
                let reachable_preds: Vec<usize> = preds[id].iter().copied()
                    .filter(|pred| *pred == 0 || dom[*pred].contains(&0))
                    .collect();
                let Some(first) = reachable_preds.first() else { continue };
                let mut merged = dom[*first].clone();
                for pred in reachable_preds.iter().skip(1) {
                    merged = merged.intersection(&dom[*pred]).copied().collect();
                }
                merged.insert(id);
                if merged != dom[id] {
                    dom[id] = merged;
                    changed = true;
                }
            }
        }
    }
    dom
}

// A persistent borrow's base variable: loops borrow containers, token loops
// borrow their base, match borrows its subject. View borrows take the NLL
// path above, not the scope regions.
fn introduce_borrows(statement: &Statement, live: &mut std::collections::HashSet<usize>) {
    match statement {
        Statement::For(_, container, ..) => { live.insert(*container); }
        Statement::ForTokens(_, base, ..) => { live.insert(*base); }
        Statement::Match(subject, ..) => { live.insert(*subject); }
        // A payload binding (M2.3) borrows from the subject: views and
        // borrows read the enum, so its region covers the arm's block.
        Statement::BindPayload(_, subject, ..) => { live.insert(*subject); }
        _ => {}
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

#[cfg(test)]
mod tests {
    use super::*;
    use super::cfg::{CfgBlock, CfgFunction, CfgTerminator};

    fn function_of(blocks: Vec<CfgBlock>) -> CfgFunction {
        CfgFunction { id: 0, is_main: true, params: Vec::new(), returns: Type::Int, blocks }
    }

    fn block(id: usize, statements: Vec<Statement>, terminator: CfgTerminator) -> CfgBlock {
        CfgBlock { id, statements, terminator }
    }

    // A one-block function: the view is declared, the base is written while
    // the view is still readable, and the view is read afterwards.
    #[test]
    fn a_write_inside_a_view_lifetime_is_a_conflict() {
        // let v = slice_view(s, 0, 3);
        // s = "other";        <- conflict
        // print(v);
        let statements = vec![
            Statement::Declare(10, Expr::SliceView(5, None, Box::new(Expr::Number(0)), Box::new(Expr::Number(3)), 1, 1)),
            Statement::Assign(5, Expr::StrLit("other".to_owned(), 2, 8), true),
            Statement::Print(Printed::Expr(Expr::Variable(10, Type::StrView, 3, 6))),
        ];
        let function = function_of(vec![block(0, statements, CfgTerminator::Unreachable)]);
        let regions = NllRegions::compute(&function, &[(10, 5)]);
        assert!(regions.live_in[&0].contains(&10), "the view is live on entry: {:?}", regions.live_in);
        let error = BorrowRegions::run(&function).unwrap_err();
        assert!(error.contains("may still be read"), "the NLL check fires: {error}");
    }

    // The same shape with the write after the view's last use: the region
    // dies at the read, so the write is outside it (L1's allowance).
    #[test]
    fn a_write_after_the_last_view_use_is_outside_the_region() {
        // let v = slice_view(s, 0, 3);
        // print(v);
        // s = "other";        <- allowed once enforcement moves onto the graph
        let statements = vec![
            Statement::Declare(10, Expr::SliceView(5, None, Box::new(Expr::Number(0)), Box::new(Expr::Number(3)), 1, 1)),
            Statement::Print(Printed::Expr(Expr::Variable(10, Type::StrView, 2, 6))),
            Statement::Assign(5, Expr::StrLit("other".to_owned(), 3, 8), true),
        ];
        let function = function_of(vec![block(0, statements, CfgTerminator::Unreachable)]);
        let regions = NllRegions::compute(&function, &[(10, 5)]);
        // The view's last read is the print, inside this block, so the
        // entry liveness still includes it — but nothing reads the view
        // after the write, which is what the check asks.
        assert!(regions.live_in[&0].contains(&10), "the view is live on entry: {:?}", regions.live_in);
        assert!(BorrowRegions::run(&function).is_ok(), "no conflict after the last use");
    }

    // Scope regions keep their union meet: a for's container is borrowed for
    // the whole loop, on every path, whatever the view rules say.
    #[test]
    fn a_loop_container_stays_borrowed_across_its_scope() {
        // for t in cur { push(words, 1); }  <- words is the loop's container
        let statements = vec![
            Statement::For(20, 9, Vec::new(), 2, 1, 1),
        ];
        let function = function_of(vec![block(0, statements, CfgTerminator::Unreachable)]);
        let regions = ScopeRegions::compute(&function);
        assert!(regions.live_in[0].contains(&9), "the container is live on entry: {:?}", regions.live_in);
    }
}
