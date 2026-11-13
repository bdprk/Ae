//! The C backend over the MIR block graph (M2.3).
//!
//! The structured statement tree is emitted as `if`/`while` blocks by
//! `codegen`; this module emits the same program as the MIR's basic blocks:
//! each block becomes a C label, statements stay in their original order, and
//! the terminator becomes an explicit `goto`, `if (...) goto ...; else ...`,
//! `return`, or `ae_exit`. Nothing about ownership changes: the
//! statements carry the same releases, moves, and runtime backstops the tree
//! emitter consumes, so the runtime behavior is identical by construction.
//!
//! Two lowering slices are not yet complete enough to drive codegen, and a
//! function that contains one stays on the tree backend:
//!
//! - `for` / `for ... in tokens(...)`: the tree statement carries the hidden
//!   counter increment and the `continue` landing label; the CFG slice
//!   branches on the counter without them.
//! - `match`: the arms' payload bindings are annotations of the switch edge,
//!   not statements, so the arm bodies would read undeclared slots.
//!
//! Everything else — `if`, `while`, `break`, `continue`, early `return`,
//! `exit`, `?` propagation, `assert`, containers, strings, structs, enums,
//! closures — keeps its whole statement form in the block and is emitted by
//! the same statement emitter the tree backend uses.
use super::*;
use super::cfg::CfgTerminator;
use super::mir::{self, MirBlock, MirFunction};

// The MIR path is the default; `AE_CODEGEN=tree` forces the structured
// tree emitter for the whole program, which is how the differential tests
// compare the two backends and how a regression can be rolled back without a
// rebuild of the decision logic.
pub(super) fn enabled() -> bool {
    !matches!(std::env::var("AE_CODEGEN").as_deref(), Ok("tree"))
}

// A function may be emitted from MIR when no statement (at any depth) depends
// on the lowering slices listed above.
pub(super) fn supported(function: &Function) -> bool {
    statements_supported(&function.body)
}

fn statements_supported(statements: &[Statement]) -> bool {
    statements.iter().all(statement_supported)
}

fn statement_supported(statement: &Statement) -> bool {
    match statement {
        Statement::For(..) | Statement::ForTokens(..) | Statement::Match(..) => false,
        Statement::If(_, yes, no) => statements_supported(yes) && statements_supported(no),
        Statement::While(_, body) => statements_supported(body),
        _ => true,
    }
}

// Emit one function body as labelled blocks. The caller owns the signature and
// the trailing backstops; this writes everything between them.
pub(super) fn emit_function_body(function: &Function, out: &mut String) {
    let mut mir = mir::lower_function(function);
    // The same optimization pipeline the printer uses: constant folding and
    // CFG simplification decide which blocks are live. A pruned block is
    // unreachable, so not emitting it changes nothing.
    mir::optimize_function(&mut mir);
    let mut next = 0usize;
    for block in &mir.blocks {
        out.push_str(&format!("  block_{}: ;\n", block.id));
        emit_block(&mir, block, out, &mut next);
    }
}

fn emit_block(function: &MirFunction, block: &MirBlock, out: &mut String, next: &mut usize) {
    let mut statements: &[Statement] = &block.statements;
    // A `return`/`exit` carries the scope's releases as a trailing `Frees`
    // statement (the CFG lowering puts them in the block, before the
    // terminator). They must run after the returned expression is evaluated —
    // the return may move a value out of one of those slots — so the terminator
    // emits them exactly where the tree backend does.
    let mut frees: &[(usize, Type)] = &[];
    if matches!(block.terminator, CfgTerminator::Return(_) | CfgTerminator::Exit(_)) {
        if let Some(Statement::Frees(ids)) = statements.last() {
            frees = ids;
            statements = &statements[..statements.len() - 1];
        }
    }
    emit_statements(statements, out, next);
    match &block.terminator {
        CfgTerminator::Goto(target) => out.push_str(&format!("    goto block_{target};\n")),
        CfgTerminator::Branch(condition, yes, no) => {
            // The condition keeps its source form: emitting it once, here,
            // preserves short-circuit evaluation and any call it contains.
            let cond = emit_expr(condition, out, next, false);
            out.push_str(&format!("    if ({}) goto block_{yes}; else goto block_{no};\n", cond.text));
        }
        CfgTerminator::Return(expr) => {
            let value = emit_expr(expr, out, next, true);
            for &(id, ty) in frees {
                if value.moves.iter().any(|&(moved, _)| moved == id) { continue; }
                release_var(out, id, ty);
            }
            null_moves(out, &value.moves);
            out.push_str(&format!("    return {};\n", value.text));
        }
        CfgTerminator::Exit(code) => {
            let value = emit_expr(code, out, next, false);
            for &(id, ty) in frees { release_var(out, id, ty); }
            out.push_str(&format!("    ae_exit({});\n", value.text));
        }
        // `supported` rejects every `match`, so a switch never reaches here.
        CfgTerminator::Switch(..) => unreachable!("a function with a match stays on the tree backend"),
        // `main` falls through to the caller's flush-and-return; any other
        // function reaching its tail failed to return a value.
        CfgTerminator::Unreachable => {
            if !function.is_main {
                out.push_str("    ae_fail(\"function exited without return\", 0, 0);\n");
            }
        }
    }
}
