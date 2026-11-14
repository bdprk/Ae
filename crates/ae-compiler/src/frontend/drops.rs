//! Drop elaboration over the MIR block graph (M2.4).
//!
//! The parser attaches a release list to every point where storage leaves
//! scope: the end of a block, `break`/`continue`, and the releases a `return`
//! or `exit` carries. Those lists are written before the graph exists, and
//! they are conservative in both directions:
//!
//! - they list a slot a statement has already **moved out** of (the move
//!   zeroed the slot, so the release frees nothing but still branches and
//!   stores), and
//! - they list a slot a path can reach **before it was ever written** (a
//!   release of an uninitialized slot would free a garbage pointer).
//!
//! This pass turns each list into what the graph says it must be. It computes,
//! per block and per owned local, whether the slot holds a value that still
//! needs releasing on **every** incoming path (`Live`), on **none** of them
//! (`Uninit` for a slot no path wrote, `Dropped` for a slot every path already
//! moved or released), or on **some** of them (`Mixed`). A `Live` entry stays,
//! `Uninit`/`Dropped` entries are removed, and a `Mixed` entry would need a
//! runtime drop flag — a shape the language cannot currently produce, because
//! every owned binding is declared inside the block whose release list names
//! it, so the pass reports one as an internal error rather than silently
//! depending on release idempotence.
//!
//! The meet is exact: `Live ⊔ Live = Live`, `Uninit ⊔ Uninit = Uninit`,
//! `Dropped ⊔ Dropped = Dropped`, and any other pair joins to `Mixed`. Move
//! events come from `movewalk::statement_move_effects`, the same reading of
//! "what consumes a binding" the move verifier uses, so the two analyses
//! cannot disagree about what a statement moves.
use std::collections::HashMap;

use super::*;
use super::cfg::CfgTerminator;
use super::mir::MirFunction;
use super::movewalk::statement_move_effects;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum DropState {
    // No path wrote this slot: releasing it would free a garbage pointer.
    Uninit,
    // Every path reaching here holds a value that still needs releasing.
    Live,
    // Every path reaching here already moved or released the value.
    Dropped,
    // Some paths hold a value, others do not: a runtime drop flag would be
    // needed. The language cannot currently produce this state at a release.
    Mixed,
}

fn join(a: DropState, b: DropState) -> DropState {
    if a == b { a } else { DropState::Mixed }
}

type State = HashMap<usize, DropState>;

// What the pass changed, so the compiler can report it and tests can assert
// that a program's releases were actually reconsidered rather than passed
// through.
#[derive(Debug, Default, PartialEq, Eq)]
pub(super) struct DropStats {
    // Releases kept: a live value on every path.
    pub(super) kept: usize,
    // Releases removed because every path already moved the value.
    pub(super) elided_dropped: usize,
    // Releases removed because no path ever wrote the slot.
    pub(super) elided_uninit: usize,
}

pub(super) fn elaborate(function: &mut MirFunction) -> Result<DropStats, String> {
    let states = entry_states(function);
    let mut stats = DropStats::default();
    for block in &mut function.blocks {
        let mut state: State = states.get(&block.id).cloned().unwrap_or_default();
        if std::env::var_os("AE_DROP_DEBUG").is_some() { eprintln!("DEBUG fn#{} block{} entry={state:?} statements={:?}", function.id, block.id, block.statements.iter().map(|s| format!("{s:?}").chars().take(120).collect::<String>()).collect::<Vec<_>>()); }
        let statements = std::mem::take(&mut block.statements);
        let mut kept = Vec::with_capacity(statements.len());
        for statement in statements {
            if let Some(statement) = rewrite(statement, &mut state, &mut stats, function.id, block.id)? {
                kept.push(statement);
            }
        }
        block.statements = kept;
    }
    Ok(stats)
}

// Per-block state on entry, to a fixed point. `Uninit` is the default for
// every slot a state does not name, so the entry state names only the owned
// parameters (live on entry).
fn entry_states(function: &MirFunction) -> HashMap<usize, State> {
    let Some(entry) = function.blocks.first().map(|block| block.id) else { return HashMap::new() };
    let mut initial = State::new();
    for &(id, ty) in &function.params {
        if ty.owned() { initial.insert(id, DropState::Live); }
    }
    let mut in_states: HashMap<usize, State> = HashMap::new();
    let mut out_states: HashMap<usize, State> = HashMap::new();
    // Each round recomputes every block from its predecessors' current exit
    // states. States only move up the lattice (`Mixed` is the top), so the
    // iteration terminates; the round bound is a guard against a lowering bug
    // introducing a cycle the lattice cannot absorb, not the real limit.
    let rounds = function.blocks.len() * 4 + 8;
    for _ in 0..rounds {
        let mut changed = false;
        for block in &function.blocks {
            let incoming = if block.id == entry { initial.clone() } else { merge_predecessors(function, block.id, &out_states) };
            let mut exit = incoming.clone();
            transfer_statements(&block.statements, &mut exit);
            if in_states.get(&block.id) != Some(&incoming) {
                in_states.insert(block.id, incoming);
                changed = true;
            }
            if out_states.get(&block.id) != Some(&exit) {
                out_states.insert(block.id, exit);
                changed = true;
            }
        }
        if !changed { return in_states; }
    }
    in_states
}

fn merge_predecessors(function: &MirFunction, block_id: usize, out_states: &HashMap<usize, State>) -> State {
    let mut merged: Option<State> = None;
    for block in &function.blocks {
        if !terminator_targets(&block.terminator).contains(&block_id) { continue; }
        let exit = out_states.get(&block.id).cloned().unwrap_or_default();
        merged = Some(match merged {
            None => exit,
            Some(joined) => join_states(&joined, &exit),
        });
    }
    merged.unwrap_or_default()
}

fn join_states(a: &State, b: &State) -> State {
    let mut joined = a.clone();
    for (&id, &right) in b {
        let left = joined.get(&id).copied().unwrap_or(DropState::Uninit);
        joined.insert(id, join(left, right));
    }
    joined
}

fn terminator_targets(terminator: &CfgTerminator) -> Vec<usize> {
    match terminator {
        CfgTerminator::Goto(target) => vec![*target],
        CfgTerminator::Branch(_, yes, no) => vec![*yes, *no],
        CfgTerminator::Switch(_, _, targets) => targets.clone(),
        CfgTerminator::Return(_) | CfgTerminator::Exit(_) | CfgTerminator::Unreachable => Vec::new(),
    }
}

// The state a statement leaves behind, without rewriting it. Releases drop
// every listed slot; the moves and re-initializations come from the shared
// move walk, applied in the order a statement performs them.
//
// Terminators need no transfer of their own: `return`/`exit` end the block, so
// the only paths that read a post-terminator state are `goto`/`branch`/`switch`
// edges, and their conditions are emitted without moving anything.
fn transfer_statements(statements: &[Statement], state: &mut State) {
    for statement in statements {
        transfer_statement(statement, state);
    }
}

fn transfer_statement(statement: &Statement, state: &mut State) {
    match statement {
        Statement::Frees(list) => drop_list(list, state),
        // A return/exit carries its releases on the statement in the tree; the
        // CFG lowering moves them into the block, so this arm covers the tree
        // shape only.
        Statement::Return(_, list) | Statement::Exit(_, list) => drop_list(list, state),
        // Control statements never survive into a MIR block (the lowering
        // turns them into blocks and edges). The arms stay total: a nested
        // list is joined into the state the way its own blocks would be.
        Statement::If(_, yes, no) => {
            let mut then_state = state.clone();
            transfer_statements(yes, &mut then_state);
            let mut else_state = state.clone();
            transfer_statements(no, &mut else_state);
            *state = join_states(&then_state, &else_state);
        }
        Statement::While(_, body) | Statement::For(_, _, body, ..) | Statement::ForTokens(_, _, body, ..) => {
            let mut body_state = state.clone();
            transfer_statements(body, &mut body_state);
            let entry_state = state.clone();
            *state = join_states(&entry_state, &body_state);
        }
        Statement::Match(_, _, arms) => {
            let mut merged = state.clone();
            for arm in arms {
                let mut arm_state = state.clone();
                transfer_statements(&arm.body, &mut arm_state);
                merged = join_states(&merged, &arm_state);
            }
            *state = merged;
        }
        other => apply_effects(other, state),
    }
}

// A statement's own moves and re-initializations. The right side evaluates
// before the slot is written, so the move events apply first.
fn apply_effects(statement: &Statement, state: &mut State) {
    let effects = statement_move_effects(statement);
    for id in effects.moved {
        state.insert(id, DropState::Dropped);
    }
    for id in effects.revived {
        state.insert(id, DropState::Live);
    }
}

fn drop_list(list: &[(usize, Type)], state: &mut State) {
    for &(id, _) in list {
        state.insert(id, DropState::Dropped);
    }
}

// Rewrite one statement's release lists. `Ok(None)` means the statement does
// nothing any more and is removed, so a block can become empty and eligible
// for edge threading again.
fn rewrite(statement: Statement, state: &mut State, stats: &mut DropStats, function: usize, block: usize) -> Result<Option<Statement>, String> {
    match statement {
        Statement::Frees(list) => {
            let kept = rewrite_list(list, state, stats, function, block, "release")?;
            if kept.is_empty() { return Ok(None); }
            Ok(Some(Statement::Frees(kept)))
        }
        Statement::Return(expr, list) => Ok(Some(Statement::Return(expr, rewrite_list(list, state, stats, function, block, "return")?))),
        Statement::Exit(code, list) => Ok(Some(Statement::Exit(code, rewrite_list(list, state, stats, function, block, "exit")?))),
        other => {
            apply_effects(&other, state);
            Ok(Some(other))
        }
    }
}

fn rewrite_list(list: Vec<(usize, Type)>, state: &mut State, stats: &mut DropStats, function: usize, block: usize, where_: &str) -> Result<Vec<(usize, Type)>, String> {
    let mut kept = Vec::with_capacity(list.len());
    for (id, ty) in list {
        match state.get(&id).copied().unwrap_or(DropState::Uninit) {
            DropState::Live => {
                stats.kept += 1;
                state.insert(id, DropState::Dropped);
                kept.push((id, ty));
            }
            DropState::Dropped => stats.elided_dropped += 1,
            DropState::Uninit => {
                if std::env::var_os("AE_DROP_DEBUG").is_some() { eprintln!("DEBUG elide-uninit v{id} in fn#{function} block{block} state={state:?} statements-seen"); }
                stats.elided_uninit += 1
            }
            // Releasing here would be required on some paths and a double
            // release on others: the compile-time answer is a drop flag, which
            // no accepted program can need (every owned binding is declared in
            // the block whose release list names it). Refuse rather than emit
            // a release that depends on the slot's zeroed state.
            DropState::Mixed => {
                return Err(format!(
                    "internal: fn #{} block {} has a mixed drop state at the {where_} of v{}; drop flags are not implemented, and no accepted program should reach this",
                    function, block, id
                ));
            }
        }
    }
    Ok(kept)
}