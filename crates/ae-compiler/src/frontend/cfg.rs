//! The control-flow layer: each function becomes a graph of blocks whose
//! contents are the statement tree's own statements, verbatim (M2.2 slice 2).
//!
//! Only control flow is lowered: `if`/`while`/`for`/`match`/`return`/`exit`
//! shape the graph, `break`/`continue` become explicit edges, and every other
//! statement — assignments, calls, pushes, releases — is kept whole inside
//! its block. This is the flow-sensitive base the borrow checker needs: move
//! and borrow states are computed per block along these edges. Expressions
//! stay unsplit; `&&`/`||` keep their source form as whole conditions, and
//! `?`/`assert` keep their whole-statement form because their branch lives
//! inside the operation.
//!
//! Printed by `ae emit-cfg`; the validator checks that every reachable
//! block ends in exactly one terminator and that all branch targets exist.
//!
//! Deadness is tracked per statement list: a list whose every path ends
//! unconditionally (return/exit/break/continue) reports that upward, so the
//! statements after it — the tree's function-tail releases a return edge
//! already carries — are dropped instead of being placed in a block that
//! never terminates. Control statements with a continuation (if/while/for/
//! match) report "alive" through their join as long as some arm continues.
use super::*;

pub(super) fn cfg_path(path: &Path) -> Result<String, String> {
    let program = loader::load_program(path)?;
    let mut out = String::new();
    for file in &program.files {
        for function in file {
            let graph = build_cfg(function);
            validate(&graph);
            print_function(&graph, &mut out);
        }
    }
    Ok(out)
}

#[derive(Debug)]
pub(super) struct CfgFunction {
    pub(super) id: usize,
    pub(super) is_main: bool,
    pub(super) params: Vec<(usize, Type)>,
    pub(super) returns: Type,
    pub(super) blocks: Vec<CfgBlock>,
}

#[derive(Debug)]
pub(super) struct CfgBlock {
    pub(super) id: usize,
    pub(super) statements: Vec<Statement>,
    pub(super) terminator: CfgTerminator,
}

#[derive(Debug, Clone)]
pub(super) enum CfgTerminator {
    // Unconditional edge to another block.
    Goto(usize),
    // Two-way branch; the condition is the source expression, unsplit.
    Branch(Expr, usize, usize),
    // Enum dispatch: the subject variable id, its layout id, one target per
    // variant of the matched enum in variant order.
    Switch(usize, usize, Vec<usize>),
    Return(Expr),
    Exit(Expr),
    // A block that no path reaches: the lowering's pending tail, or the
    // continuation every path of a fully-returning list abandoned.
    Unreachable,
}

#[derive(Default)]
struct Lowering {
    blocks: Vec<CfgBlock>,
    next_block: usize,
    // Loop stacks: break and continue target the exit and the head.
    loop_exits: Vec<usize>,
    loop_heads: Vec<usize>,
}

impl Lowering {
    fn new_block(&mut self) -> usize {
        let id = self.next_block;
        self.next_block += 1;
        self.blocks.push(CfgBlock { id, statements: Vec::new(), terminator: CfgTerminator::Unreachable });
        id
    }
    fn block(&mut self, id: usize) -> &mut CfgBlock {
        &mut self.blocks[id]
    }
}

pub(super) fn build_cfg(function: &Function) -> CfgFunction {
    let mut lowering = Lowering::default();
    let entry = lowering.new_block();
    lower_statements(&function.body, entry, &mut lowering);
    CfgFunction { id: function.id, is_main: function.is_main, params: function.params.clone(), returns: function.returns, blocks: lowering.blocks }
}

// Lower a statement list into `entry`. The bool answers "did every path
// through this list end unconditionally (return/exit/break/continue)?" —
// when true, the block the list nominally continues in is unreachable, and
// statements after the list (the tree's function-tail releases) are dead.
// Statements *within* the list after such an end are dropped by the loop.
fn lower_statements(statements: &[Statement], entry: usize, lowering: &mut Lowering) -> (usize, bool) {
    let mut current = entry;
    let mut ended = false;
    for statement in statements {
        if ended { break; }
        let (next, list_ended) = lower_statement(statement, current, lowering);
        current = next;
        ended = list_ended;
    }
    (current, ended)
}

// Lower one statement; returns the continuation block and whether every
// path through it ended unconditionally.
fn lower_statement(statement: &Statement, current: usize, lowering: &mut Lowering) -> (usize, bool) {
    match statement {
        Statement::If(cond, yes, no) => {
            let then_block = lowering.new_block();
            let else_block = lowering.new_block();
            lowering.block(current).terminator = CfgTerminator::Branch((*cond).clone(), then_block, else_block);
            let (after_then, ended_then) = lower_statements(yes, then_block, lowering);
            let (after_else, ended_else) = lower_statements(no, else_block, lowering);
            let joined = join(after_then, after_else, lowering);
            // The join is a continuation only when some arm continues.
            (joined, ended_then && ended_else)
        }
        Statement::While(cond, body) => {
            // The head is `current`: the condition lives in its terminator,
            // the body loops back to it, and the exit continues on.
            let body_block = lowering.new_block();
            let exit = lowering.new_block();
            lowering.block(current).terminator = CfgTerminator::Branch((*cond).clone(), body_block, exit);
            lowering.loop_exits.push(exit);
            lowering.loop_heads.push(current);
            let (after_body, _) = lower_statements(body, body_block, lowering);
            lowering.loop_exits.pop();
            lowering.loop_heads.pop();
            redirect(after_body, current, lowering);
            // The exit is a continuation whatever the body did.
            (exit, false)
        }
        Statement::For(var, container, body, ..) => {
            // The for-in loop borrows its container for the whole body and
            // snapshots the length once, which the loop head covers.
            let head = lowering.new_block();
            lowering.block(current).terminator = CfgTerminator::Goto(head);
            let body_block = lowering.new_block();
            let exit = lowering.new_block();
            // The head re-checks the snapshot counter each iteration.
            let counter = Expr::Variable(*var, Type::Int, 0, 0);
            lowering.block(head).terminator = CfgTerminator::Branch(counter, body_block, exit);
            let _ = container;
            lowering.loop_exits.push(exit);
            lowering.loop_heads.push(head);
            let (after_body, _) = lower_statements(body, body_block, lowering);
            lowering.loop_exits.pop();
            lowering.loop_heads.pop();
            redirect(after_body, head, lowering);
            (exit, false)
        }
        Statement::ForTokens(var, base, body, ..) => {
            let head = lowering.new_block();
            lowering.block(current).terminator = CfgTerminator::Goto(head);
            let body_block = lowering.new_block();
            let exit = lowering.new_block();
            let counter = Expr::Variable(*var, Type::Int, 0, 0);
            let _ = base;
            lowering.block(head).terminator = CfgTerminator::Branch(counter, body_block, exit);
            lowering.loop_exits.push(exit);
            lowering.loop_heads.push(head);
            let (after_body, _) = lower_statements(body, body_block, lowering);
            lowering.loop_exits.pop();
            lowering.loop_heads.pop();
            redirect(after_body, head, lowering);
            (exit, false)
        }
        Statement::Match(subject, ty, arms) => {
            let Type::Enum(_, eid) = *ty else {
                // The statement tree guarantees an enum subject; the arm is a
                // defensive default that the validator would flag.
                return (current, false);
            };
            let mut targets = Vec::new();
            for _ in arms { targets.push(lowering.new_block()); }
            lowering.block(current).terminator = CfgTerminator::Switch(*subject, eid, targets.clone());
            let mut joins = Vec::new();
            let mut all_ended = true;
            for (arm, block) in arms.iter().zip(targets.iter()) {
                // Payload bindings stay annotations of the switch edge in this
                // slice: the tree binds them as reads, and the MIR
                // construction turns them into explicit assignments.
                let _ = &arm.bindings;
                let (after, ended) = lower_statements(&arm.body, *block, lowering);
                joins.push((after, ended));
                all_ended = all_ended && ended;
            }
            join_all(&joins, lowering);
            // The match's continuation is alive only when some arm continues.
            let continuation = alive_target(&joins, lowering);
            (continuation, all_ended)
        }
        Statement::Return(expr, frees) => {
            // The release annotations ride the return: they are statements
            // the lowering keeps in order, then the return edge.
            if !frees.is_empty() {
                lowering.block(current).statements.push(Statement::Frees(frees.clone()));
            }
            lowering.block(current).terminator = CfgTerminator::Return((*expr).clone());
            (lowering.new_block(), true)
        }
        Statement::Exit(code, frees) => {
            if !frees.is_empty() {
                lowering.block(current).statements.push(Statement::Frees(frees.clone()));
            }
            lowering.block(current).terminator = CfgTerminator::Exit((*code).clone());
            (lowering.new_block(), true)
        }
        Statement::Break => {
            let Some(exit) = lowering.loop_exits.last().copied() else {
                // The parser rejects break outside loops; the arm is a guard.
                lowering.block(current).terminator = CfgTerminator::Unreachable;
                return (lowering.new_block(), true);
            };
            lowering.block(current).terminator = CfgTerminator::Goto(exit);
            (lowering.new_block(), true)
        }
        Statement::Continue(depth) => {
            let index = match depth { Some(d) if *d > 0 && *d <= lowering.loop_heads.len() => lowering.loop_heads.len() - *d, _ => lowering.loop_heads.len().saturating_sub(1) };
            let Some(head) = lowering.loop_heads.get(index).copied() else {
                lowering.block(current).terminator = CfgTerminator::Unreachable;
                return (lowering.new_block(), true);
            };
            lowering.block(current).terminator = CfgTerminator::Goto(head);
            (lowering.new_block(), true)
        }
        // `?` propagation and `assert` keep their whole statement form: their
        // control flow is decided by a result tag inside the operation, which
        // this slice does not split out. The MIR construction splits them.
        // Every other non-control statement stays whole in the current block.
        other => {
            lowering.block(current).statements.push(other.clone());
            (current, false)
        }
    }
}

// Redirect two paths into one continuation block; identical paths stay one.
fn join(a: usize, b: usize, lowering: &mut Lowering) -> usize {
    if a == b { return a; }
    let join = lowering.new_block();
    redirect(a, join, lowering);
    redirect(b, join, lowering);
    join
}

// Merge a set of path continuations into one block.
fn join_all(joins: &[(usize, bool)], lowering: &mut Lowering) -> usize {
    let mut blocks: Vec<usize> = joins.iter().map(|(block, _)| *block).collect();
    let Some(first) = blocks.first().copied() else { return lowering.new_block(); };
    let mut joined = first;
    for next in blocks.drain(1..) {
        joined = join(joined, next, lowering);
    }
    joined
}

// The block the flow continues in after a set of paths: the first path that
// did not end unconditionally, or — when every path ended — the merged tail,
// which is then unreachable by construction.
fn alive_target(joins: &[(usize, bool)], lowering: &mut Lowering) -> usize {
    for (block, ended) in joins {
        if !ended { return *block; }
    }
    join_all(joins, lowering)
}

// Point a block's fall-through at `target`; terminated blocks keep theirs.
fn redirect(from: usize, target: usize, lowering: &mut Lowering) {
    if let CfgTerminator::Unreachable = lowering.block(from).terminator {
        lowering.block(from).terminator = CfgTerminator::Goto(target);
    }
}

// The graph's invariants: one terminator per block and targets that exist.
// A violation is a lowering bug, not a user error -- the message says
// `internal`. A block with statements and no terminator is legal only in
// `main`, whose implicit exit-0 the tree does not spell out.
fn validate(function: &CfgFunction) {
    let count = function.blocks.len();
    for block in &function.blocks {
        match &block.terminator {
            CfgTerminator::Unreachable => {
                if !block.statements.is_empty() && !function.is_main {
                    if std::env::var_os("AE_DEBUG_CFG").is_some() {
                        eprintln!("cfg debug: fn #{} block {} holds {} statements:", function.id, block.id, block.statements.len());
                        for other in &function.blocks {
                            let shape = match &other.terminator {
                                CfgTerminator::Goto(t) => format!("goto {}", t),
                                CfgTerminator::Branch(_, a, b) => format!("branch {} {}", a, b),
                                CfgTerminator::Switch(_, _, ts) => format!("switch {:?}", ts),
                                CfgTerminator::Return(_) => "return".to_owned(),
                                CfgTerminator::Exit(_) => "exit".to_owned(),
                                CfgTerminator::Unreachable => "unreachable".to_owned(),
                            };
                            eprintln!("  block {} -> {} ({} statements)", other.id, shape, other.statements.len());
                        }
                    }
                    panic!("internal: block {} of fn #{} holds statements but never terminates", block.id, function.id);
                }
            }
            CfgTerminator::Goto(target) => assert!(*target < count, "internal: fn #{} block {} branches to missing block {}", function.id, block.id, target),
            CfgTerminator::Branch(_, a, b) => assert!(*a < count && *b < count, "internal: fn #{} block {} branches to missing block", function.id, block.id),
            CfgTerminator::Switch(_, _, targets) => for t in targets { assert!(*t < count, "internal: fn #{} switch target missing", function.id); },
            CfgTerminator::Return(_) | CfgTerminator::Exit(_) => {}
        }
    }
}

fn print_function(function: &CfgFunction, out: &mut String) {
    let name = if function.is_main { "main".to_owned() } else { format!("#{}", function.id) };
    let params = function.params.iter().map(|(id, t)| format!("v{}: {}", id, t.name())).collect::<Vec<_>>().join(", ");
    out.push_str(&format!("fn {}({}) -> {} ({} blocks)\n", name, params, function.returns.name(), function.blocks.len()));
    for block in &function.blocks {
        out.push_str(&format!("  block {}:\n", block.id));
        for statement in &block.statements {
            out.push_str("    ");
            out.push_str(&crate::frontend::tree::statement_text(statement));
            out.push('\n');
        }
        let tail = match &block.terminator {
            CfgTerminator::Goto(target) => format!("goto block {}", target),
            CfgTerminator::Branch(cond, a, b) => format!("branch on {} -> then block {} else block {}", crate::frontend::tree::expression_text(cond), a, b),
            CfgTerminator::Switch(subject, _, targets) => format!("switch v{} -> {}", subject, targets.iter().map(|t| format!("block {}", t)).collect::<Vec<_>>().join(", ")),
            CfgTerminator::Return(expr) => format!("return {}", crate::frontend::tree::expression_text(expr)),
            CfgTerminator::Exit(code) => format!("exit {}", crate::frontend::tree::expression_text(code)),
            // Only main can end this way: the implicit exit-0.
            CfgTerminator::Unreachable if function.is_main && !block.statements.is_empty() => "implicit exit (main)".to_owned(),
            CfgTerminator::Unreachable => "unreachable".to_owned(),
        };
        out.push_str(&format!("    {}\n", tail));
    }
}
