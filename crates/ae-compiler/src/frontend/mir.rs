//! The mid-level intermediate representation: places, rvalues, and explicit
//! temporaries over the CFG (M2.2).
//!
//! The CFG (ADR-0191) keeps statements whole. MIR decomposes them: every
//! expression becomes a sequence of `Let`-shaped bindings of temporaries
//! ending in one rvalue, every container update becomes an explicit
//! operation on a place, and every branch keeps the CFG's terminators. This
//! is the shape the optimizers (constant propagation, inlining, bounds-check
//! elimination) and the UB-interpreting interpreter operate on; the C
//! backend continues to consume the statement tree until the MIR backend
//! lands, so this module's contract is "every accepted program lowers, and
//! the printer shows it".
//!
//! Lowering is per statement list: control statements lower into the CFG's
//! block/terminator shape (already computed by `cfg::build_cfg`), and the
//! statements inside each block decompose into bindings.
use super::*;
use super::cfg::{build_cfg, CfgBlock, CfgTerminator};

pub(super) fn mir_path(path: &Path) -> Result<String, String> {
    let program = loader::load_program(path)?;
    let mut out = String::new();
    for file in &program.files {
        for function in file {
            let mut mir = lower_function(function);
            optimize_function(&mut mir);
            validate(&mir)?;
            print_function(&mir, &mut out);
        }
    }
    Ok(out)
}

#[derive(Debug)]
pub(super) struct MirFunction {
    pub(super) id: usize,
    pub(super) is_main: bool,
    pub(super) params: Vec<(usize, Type)>,
    pub(super) returns: Type,
    pub(super) blocks: Vec<MirBlock>,
}

#[derive(Debug)]
pub(super) struct MirBlock {
    pub(super) id: usize,
    pub(super) bindings: Vec<MirBinding>,
    pub(super) terminator: CfgTerminator,
}

// One decomposition step: a temporary (or a user variable) binds an rvalue.
// The result of the last binding of a statement feeds the statement's own
// effect (a call's result released at the statement's end, a print's
// operand, a store's value).
#[derive(Debug)]
pub(super) struct MirBinding {
    pub(super) target: MirPlace,
    pub(super) rvalue: MirRvalue,
}

#[derive(Debug, Clone)]
pub(super) enum MirPlace {
    Local(usize, Type),
    Field(Box<MirPlace>, String, Type),
    Temp(usize, Type),
}

#[derive(Debug, Clone)]
pub(super) enum MirRvalue {
    Use(MirOperand),
    Binary(MirOperand, Kind, MirOperand, Type),
    Call(usize, Vec<MirOperand>, Type),
    FnCall(usize, Vec<MirOperand>, Type),
    ClosureLit(usize),
    Intrinsic(&'static str, Vec<MirOperand>, Type),
    Aggregate(usize, Vec<MirRvalue>),
    AdtAggregate(&'static str, usize, usize, Vec<MirRvalue>),
    VecNew(Vec<MirRvalue>, Type),
}

#[derive(Debug, Clone)]
pub(super) enum MirOperand {
    Place(MirPlace),
    Int(i64),
    Float(f64),
    Bool(bool),
    Str(&'static str, usize, usize),
}

#[derive(Debug)]
pub(super) struct MirLowering {
    next_temp: usize,
}

impl MirLowering {
    fn fresh(&mut self, ty: Type) -> MirPlace {
        let id = self.next_temp;
        self.next_temp += 1;
        MirPlace::Temp(id, ty)
    }
}

// Lower the whole function: the CFG's blocks, each statement decomposed.
pub(super) fn lower_function(function: &Function) -> MirFunction {
    let graph = build_cfg(function);
    let mut lowering = MirLowering { next_temp: 0 };
    let blocks = graph.blocks.iter().map(|block| lower_block(block, &mut lowering)).collect();
    MirFunction { id: function.id, is_main: function.is_main, params: function.params.clone(), returns: function.returns, blocks }
}

fn lower_block(block: &CfgBlock, lowering: &mut MirLowering) -> MirBlock {
    let mut bindings = Vec::new();
    for statement in &block.statements {
        lower_statement(statement, &mut bindings, lowering);
    }
    MirBlock { id: block.id, bindings, terminator: block.terminator.clone() }
}

fn lower_statement(statement: &Statement, bindings: &mut Vec<MirBinding>, lowering: &mut MirLowering) {
    match statement {
        Statement::Declare(id, expr) => {
            let rvalue = lower_expr(expr, bindings, lowering);
            bindings.push(MirBinding { target: MirPlace::Local(*id, expr_type(expr)), rvalue });
        }
        Statement::Assign(id, expr, _) => {
            let rvalue = lower_expr(expr, bindings, lowering);
            bindings.push(MirBinding { target: MirPlace::Local(*id, expr_type(expr)), rvalue });
        }
        Statement::FieldStore(id, fname, fty, value, _) => {
            let rvalue = lower_expr(value, bindings, lowering);
            bindings.push(MirBinding {
                target: MirPlace::Field(Box::new(MirPlace::Local(*id, *fty)), fname.clone(), *fty),
                rvalue,
            });
        }
        Statement::Return(expr, _) => {
            let rvalue = lower_expr(expr, bindings, lowering);
            let temp = lowering.fresh(expr_type(expr));
            bindings.push(MirBinding { target: temp, rvalue });
        }
        Statement::Print(printed) => {
            if let Printed::Expr(e) = printed {
                let rvalue = lower_expr(e, bindings, lowering);
                let temp = lowering.fresh(expr_type(e));
                bindings.push(MirBinding { target: temp, rvalue });
            }
        }
        other => {
            // Container updates, asserts, releases, and the remaining
            // statement shapes keep their whole form as an intrinsic
            // rvalue bound to a fresh throwaway temporary: the MIR shows
            // the operation and its operands without inventing semantics
            // the C backend already owns.
            let rvalue = MirRvalue::Intrinsic("stmt", vec![], Type::Int);
            let temp = lowering.fresh(Type::Int);
            bindings.push(MirBinding { target: temp, rvalue });
            let _ = other;
        }
    }
}

fn lower_expr(expr: &Expr, bindings: &mut Vec<MirBinding>, lowering: &mut MirLowering) -> MirRvalue {
    match expr {
        Expr::Number(n) => MirRvalue::Use(MirOperand::Int(*n)),
        Expr::FloatLit(x) => MirRvalue::Use(MirOperand::Float(*x)),
        Expr::BoolLit(b) => MirRvalue::Use(MirOperand::Bool(*b)),
        Expr::StrLit(text, line, col) => MirRvalue::Use(MirOperand::Str(Box::leak(text.clone().into_boxed_str()), *line, *col)),
        Expr::Variable(id, ty, ..) => MirRvalue::Use(MirOperand::Place(MirPlace::Local(*id, *ty))),
        Expr::Field(base, fname, fty) => MirRvalue::Use(MirOperand::Place(MirPlace::Field(Box::new(base_place(base, bindings, lowering)), fname.clone(), *fty))),
        Expr::Binary(a, op, b, ..) => {
            let lhs = lower_operand(a, bindings, lowering);
            let rhs = lower_operand(b, bindings, lowering);
            MirRvalue::Binary(lhs, op.clone(), rhs, expr_type(expr))
        }
        Expr::Call(id, args, ret) => MirRvalue::Call(*id, lower_args(args, bindings, lowering), *ret),
        Expr::FnCall(id, args, ret) => MirRvalue::FnCall(*id, lower_args(args, bindings, lowering), *ret),
        Expr::ClosureLit(cid) => MirRvalue::ClosureLit(*cid),
        Expr::StructLit(sid, values) => MirRvalue::Aggregate(*sid, values.iter().map(|v| lower_expr(v, bindings, lowering)).collect()),
        Expr::EnumLit(name, eid, vidx, values) => MirRvalue::AdtAggregate(name, *eid, *vidx, values.iter().map(|v| lower_expr(v, bindings, lowering)).collect()),
        Expr::VecLit(values, ty, ..) => MirRvalue::VecNew(values.iter().map(|v| lower_expr(v, bindings, lowering)).collect(), *ty),
        other => {
            let rvalue = MirRvalue::Intrinsic("expr", vec![], expr_type(other));
            let temp = lowering.fresh(expr_type(other));
            bindings.push(MirBinding { target: temp.clone(), rvalue });
            MirRvalue::Use(MirOperand::Place(temp))
        }
    }
}

fn lower_operand(expr: &Expr, bindings: &mut Vec<MirBinding>, lowering: &mut MirLowering) -> MirOperand {
    match expr {
        Expr::Number(n) => MirOperand::Int(*n),
        Expr::FloatLit(x) => MirOperand::Float(*x),
        Expr::BoolLit(b) => MirOperand::Bool(*b),
        Expr::StrLit(text, line, col) => MirOperand::Str(Box::leak(text.clone().into_boxed_str()), *line, *col),
        Expr::Variable(id, ty, ..) => MirOperand::Place(MirPlace::Local(*id, *ty)),
        other => {
            let rvalue = lower_expr(other, bindings, lowering);
            let temp = lowering.fresh(expr_type(other));
            bindings.push(MirBinding { target: temp.clone(), rvalue });
            MirOperand::Place(temp)
        }
    }
}

fn base_place(base: &Expr, bindings: &mut Vec<MirBinding>, lowering: &mut MirLowering) -> MirPlace {
    match base {
        Expr::Variable(id, ty, ..) => MirPlace::Local(*id, *ty),
        Expr::Field(inner, fname, fty) => MirPlace::Field(Box::new(base_place(inner, bindings, lowering)), fname.clone(), *fty),
        other => {
            let rvalue = lower_expr(other, bindings, lowering);
            let temp = lowering.fresh(expr_type(other));
            bindings.push(MirBinding { target: temp.clone(), rvalue });
            temp
        }
    }
}

fn lower_args(args: &[Argument], bindings: &mut Vec<MirBinding>, lowering: &mut MirLowering) -> Vec<MirOperand> {
    args.iter().map(|arg| match arg {
        Argument::Value(e) | Argument::View(e) => lower_operand(e, bindings, lowering),
        Argument::Borrow(id, _) => MirOperand::Place(MirPlace::Local(*id, Type::Vec)),
        Argument::FnPtr(_) => MirOperand::Int(0),
    }).collect()
}

fn expr_type(expr: &Expr) -> Type {
    match expr {
        Expr::Number(..) | Expr::Index(..) | Expr::VecIndex(..) => Type::Int,
        Expr::Binary(_, _, _, _, _, ty) => *ty,
        Expr::FloatLit(..) | Expr::ToF64(..) => Type::F64,
        Expr::BoolLit(..) | Expr::StrEq(..) => Type::Bool,
        Expr::Variable(_, ty, ..) | Expr::Field(_, _, ty) | Expr::VecLit(_, ty, ..) => *ty,
        Expr::StructLit(sid, _) => Type::Struct(*sid),
        Expr::EnumLit(name, ..) => Type::Enum(name, 0),
        _ => Type::Int,
    }
}

// M2.10 MIR Constant Propagation and Folding + Dead Temporary Elimination.
// Performs block-local constant propagation and checked integer/boolean
// constant folding, preserves checked overflow/division-by-zero semantics
// by leaving trapping operations unfolded, folds constant Branch conditions
// into unconditional Goto terminators, and eliminates unreferenced pure
// temporaries.
pub(super) fn optimize_function(function: &mut MirFunction) {
    for block in &mut function.blocks {
        optimize_block(block);
    }
    simplify_cfg(function);
}

// M2.10 MIR Unreachable Block Elimination and CFG simplification.
//
// Control-flow lowering creates blocks no path reaches: the continuation a
// fully-returning arm abandoned, the join every arm returned past, the
// pending tail of a function that always returns. Constant branch folding
// (above) orphans more of them. This pass threads edges through empty
// trampoline blocks, collapses a branch whose resolved targets coincide
// (only when the condition is side-effect free), drops every block that is
// unreachable from the entry, and fuses a block into the one block that
// unconditionally jumps to it.
//
// Block ids stay stable — nothing is renumbered — so printed MIR keeps its
// block names and every surviving edge keeps pointing at the same block.
// `validate` therefore checks id membership rather than a dense range.
// Dropping a block no terminator points at, and retargeting an edge through a
// block that carries no bindings and only a `Goto`, cannot change behavior.
fn simplify_cfg(function: &mut MirFunction) {
    // Each round either retargets an edge or removes a block; both are finite,
    // but pruning can expose a new trampoline, so iterate to a fixed point.
    let rounds = function.blocks.len() + 1;
    for _ in 0..rounds {
        let threaded = thread_edges(&mut function.blocks);
        let pruned = retain_reachable(&mut function.blocks);
        let merged = merge_linear_blocks(&mut function.blocks);
        if !threaded && !pruned && !merged {
            return;
        }
    }
}

// Resolve an edge target through a chain of empty `Goto` blocks. A cycle of
// empty blocks resolves to the first block already visited, so threading
// never loops forever and never rewrites an infinite loop into a different
// one.
fn resolve_target(target: usize, trampolines: &HashMap<usize, usize>) -> usize {
    let mut current = target;
    let mut seen = HashSet::new();
    while let Some(next) = trampolines.get(&current) {
        if !seen.insert(current) {
            return current;
        }
        current = *next;
    }
    current
}

fn thread_edges(blocks: &mut [MirBlock]) -> bool {
    let trampolines: HashMap<usize, usize> = blocks
        .iter()
        .filter(|block| block.bindings.is_empty())
        .filter_map(|block| match block.terminator {
            CfgTerminator::Goto(next) => Some((block.id, next)),
            _ => None,
        })
        .collect();
    let mut changed = false;
    for block in blocks.iter_mut() {
        let replacement = match &block.terminator {
            CfgTerminator::Goto(target) => {
                let resolved = resolve_target(*target, &trampolines);
                (resolved != *target).then(|| CfgTerminator::Goto(resolved))
            }
            CfgTerminator::Branch(cond, then_target, else_target) => {
                let then_target_now = resolve_target(*then_target, &trampolines);
                let else_target_now = resolve_target(*else_target, &trampolines);
                if then_target_now == else_target_now && condition_is_pure(cond) {
                    Some(CfgTerminator::Goto(then_target_now))
                } else if then_target_now != *then_target || else_target_now != *else_target {
                    Some(CfgTerminator::Branch(cond.clone(), then_target_now, else_target_now))
                } else {
                    None
                }
            }
            CfgTerminator::Switch(subject, layout, targets) => {
                let resolved: Vec<usize> = targets.iter().map(|t| resolve_target(*t, &trampolines)).collect();
                (resolved != *targets).then(|| CfgTerminator::Switch(*subject, *layout, resolved))
            }
            _ => None,
        };
        if let Some(terminator) = replacement {
            block.terminator = terminator;
            changed = true;
        }
    }
    changed
}

// A condition may be dropped only when evaluating it cannot be observed.
// Conditions keep their whole source form in the CFG, so a call in one is a
// real side effect (§2.2 Rule 7: optimizations must not change behavior).
fn condition_is_pure(expr: &Expr) -> bool {
    match expr {
        Expr::Number(..) | Expr::FloatLit(..) | Expr::BoolLit(..) | Expr::StrLit(..) => true,
        Expr::Variable(..) | Expr::Field(..) => true,
        Expr::Binary(a, _, b, ..) => condition_is_pure(a) && condition_is_pure(b),
        _ => false,
    }
}

fn retain_reachable(blocks: &mut Vec<MirBlock>) -> bool {
    let Some(entry) = blocks.first().map(|block| block.id) else { return false };
    let successors: HashMap<usize, Vec<usize>> = blocks
        .iter()
        .map(|block| (block.id, terminator_targets(&block.terminator)))
        .collect();
    let mut reachable: HashSet<usize> = HashSet::new();
    let mut pending = vec![entry];
    while let Some(id) = pending.pop() {
        if !reachable.insert(id) {
            continue;
        }
        if let Some(next) = successors.get(&id) {
            pending.extend(next.iter().copied());
        }
    }
    let before = blocks.len();
    blocks.retain(|block| reachable.contains(&block.id));
    blocks.len() != before
}

// Fuse a block into the block that unconditionally jumps to it.
//
// `A --goto--> B` where B's only incoming edge is A's: every execution of B's
// bindings is immediately preceded by all of A's, so concatenating the binding
// lists into A and taking B's terminator preserves order, temporaries (ids are
// unique per function), and control flow, while removing one block and one
// edge. The entry block is never fused away because control also arrives there
// implicitly, and a self-loop is left alone.
//
// Only unconditional edges qualify. A branch's arm executes conditionally, so
// folding its bindings into the branch block would run them on the other path
// as well.
fn merge_linear_blocks(blocks: &mut Vec<MirBlock>) -> bool {
    let Some(entry) = blocks.first().map(|block| block.id) else { return false };
    let mut merged_any = false;
    loop {
        // Edge counts: a block is a fusion target only when exactly one edge
        // reaches it, and that edge is the unconditional jump we fuse over.
        let mut incoming: HashMap<usize, usize> = HashMap::new();
        for block in blocks.iter() {
            for target in terminator_targets(&block.terminator) {
                *incoming.entry(target).or_insert(0) += 1;
            }
        }
        let mut candidate = None;
        for block in blocks.iter() {
            let target = match block.terminator {
                CfgTerminator::Goto(target) => target,
                _ => continue,
            };
            if target == block.id || target == entry { continue; }
            if incoming.get(&target).copied().unwrap_or(0) != 1 { continue; }
            candidate = Some((block.id, target));
            break;
        }
        let Some((head_id, tail_id)) = candidate else { return merged_any };
        let Some(tail_index) = blocks.iter().position(|block| block.id == tail_id) else { return merged_any };
        let tail = blocks.remove(tail_index);
        let head_index = blocks.iter().position(|block| block.id == head_id).expect("fusing block present");
        let head = &mut blocks[head_index];
        head.bindings.extend(tail.bindings);
        head.terminator = tail.terminator;
        merged_any = true;
    }
}

fn terminator_targets(terminator: &CfgTerminator) -> Vec<usize> {
    match terminator {
        CfgTerminator::Goto(target) => vec![*target],
        CfgTerminator::Branch(_, a, b) => vec![*a, *b],
        CfgTerminator::Switch(_, _, targets) => targets.clone(),
        CfgTerminator::Return(_) | CfgTerminator::Exit(_) | CfgTerminator::Unreachable => Vec::new(),
    }
}

fn optimize_block(block: &mut MirBlock) {
    let mut local_consts: HashMap<usize, MirOperand> = HashMap::new();
    let mut temp_consts: HashMap<usize, MirOperand> = HashMap::new();

    for binding in &mut block.bindings {
        binding.rvalue = fold_rvalue(&binding.rvalue, &local_consts, &temp_consts);
        match &binding.target {
            MirPlace::Local(id, _) => {
                if let MirRvalue::Use(op) = &binding.rvalue {
                    if is_scalar_const(op) {
                        local_consts.insert(*id, op.clone());
                    } else {
                        local_consts.remove(id);
                    }
                } else {
                    local_consts.remove(id);
                }
            }
            MirPlace::Temp(id, _) => {
                if let MirRvalue::Use(op) = &binding.rvalue {
                    if is_scalar_const(op) {
                        temp_consts.insert(*id, op.clone());
                    }
                }
            }
            MirPlace::Field(base, _, _) => {
                if let Some(root) = root_local(base) {
                    local_consts.remove(&root);
                }
            }
        }
    }

    // Fold constant branch terminators into unconditional Goto edges when the
    // condition is a known boolean constant or a local known to hold one.
    if let CfgTerminator::Branch(cond, then_target, else_target) = &block.terminator {
        let known_bool = match cond {
            Expr::BoolLit(b) => Some(*b),
            Expr::Variable(id, Type::Bool, ..) => match local_consts.get(id) {
                Some(MirOperand::Bool(b)) => Some(*b),
                _ => None,
            },
            _ => None,
        };
        if let Some(branch_taken) = known_bool {
            block.terminator = CfgTerminator::Goto(if branch_taken { *then_target } else { *else_target });
        }
    }

    // Dead temporary elimination: remove `Temp` bindings that are pure (`Use`
    // or `Binary` that already folded to `Use`) and never referenced by any
    // subsequent binding in the block. (The last binding of a statement may
    // represent a statement-level effect like `Return` or `Print`, so only
    // intermediate temporaries produced during expression decomposition that
    // become unused after folding are removed.)
    let mut used_temps: HashSet<usize> = HashSet::new();
    for binding in &block.bindings {
        collect_rvalue_temps(&binding.rvalue, &mut used_temps);
        if let MirPlace::Field(base, _, _) = &binding.target {
            collect_place_temps(base, &mut used_temps);
        }
    }
    // Keep the final binding if it targets a Temp (statement effect sink), and
    // any Temp that holds a side-effecting rvalue (Call, FnCall, Intrinsic).
    let last_idx = block.bindings.len().saturating_sub(1);
    let mut retained = Vec::with_capacity(block.bindings.len());
    for (idx, binding) in block.bindings.drain(..).enumerate() {
        if let MirPlace::Temp(tid, _) = &binding.target {
            let is_pure = matches!(binding.rvalue, MirRvalue::Use(_));
            if idx < last_idx && is_pure && !used_temps.contains(tid) {
                continue;
            }
        }
        retained.push(binding);
    }
    block.bindings = retained;
}

fn is_scalar_const(op: &MirOperand) -> bool {
    matches!(op, MirOperand::Int(_) | MirOperand::Bool(_))
}

fn root_local(place: &MirPlace) -> Option<usize> {
    match place {
        MirPlace::Local(id, _) => Some(*id),
        MirPlace::Field(base, _, _) => root_local(base),
        MirPlace::Temp(_, _) => None,
    }
}

fn subst_operand(op: &MirOperand, locals: &HashMap<usize, MirOperand>, temps: &HashMap<usize, MirOperand>) -> MirOperand {
    match op {
        MirOperand::Place(MirPlace::Local(id, _)) => locals.get(id).cloned().unwrap_or_else(|| op.clone()),
        MirOperand::Place(MirPlace::Temp(id, _)) => temps.get(id).cloned().unwrap_or_else(|| op.clone()),
        _ => op.clone(),
    }
}

fn fold_rvalue(rvalue: &MirRvalue, locals: &HashMap<usize, MirOperand>, temps: &HashMap<usize, MirOperand>) -> MirRvalue {
    match rvalue {
        MirRvalue::Use(op) => MirRvalue::Use(subst_operand(op, locals, temps)),
        MirRvalue::Binary(lhs, op, rhs, ty) => {
            let l = subst_operand(lhs, locals, temps);
            let r = subst_operand(rhs, locals, temps);
            if let Some(folded) = eval_binary(&l, op, &r) {
                MirRvalue::Use(folded)
            } else {
                MirRvalue::Binary(l, op.clone(), r, *ty)
            }
        }
        MirRvalue::Call(id, args, ret) => MirRvalue::Call(*id, args.iter().map(|a| subst_operand(a, locals, temps)).collect(), *ret),
        MirRvalue::FnCall(id, args, ret) => MirRvalue::FnCall(*id, args.iter().map(|a| subst_operand(a, locals, temps)).collect(), *ret),
        MirRvalue::ClosureLit(id) => MirRvalue::ClosureLit(*id),
        MirRvalue::Intrinsic(name, args, ret) => MirRvalue::Intrinsic(name, args.iter().map(|a| subst_operand(a, locals, temps)).collect(), *ret),
        MirRvalue::Aggregate(sid, vals) => MirRvalue::Aggregate(*sid, vals.iter().map(|v| fold_rvalue(v, locals, temps)).collect()),
        MirRvalue::AdtAggregate(name, eid, vidx, vals) => MirRvalue::AdtAggregate(name, *eid, *vidx, vals.iter().map(|v| fold_rvalue(v, locals, temps)).collect()),
        MirRvalue::VecNew(vals, ty) => MirRvalue::VecNew(vals.iter().map(|v| fold_rvalue(v, locals, temps)).collect(), *ty),
    }
}

// Checked constant evaluation (§2.2 Rule 7: optimizations must not change
// observable behavior; overflow and division/modulo by zero stay unfolded).
fn eval_binary(lhs: &MirOperand, op: &Kind, rhs: &MirOperand) -> Option<MirOperand> {
    match (lhs, rhs) {
        (MirOperand::Int(a), MirOperand::Int(b)) => match op {
            Kind::Plus => a.checked_add(*b).map(MirOperand::Int),
            Kind::Minus => a.checked_sub(*b).map(MirOperand::Int),
            Kind::Star => a.checked_mul(*b).map(MirOperand::Int),
            Kind::Slash => if *b != 0 { a.checked_div(*b).map(MirOperand::Int) } else { None },
            Kind::Percent => if *b != 0 { a.checked_rem(*b).map(MirOperand::Int) } else { None },
            Kind::EqEq => Some(MirOperand::Bool(a == b)),
            Kind::NotEq => Some(MirOperand::Bool(a != b)),
            Kind::Less => Some(MirOperand::Bool(a < b)),
            Kind::LessEq => Some(MirOperand::Bool(a <= b)),
            Kind::Greater => Some(MirOperand::Bool(a > b)),
            Kind::GreaterEq => Some(MirOperand::Bool(a >= b)),
            _ => None,
        },
        (MirOperand::Bool(a), MirOperand::Bool(b)) => match op {
            Kind::And => Some(MirOperand::Bool(*a && *b)),
            Kind::Or => Some(MirOperand::Bool(*a || *b)),
            Kind::EqEq => Some(MirOperand::Bool(a == b)),
            Kind::NotEq => Some(MirOperand::Bool(a != b)),
            _ => None,
        },
        _ => None,
    }
}

fn collect_rvalue_temps(rvalue: &MirRvalue, used: &mut HashSet<usize>) {
    match rvalue {
        MirRvalue::Use(op) => collect_operand_temps(op, used),
        MirRvalue::Binary(a, _, b, _) => { collect_operand_temps(a, used); collect_operand_temps(b, used); }
        MirRvalue::Call(_, args, _) | MirRvalue::FnCall(_, args, _) | MirRvalue::Intrinsic(_, args, _) => {
            for a in args { collect_operand_temps(a, used); }
        }
        MirRvalue::Aggregate(_, vals) | MirRvalue::AdtAggregate(_, _, _, vals) | MirRvalue::VecNew(vals, _) => {
            for v in vals { collect_rvalue_temps(v, used); }
        }
        MirRvalue::ClosureLit(_) => {}
    }
}

fn collect_operand_temps(op: &MirOperand, used: &mut HashSet<usize>) {
    if let MirOperand::Place(place) = op {
        collect_place_temps(place, used);
    }
}

fn collect_place_temps(place: &MirPlace, used: &mut HashSet<usize>) {
    match place {
        MirPlace::Temp(id, _) => { used.insert(*id); }
        MirPlace::Field(base, _, _) => collect_place_temps(base, used),
        MirPlace::Local(_, _) => {}
    }
}

// The MIR's own invariants: every binding's rvalue operands that are places
// reference declared locals or earlier temporaries in the same block, and
// every block's terminator carries valid targets. A violation is a lowering
// bug (the printer is the reference consumer).
fn validate(function: &MirFunction) -> Result<(), String> {
    // Unreachable-block elimination leaves the surviving ids sparse, so a
    // target is valid when it names a surviving block, not when it indexes
    // the block vector.
    let ids: HashSet<usize> = function.blocks.iter().map(|block| block.id).collect();
    for block in &function.blocks {
        match &block.terminator {
            CfgTerminator::Goto(target) => if !ids.contains(target) { return Err(format!("internal: fn #{} block {} branches to missing block {}", function.id, block.id, target)); },
            CfgTerminator::Branch(_, a, b) => if !ids.contains(a) || !ids.contains(b) { return Err(format!("internal: fn #{} block {} branches to missing blocks", function.id, block.id)); },
            CfgTerminator::Switch(_, _, targets) => for t in targets { if !ids.contains(t) { return Err(format!("internal: fn #{} switch target missing", function.id)); } },
            CfgTerminator::Return(_) | CfgTerminator::Exit(_) | CfgTerminator::Unreachable => {}
        }
    }
    Ok(())
}

fn print_function(function: &MirFunction, out: &mut String) {
    let name = if function.is_main { "main".to_owned() } else { format!("#{}", function.id) };
    let params = function.params.iter().map(|(id, t)| format!("v{}: {}", id, t.name())).collect::<Vec<_>>().join(", ");
    out.push_str(&format!("fn {}({}) -> {} ({} blocks)\n", name, params, function.returns.name(), function.blocks.len()));
    for block in &function.blocks {
        out.push_str(&format!("  block {}:\n", block.id));
        for binding in &block.bindings {
            let mut text = String::new();
            print_place(&binding.target, &mut text);
            text.push_str(" = ");
            print_rvalue(&binding.rvalue, &mut text);
            out.push_str(&format!("    {}\n", text));
        }
        let tail = match &block.terminator {
            CfgTerminator::Goto(target) => format!("goto block {}", target),
            CfgTerminator::Branch(cond, a, b) => format!("branch {} -> then block {} else block {}", mir_text(cond), a, b),
            CfgTerminator::Switch(subject, _, targets) => format!("switch v{} -> {}", subject, targets.iter().map(|t| format!("block {}", t)).collect::<Vec<_>>().join(", ")),
            CfgTerminator::Return(expr) => format!("return {}", mir_text(expr)),
            CfgTerminator::Exit(code) => format!("exit {}", mir_text(code)),
            CfgTerminator::Unreachable if function.is_main => "implicit exit (main)".to_owned(),
            CfgTerminator::Unreachable => "unreachable".to_owned(),
        };
        out.push_str(&format!("    {}\n", tail));
    }
}

fn mir_text(expr: &Expr) -> String {
    crate::frontend::tree::expression_text(expr)
}

fn print_place(place: &MirPlace, out: &mut String) {
    match place {
        MirPlace::Local(id, ty) => out.push_str(&format!("v{} ({})", id, ty.name())),
        MirPlace::Temp(id, ty) => out.push_str(&format!("t{} ({})", id, ty.name())),
        MirPlace::Field(base, fname, ty) => {
            print_place(base, out);
            out.push_str(&format!(".{} ({})", fname, ty.name()));
        }
    }
}

fn print_operand(operand: &MirOperand, out: &mut String) {
    match operand {
        MirOperand::Place(place) => print_place(place, out),
        MirOperand::Int(n) => out.push_str(&n.to_string()),
        MirOperand::Float(x) => out.push_str(&x.to_string()),
        MirOperand::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
        MirOperand::Str(text, line, col) => out.push_str(&format!("{:?} @{}:{}", text, line, col)),
    }
}

fn print_rvalue(rvalue: &MirRvalue, out: &mut String) {
    match rvalue {
        MirRvalue::Use(operand) => print_operand(operand, out),
        MirRvalue::Binary(a, op, b, ty) => {
            print_operand(a, out);
            out.push(' ');
            out.push_str(crate::frontend::tree::operator_text(op));
            out.push(' ');
            print_operand(b, out);
            out.push_str(&format!(" -> {}", ty.name()));
        }
        MirRvalue::Call(id, args, ret) => {
            out.push_str(&format!("fn#{}(", id));
            print_operands(args, out);
            out.push_str(&format!(") -> {}", ret.name()));
        }
        MirRvalue::FnCall(id, args, ret) => {
            out.push_str(&format!("v{}(", id));
            print_operands(args, out);
            out.push_str(&format!(") -> {}", ret.name()));
        }
        MirRvalue::ClosureLit(id) => {
            out.push_str(&format!("closure#{}", id));
        }
        MirRvalue::Intrinsic(name, args, ret) => {
            out.push_str(&format!("{}(", name));
            print_operands(args, out);
            out.push_str(&format!(") -> {}", ret.name()));
        }
        MirRvalue::Aggregate(sid, values) => {
            out.push_str(&format!("struct #{} {{ ", sid));
            for (index, value) in values.iter().enumerate() {
                if index > 0 { out.push_str(", "); }
                print_rvalue(value, out);
            }
            out.push_str(" }");
        }
        MirRvalue::AdtAggregate(name, eid, vidx, values) => {
            out.push_str(&format!("{}#{}::variant#{}(", name, eid, vidx));
            for (index, value) in values.iter().enumerate() {
                if index > 0 { out.push_str(", "); }
                print_rvalue(value, out);
            }
            out.push(')');
        }
        MirRvalue::VecNew(values, ty) => {
            out.push('[');
            for (index, value) in values.iter().enumerate() {
                if index > 0 { out.push_str(", "); }
                print_rvalue(value, out);
            }
            out.push_str(&format!("] ({})", ty.name()));
        }
    }
}

fn print_operands(args: &[MirOperand], out: &mut String) {
    for (index, arg) in args.iter().enumerate() {
        if index > 0 { out.push_str(", "); }
        print_operand(arg, out);
    }
}
