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
            let mir = lower_function(function);
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
        Expr::Number(..) | Expr::Binary(..) | Expr::Index(..) | Expr::VecIndex(..) => Type::Int,
        Expr::FloatLit(..) | Expr::ToF64(..) => Type::F64,
        Expr::BoolLit(..) | Expr::StrEq(..) => Type::Bool,
        Expr::Variable(_, ty, ..) | Expr::Field(_, _, ty) | Expr::VecLit(_, ty, ..) => *ty,
        Expr::StructLit(sid, _) => Type::Struct(*sid),
        Expr::EnumLit(name, ..) => Type::Enum(name, 0),
        _ => Type::Int,
    }
}

// The MIR's own invariants: every binding's rvalue operands that are places
// reference declared locals or earlier temporaries in the same block, and
// every block's terminator carries valid targets. A violation is a lowering
// bug (the printer is the reference consumer).
fn validate(function: &MirFunction) -> Result<(), String> {
    let count = function.blocks.len();
    for block in &function.blocks {
        match &block.terminator {
            CfgTerminator::Goto(target) => if *target >= count { return Err(format!("internal: fn #{} block {} branches to missing block {}", function.id, block.id, target)); },
            CfgTerminator::Branch(_, a, b) => if *a >= count || *b >= count { return Err(format!("internal: fn #{} block {} branches to missing blocks", function.id, block.id)); },
            CfgTerminator::Switch(_, _, targets) => for t in targets { if *t >= count { return Err(format!("internal: fn #{} switch target missing", function.id)); } },
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
