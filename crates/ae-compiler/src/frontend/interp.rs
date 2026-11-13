//! M2.11: In-process execution interpreter with UB / checked-arithmetic /
//! use-after-move / bounds detection (`ae test --interp`, `ae run --interp`).
//!
//! Executes the lowered program directly in the compiler process without
//! invoking a C compiler or linker. Enforces the same runtime invariants as
//! the generated C runtime (`integer overflow`, `division by zero`,
//! `assertion failed`, `array index out of bounds`, `vector index out of bounds`,
//! `use of moved ...`), reporting failures in the exact `ae:line:col: msg`
//! diagnostic format.
use super::*;

#[derive(Debug, Clone)]
enum Val {
    Int(i64),
    Float(f64),
    Bool(bool),
    Str(String),
    Array(Vec<i64>),
    VecI(Vec<i64>),
    VecS(Vec<String>),
    VecF(Vec<f64>),
    Struct(#[allow(dead_code)] usize, HashMap<String, Val>),
    Enum(#[allow(dead_code)] usize, usize, Vec<Val>),
    Closure(usize),
    FnPtr(usize),
    Ref(usize),
    Moved,
}

enum Flow {
    Next,
    Return(Val),
    Break,
    Continue(usize),
    Exit(u8),
    Trap(String),
}

struct Env<'a> {
    program: &'a Program,
    fns: HashMap<usize, &'a Function>,
    out: String,
    steps: usize,
}

const MAX_STEPS: usize = 5_000_000;

pub fn interpret_path(path: &Path) -> Result<(String, u8), (String, String)> {
    let program = loader::load_program(path).map_err(|e| (String::new(), e))?;
    let mut fns = HashMap::new();
    let mut main_fn = None;
    for file in &program.files {
        for function in file {
            fns.insert(function.id, function);
            if function.is_main {
                main_fn = Some(function);
            }
        }
    }
    let main_fn = main_fn.ok_or_else(|| (String::new(), "program must define a main function".to_owned()))?;
    let mut env = Env { program: &program, fns, out: String::new(), steps: 0 };
    let mut locals: HashMap<usize, Val> = HashMap::new();
    match eval_stmts(&mut env, &main_fn.body, &mut locals, &mut None) {
        Flow::Next | Flow::Return(_) => Ok((env.out, 0)),
        Flow::Exit(code) => Ok((env.out, code)),
        Flow::Trap(msg) => Err((env.out, msg)),
        Flow::Break | Flow::Continue(_) => Err((env.out, "internal: unwound loop control in main".to_owned())),
    }
}

fn trap(msg: &str, line: usize, col: usize) -> Flow {
    Flow::Trap(format!("{line}:{col}: {msg}"))
}

fn eval_stmts(
    env: &mut Env<'_>,
    stmts: &[Statement],
    locals: &mut HashMap<usize, Val>,
    caller_locals: &mut Option<&mut HashMap<usize, Val>>,
) -> Flow {
    for stmt in stmts {
        env.steps += 1;
        if env.steps > MAX_STEPS {
            return Flow::Trap("interpreter step limit exceeded".to_owned());
        }
        match stmt {
            Statement::Declare(id, expr) => {
                match eval_expr(env, expr, locals, caller_locals, true) {
                    Ok(v) => { locals.insert(*id, v); }
                    Err(f) => return f,
                }
            }
            Statement::Assign(id, expr, _) => {
                match eval_expr(env, expr, locals, caller_locals, true) {
                    Ok(v) => { locals.insert(*id, v); }
                    Err(f) => return f,
                }
            }
            Statement::Array(id, exprs) => {
                let mut items = Vec::with_capacity(exprs.len());
                for e in exprs {
                    match eval_expr(env, e, locals, caller_locals, false) {
                        Ok(Val::Int(n)) => items.push(n),
                        Ok(_) => return Flow::Trap("internal: array element not i64".to_owned()),
                        Err(f) => return f,
                    }
                }
                locals.insert(*id, Val::Array(items));
            }
            Statement::Store(id, len, idx_expr, val_expr, line, col) => {
                let idx = match eval_expr(env, idx_expr, locals, caller_locals, false) {
                    Ok(Val::Int(n)) => n,
                    Ok(_) => return Flow::Trap("internal: index not i64".to_owned()),
                    Err(f) => return f,
                };
                let val = match eval_expr(env, val_expr, locals, caller_locals, false) {
                    Ok(Val::Int(n)) => n,
                    Ok(_) => return Flow::Trap("internal: array store value not i64".to_owned()),
                    Err(f) => return f,
                };
                if idx < 0 || (idx as usize) >= *len {
                    return trap("array index out of bounds", *line, *col);
                }
                if let Some(Val::Array(arr)) = locals.get_mut(id) {
                    arr[idx as usize] = val;
                }
            }
            Statement::FieldStore(id, fname, _, val_expr, through_ptr) => {
                let val = match eval_expr(env, val_expr, locals, caller_locals, true) {
                    Ok(v) => v,
                    Err(f) => return f,
                };
                if *through_ptr {
                    if let Some(Val::Ref(target_id)) = locals.get(id).cloned() {
                        if let Some(caller) = caller_locals.as_mut() {
                            if let Some(Val::Struct(_, fields)) = caller.get_mut(&target_id) {
                                fields.insert(fname.clone(), val);
                            }
                        }
                    }
                } else if let Some(Val::Struct(_, fields)) = locals.get_mut(id) {
                    fields.insert(fname.clone(), val);
                }
            }
            Statement::VecPush(id, val_expr, line, col) => {
                let val = match eval_expr(env, val_expr, locals, caller_locals, false) {
                    Ok(Val::Int(n)) => n,
                    Ok(_) => return Flow::Trap("internal: vec push type".to_owned()),
                    Err(f) => return f,
                };
                match locals.get_mut(id) {
                    Some(Val::VecI(v)) => v.push(val),
                    Some(Val::Moved) => return trap("use of moved vector", *line, *col),
                    _ => return Flow::Trap("internal: vec push on non-vec".to_owned()),
                }
            }
            Statement::StrVecPush(id, val_expr, line, col, _) => {
                let val = match eval_expr(env, val_expr, locals, caller_locals, false) {
                    Ok(Val::Str(s)) => s,
                    Ok(_) => return Flow::Trap("internal: strvec push type".to_owned()),
                    Err(f) => return f,
                };
                match locals.get_mut(id) {
                    Some(Val::VecS(v)) => v.push(val),
                    Some(Val::Moved) => return trap("use of moved vector", *line, *col),
                    _ => return Flow::Trap("internal: strvec push on non-vec".to_owned()),
                }
            }
            Statement::VecFPush(id, val_expr, line, col) => {
                let val = match eval_expr(env, val_expr, locals, caller_locals, false) {
                    Ok(Val::Float(x)) => x,
                    Ok(_) => return Flow::Trap("internal: f64vec push type".to_owned()),
                    Err(f) => return f,
                };
                match locals.get_mut(id) {
                    Some(Val::VecF(v)) => v.push(val),
                    Some(Val::Moved) => return trap("use of moved vector", *line, *col),
                    _ => return Flow::Trap("internal: f64vec push on non-vec".to_owned()),
                }
            }
            Statement::VecPop(id, line, col) | Statement::StrVecPop(id, line, col) | Statement::VecFPop(id, line, col) => {
                match locals.get_mut(id) {
                    Some(Val::VecI(v)) => {
                        if v.pop().is_none() { return trap("pop on empty vector", *line, *col); }
                    }
                    Some(Val::VecS(v)) => {
                        if v.pop().is_none() { return trap("pop on empty vector", *line, *col); }
                    }
                    Some(Val::VecF(v)) => {
                        if v.pop().is_none() { return trap("pop on empty vector", *line, *col); }
                    }
                    Some(Val::Moved) => return trap("use of moved vector", *line, *col),
                    _ => return Flow::Trap("internal: pop on non-vec".to_owned()),
                }
            }
            Statement::VecStore(id, idx_expr, val_expr, line, col) => {
                let idx = match eval_expr(env, idx_expr, locals, caller_locals, false) {
                    Ok(Val::Int(n)) => n,
                    Ok(_) => return Flow::Trap("internal: index not i64".to_owned()),
                    Err(f) => return f,
                };
                let val = match eval_expr(env, val_expr, locals, caller_locals, false) {
                    Ok(Val::Int(n)) => n,
                    Ok(_) => return Flow::Trap("internal: vec store type".to_owned()),
                    Err(f) => return f,
                };
                match locals.get_mut(id) {
                    Some(Val::VecI(v)) => {
                        if idx < 0 || (idx as usize) >= v.len() { return trap("vector index out of bounds", *line, *col); }
                        v[idx as usize] = val;
                    }
                    Some(Val::Moved) => return trap("use of moved vector", *line, *col),
                    _ => return Flow::Trap("internal: vec store on non-vec".to_owned()),
                }
            }
            Statement::StrVecStore(id, idx_expr, val_expr, line, col, _) => {
                let idx = match eval_expr(env, idx_expr, locals, caller_locals, false) {
                    Ok(Val::Int(n)) => n,
                    Ok(_) => return Flow::Trap("internal: index not i64".to_owned()),
                    Err(f) => return f,
                };
                let val = match eval_expr(env, val_expr, locals, caller_locals, false) {
                    Ok(Val::Str(s)) => s,
                    Ok(_) => return Flow::Trap("internal: strvec store type".to_owned()),
                    Err(f) => return f,
                };
                match locals.get_mut(id) {
                    Some(Val::VecS(v)) => {
                        if idx < 0 || (idx as usize) >= v.len() { return trap("vector index out of bounds", *line, *col); }
                        v[idx as usize] = val;
                    }
                    Some(Val::Moved) => return trap("use of moved vector", *line, *col),
                    _ => return Flow::Trap("internal: strvec store on non-vec".to_owned()),
                }
            }
            Statement::VecFStore(id, idx_expr, val_expr, line, col) => {
                let idx = match eval_expr(env, idx_expr, locals, caller_locals, false) {
                    Ok(Val::Int(n)) => n,
                    Ok(_) => return Flow::Trap("internal: index not i64".to_owned()),
                    Err(f) => return f,
                };
                let val = match eval_expr(env, val_expr, locals, caller_locals, false) {
                    Ok(Val::Float(x)) => x,
                    Ok(_) => return Flow::Trap("internal: f64vec store type".to_owned()),
                    Err(f) => return f,
                };
                match locals.get_mut(id) {
                    Some(Val::VecF(v)) => {
                        if idx < 0 || (idx as usize) >= v.len() { return trap("vector index out of bounds", *line, *col); }
                        v[idx as usize] = val;
                    }
                    Some(Val::Moved) => return trap("use of moved vector", *line, *col),
                    _ => return Flow::Trap("internal: f64vec store on non-vec".to_owned()),
                }
            }
            Statement::Sort(id, is_str, line, col) => {
                match locals.get_mut(id) {
                    Some(Val::VecI(v)) if !*is_str => v.sort(),
                    Some(Val::VecS(v)) if *is_str => v.sort(),
                    Some(Val::Moved) => return trap("use of moved vector", *line, *col),
                    _ => return Flow::Trap("internal: sort target".to_owned()),
                }
            }
            Statement::Reverse(id, is_str, line, col) => {
                match locals.get_mut(id) {
                    Some(Val::VecI(v)) if !*is_str => v.reverse(),
                    Some(Val::VecS(v)) if *is_str => v.reverse(),
                    Some(Val::Moved) => return trap("use of moved vector", *line, *col),
                    _ => return Flow::Trap("internal: reverse target".to_owned()),
                }
            }
            Statement::Print(printed) => {
                match printed {
                    Printed::Text(t) => {
                        env.out.push_str(&unescape_str(t));
                        env.out.push('\n');
                    }
                    Printed::Expr(e) => {
                        match eval_expr(env, e, locals, caller_locals, false) {
                            Ok(v) => {
                                env.out.push_str(&format_val(&v));
                                env.out.push('\n');
                            }
                            Err(f) => return f,
                        }
                    }
                }
            }
            Statement::Assert(cond, line, col) => {
                match eval_expr(env, cond, locals, caller_locals, false) {
                    Ok(Val::Bool(true)) => {}
                    Ok(Val::Bool(false)) => return trap("assertion failed", *line, *col),
                    Ok(_) => return Flow::Trap("internal: assert non-bool".to_owned()),
                    Err(f) => return f,
                }
            }
            Statement::If(cond, yes, no) => {
                let b = match eval_expr(env, cond, locals, caller_locals, false) {
                    Ok(Val::Bool(b)) => b,
                    Ok(_) => return Flow::Trap("internal: if condition non-bool".to_owned()),
                    Err(f) => return f,
                };
                let flow = if b {
                    eval_stmts(env, yes, locals, caller_locals)
                } else {
                    eval_stmts(env, no, locals, caller_locals)
                };
                if !matches!(flow, Flow::Next) {
                    return flow;
                }
            }
            Statement::While(cond, body) => {
                loop {
                    let b = match eval_expr(env, cond, locals, caller_locals, false) {
                        Ok(Val::Bool(b)) => b,
                        Ok(_) => return Flow::Trap("internal: while condition non-bool".to_owned()),
                        Err(f) => return f,
                    };
                    if !b { break; }
                    match eval_stmts(env, body, locals, caller_locals) {
                        Flow::Next | Flow::Continue(0) => continue,
                        Flow::Continue(d) => return Flow::Continue(d - 1),
                        Flow::Break => break,
                        other => return other,
                    }
                }
            }
            Statement::For(var, container, body, line, col, _) => {
                let items: Vec<Val> = match read_var(locals, caller_locals, *container, *line, *col) {
                    Ok(Val::VecI(v)) => v.into_iter().map(Val::Int).collect(),
                    Ok(Val::VecS(v)) => v.into_iter().map(Val::Str).collect(),
                    Ok(Val::VecF(v)) => v.into_iter().map(Val::Float).collect(),
                    Ok(Val::Array(v)) => v.into_iter().map(Val::Int).collect(),
                    Ok(_) => return Flow::Trap("internal: for container".to_owned()),
                    Err(f) => return f,
                };
                for item in items {
                    locals.insert(*var, item);
                    match eval_stmts(env, body, locals, caller_locals) {
                        Flow::Next | Flow::Continue(0) => continue,
                        Flow::Continue(d) => return Flow::Continue(d - 1),
                        Flow::Break => break,
                        other => return other,
                    }
                }
            }
            Statement::Match(subject, _, arms) => {
                let (vidx, payloads) = match read_var(locals, caller_locals, *subject, 0, 0) {
                    Ok(Val::Enum(_, vidx, payloads)) => (vidx, payloads),
                    Ok(_) => return Flow::Trap("internal: match subject not enum".to_owned()),
                    Err(f) => return f,
                };
                for arm in arms {
                    if arm.variant == vidx {
                        for (bind_id, _, field_idx) in &arm.bindings {
                            if let Some(p) = payloads.get(*field_idx) {
                                locals.insert(*bind_id, p.clone());
                            }
                        }
                        let flow = eval_stmts(env, &arm.body, locals, caller_locals);
                        if !matches!(flow, Flow::Next) {
                            return flow;
                        }
                        break;
                    }
                }
            }
            Statement::Return(expr, _) => {
                return match eval_expr(env, expr, locals, caller_locals, true) {
                    Ok(v) => Flow::Return(v),
                    Err(f) => f,
                };
            }
            Statement::Exit(expr, _) => {
                return match eval_expr(env, expr, locals, caller_locals, false) {
                    Ok(Val::Int(code)) => Flow::Exit((code & 0xFF) as u8),
                    Ok(_) => Flow::Exit(1),
                    Err(f) => f,
                };
            }
            Statement::Break => return Flow::Break,
            Statement::Continue(depth) => return Flow::Continue(depth.unwrap_or(1).saturating_sub(1)),
            Statement::Frees(_) => {}
            _ => return Flow::Trap("unsupported statement in MIR interpreter".to_owned()),
        }
    }
    Flow::Next
}

fn read_var(
    locals: &HashMap<usize, Val>,
    caller_locals: &Option<&mut HashMap<usize, Val>>,
    id: usize,
    line: usize,
    col: usize,
) -> Result<Val, Flow> {
    match locals.get(&id) {
        Some(Val::Moved) => Err(trap("use of moved value", line, col)),
        Some(Val::Ref(target_id)) => {
            if let Some(caller) = caller_locals.as_ref() {
                if let Some(v) = caller.get(target_id) {
                    return Ok(v.clone());
                }
            }
            Err(Flow::Trap("internal: dangling reference in interpreter".to_owned()))
        }
        Some(v) => Ok(v.clone()),
        None => Err(Flow::Trap(format!("internal: uninitialized local v{id}"))),
    }
}

fn eval_expr(
    env: &mut Env<'_>,
    expr: &Expr,
    locals: &mut HashMap<usize, Val>,
    caller_locals: &mut Option<&mut HashMap<usize, Val>>,
    moving: bool,
) -> Result<Val, Flow> {
    match expr {
        Expr::Number(n) => Ok(Val::Int(*n)),
        Expr::FloatLit(x) => Ok(Val::Float(*x)),
        Expr::BoolLit(b) => Ok(Val::Bool(*b)),
        Expr::StrLit(s, _, _) => Ok(Val::Str(unescape_str(s))),
        Expr::Variable(id, ty, line, col) => {
            let val = read_var(locals, caller_locals, *id, *line, *col)?;
            if moving && ty.owned() && !matches!(locals.get(id), Some(Val::Ref(_))) {
                locals.insert(*id, Val::Moved);
            }
            Ok(val)
        }
        Expr::Field(base, fname, _) => {
            let base_val = eval_expr(env, base, locals, caller_locals, false)?;
            match base_val {
                Val::Struct(_, mut fields) => fields.remove(fname).ok_or_else(|| Flow::Trap(format!("internal: missing field {fname}"))),
                _ => Err(Flow::Trap("internal: field access on non-struct".to_owned())),
            }
        }
        Expr::StructLit(sid, values) => {
            let def = env.program.structs.get(*sid).ok_or_else(|| Flow::Trap("internal: missing struct def".to_owned()))?.clone();
            let mut fields = HashMap::new();
            for ((fname, _), vexpr) in def.fields.iter().zip(values.iter()) {
                let v = eval_expr(env, vexpr, locals, caller_locals, true)?;
                fields.insert(fname.clone(), v);
            }
            Ok(Val::Struct(*sid, fields))
        }
        Expr::EnumLit(_, eid, vidx, values) => {
            let mut payloads = Vec::with_capacity(values.len());
            for vexpr in values {
                payloads.push(eval_expr(env, vexpr, locals, caller_locals, true)?);
            }
            Ok(Val::Enum(*eid, *vidx, payloads))
        }
        Expr::VecLit(values, ty, _, _) => {
            match ty {
                Type::Vec => {
                    let mut out = Vec::with_capacity(values.len());
                    for v in values {
                        if let Val::Int(n) = eval_expr(env, v, locals, caller_locals, false)? { out.push(n); }
                    }
                    Ok(Val::VecI(out))
                }
                Type::VecF => {
                    let mut out = Vec::with_capacity(values.len());
                    for v in values {
                        if let Val::Float(x) = eval_expr(env, v, locals, caller_locals, false)? { out.push(x); }
                    }
                    Ok(Val::VecF(out))
                }
                _ => Err(Flow::Trap("internal: unsupported vec literal type".to_owned())),
            }
        }
        Expr::StrVecNew(_, _) => Ok(Val::VecS(Vec::new())),
        Expr::F64VecNew(_, _) => Ok(Val::VecF(Vec::new())),
        Expr::Index(id, len, idx_expr, line, col) => {
            let idx = match eval_expr(env, idx_expr, locals, caller_locals, false)? {
                Val::Int(n) => n,
                _ => return Err(Flow::Trap("internal: index type".to_owned())),
            };
            if idx < 0 || (idx as usize) >= *len {
                return Err(trap("array index out of bounds", *line, *col));
            }
            match read_var(locals, caller_locals, *id, *line, *col)? {
                Val::Array(arr) => Ok(Val::Int(arr[idx as usize])),
                _ => Err(Flow::Trap("internal: array index on non-array".to_owned())),
            }
        }
        Expr::VecIndex(id, idx_expr, line, col, _, _) => {
            let idx = match eval_expr(env, idx_expr, locals, caller_locals, false)? {
                Val::Int(n) => n,
                _ => return Err(Flow::Trap("internal: vec index type".to_owned())),
            };
            match read_var(locals, caller_locals, *id, *line, *col)? {
                Val::VecI(v) => {
                    if idx < 0 || (idx as usize) >= v.len() { return Err(trap("vector index out of bounds", *line, *col)); }
                    Ok(Val::Int(v[idx as usize]))
                }
                Val::VecF(v) => {
                    if idx < 0 || (idx as usize) >= v.len() { return Err(trap("vector index out of bounds", *line, *col)); }
                    Ok(Val::Float(v[idx as usize]))
                }
                _ => Err(Flow::Trap("internal: vec index on non-vec".to_owned())),
            }
        }
        Expr::StrVecIndex(id, idx_expr, line, col, _) => {
            let idx = match eval_expr(env, idx_expr, locals, caller_locals, false)? {
                Val::Int(n) => n,
                _ => return Err(Flow::Trap("internal: strvec index type".to_owned())),
            };
            match read_var(locals, caller_locals, *id, *line, *col)? {
                Val::VecS(v) => {
                    if idx < 0 || (idx as usize) >= v.len() { return Err(trap("vector index out of bounds", *line, *col)); }
                    Ok(Val::Str(v[idx as usize].clone()))
                }
                _ => Err(Flow::Trap("internal: strvec index on non-vec".to_owned())),
            }
        }
        Expr::Len(inner, line, col) => {
            match eval_expr(env, inner, locals, caller_locals, false)? {
                Val::Str(s) => Ok(Val::Int(s.len() as i64)),
                Val::VecI(v) => Ok(Val::Int(v.len() as i64)),
                Val::VecS(v) => Ok(Val::Int(v.len() as i64)),
                Val::VecF(v) => Ok(Val::Int(v.len() as i64)),
                Val::Array(v) => Ok(Val::Int(v.len() as i64)),
                Val::Moved => Err(trap("use of moved value", *line, *col)),
                _ => Err(Flow::Trap("internal: len on unsupported value".to_owned())),
            }
        }
        Expr::Neg(inner, line, col) => {
            match eval_expr(env, inner, locals, caller_locals, false)? {
                Val::Int(n) => n.checked_neg().map(Val::Int).ok_or_else(|| trap("integer overflow", *line, *col)),
                Val::Float(x) => Ok(Val::Float(-x)),
                _ => Err(Flow::Trap("internal: neg operand".to_owned())),
            }
        }
        Expr::Not(inner) => {
            match eval_expr(env, inner, locals, caller_locals, false)? {
                Val::Bool(b) => Ok(Val::Bool(!b)),
                _ => Err(Flow::Trap("internal: not operand".to_owned())),
            }
        }
        Expr::StrEq(a, b, negated) => {
            let Val::Str(sa) = eval_expr(env, a, locals, caller_locals, false)? else { return Err(Flow::Trap("internal: streq".to_owned())); };
            let Val::Str(sb) = eval_expr(env, b, locals, caller_locals, false)? else { return Err(Flow::Trap("internal: streq".to_owned())); };
            Ok(Val::Bool(if *negated { sa != sb } else { sa == sb }))
        }
        Expr::ToString(inner, _, _) | Expr::FloatToString(inner, _, _) => {
            let v = eval_expr(env, inner, locals, caller_locals, false)?;
            Ok(Val::Str(format_val(&v)))
        }
        Expr::ToF64(inner, line, col) => {
            let Val::Int(n) = eval_expr(env, inner, locals, caller_locals, false)? else { return Err(Flow::Trap("internal: to_f64".to_owned())); };
            if !(-9007199254740992..=9007199254740992).contains(&n) {
                return Err(trap("i64 to f64 conversion would lose precision", *line, *col));
            }
            Ok(Val::Float(n as f64))
        }
        Expr::Binary(a, op, b, line, col, _) => {
            if *op == Kind::And {
                let Val::Bool(la) = eval_expr(env, a, locals, caller_locals, false)? else { return Err(Flow::Trap("internal: &&".to_owned())); };
                if !la { return Ok(Val::Bool(false)); }
                return eval_expr(env, b, locals, caller_locals, false);
            }
            if *op == Kind::Or {
                let Val::Bool(la) = eval_expr(env, a, locals, caller_locals, false)? else { return Err(Flow::Trap("internal: ||".to_owned())); };
                if la { return Ok(Val::Bool(true)); }
                return eval_expr(env, b, locals, caller_locals, false);
            }
            let va = eval_expr(env, a, locals, caller_locals, false)?;
            let vb = eval_expr(env, b, locals, caller_locals, false)?;
            match (va, vb) {
                (Val::Int(x), Val::Int(y)) => match op {
                    Kind::Plus => x.checked_add(y).map(Val::Int).ok_or_else(|| trap("integer overflow", *line, *col)),
                    Kind::Minus => x.checked_sub(y).map(Val::Int).ok_or_else(|| trap("integer overflow", *line, *col)),
                    Kind::Star => x.checked_mul(y).map(Val::Int).ok_or_else(|| trap("integer overflow", *line, *col)),
                    Kind::Slash => {
                        if y == 0 { return Err(trap("division by zero", *line, *col)); }
                        x.checked_div(y).map(Val::Int).ok_or_else(|| trap("integer overflow", *line, *col))
                    }
                    Kind::Percent => {
                        if y == 0 { return Err(trap("division by zero", *line, *col)); }
                        x.checked_rem(y).map(Val::Int).ok_or_else(|| trap("integer overflow", *line, *col))
                    }
                    Kind::EqEq => Ok(Val::Bool(x == y)),
                    Kind::NotEq => Ok(Val::Bool(x != y)),
                    Kind::Less => Ok(Val::Bool(x < y)),
                    Kind::LessEq => Ok(Val::Bool(x <= y)),
                    Kind::Greater => Ok(Val::Bool(x > y)),
                    Kind::GreaterEq => Ok(Val::Bool(x >= y)),
                    _ => Err(Flow::Trap("internal: int op".to_owned())),
                },
                (Val::Float(x), Val::Float(y)) => match op {
                    Kind::Plus => Ok(Val::Float(x + y)),
                    Kind::Minus => Ok(Val::Float(x - y)),
                    Kind::Star => Ok(Val::Float(x * y)),
                    Kind::Slash => {
                        if y == 0.0 { return Err(trap("f64 division by zero", *line, *col)); }
                        Ok(Val::Float(x / y))
                    }
                    Kind::EqEq => Ok(Val::Bool(x == y)),
                    Kind::NotEq => Ok(Val::Bool(x != y)),
                    Kind::Less => Ok(Val::Bool(x < y)),
                    Kind::LessEq => Ok(Val::Bool(x <= y)),
                    Kind::Greater => Ok(Val::Bool(x > y)),
                    Kind::GreaterEq => Ok(Val::Bool(x >= y)),
                    _ => Err(Flow::Trap("internal: float op".to_owned())),
                },
                (Val::Bool(x), Val::Bool(y)) => match op {
                    Kind::EqEq => Ok(Val::Bool(x == y)),
                    Kind::NotEq => Ok(Val::Bool(x != y)),
                    _ => Err(Flow::Trap("internal: bool op".to_owned())),
                },
                (Val::Str(x), Val::Str(y)) if *op == Kind::Plus => Ok(Val::Str(x + &y)),
                _ => Err(Flow::Trap("internal: binary operand mismatch".to_owned())),
            }
        }
        Expr::ClosureLit(cid) => Ok(Val::Closure(*cid)),
        Expr::Call(fn_id, args, _) => invoke_call(env, *fn_id, args, locals, caller_locals),
        Expr::FnCall(var_id, args, _) => {
            let target_fn = match read_var(locals, caller_locals, *var_id, 0, 0)? {
                Val::FnPtr(fid) => fid,
                Val::Closure(cid) => closures::closure_fn_id(cid),
                _ => return Err(Flow::Trap("internal: FnCall on non-function".to_owned())),
            };
            invoke_call(env, target_fn, args, locals, caller_locals)
        }
        _ => Err(Flow::Trap("unsupported expression in MIR interpreter".to_owned())),
    }
}

fn invoke_call(
    env: &mut Env<'_>,
    fn_id: usize,
    args: &[Argument],
    locals: &mut HashMap<usize, Val>,
    caller_locals: &mut Option<&mut HashMap<usize, Val>>,
) -> Result<Val, Flow> {
    let callee = *env.fns.get(&fn_id).ok_or_else(|| Flow::Trap(format!("internal: missing fn #{fn_id}")))?;
    let mut callee_locals: HashMap<usize, Val> = HashMap::new();
    for ((param_id, param_ty), arg) in callee.params.iter().zip(args.iter()) {
        let val = match arg {
            Argument::Value(e) => eval_expr(env, e, locals, caller_locals, true)?,
            Argument::View(e) => eval_expr(env, e, locals, caller_locals, false)?,
            Argument::Borrow(slot, _) => {
                if matches!(param_ty, Type::StructMut(_)) {
                    Val::Ref(*slot)
                } else {
                    read_var(locals, caller_locals, *slot, 0, 0)?
                }
            }
            Argument::FnPtr(cid) => Val::FnPtr(closures::closure_fn_id(*cid)),
        };
        callee_locals.insert(*param_id, val);
    }
    let mut parent = Some(&mut *locals);
    match eval_stmts(env, &callee.body, &mut callee_locals, &mut parent) {
        Flow::Return(v) => Ok(v),
        Flow::Next => Ok(Val::Int(0)),
        other => Err(other),
    }
}

fn unescape_str(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    let mut chars = raw.chars();
    while let Some(c) = chars.next() {
        if c == '\\' {
            match chars.next() {
                Some('n') => out.push('\n'),
                Some('r') => out.push('\r'),
                Some('t') => out.push('\t'),
                Some('\\') => out.push('\\'),
                Some('"') => out.push('"'),
                Some('0') => out.push('\0'),
                Some(other) => { out.push('\\'); out.push(other); }
                None => out.push('\\'),
            }
        } else {
            out.push(c);
        }
    }
    out
}

fn format_val(val: &Val) -> String {
    match val {
        Val::Int(n) => n.to_string(),
        Val::Bool(b) => if *b { "true".to_owned() } else { "false".to_owned() },
        Val::Str(s) => s.clone(),
        Val::Float(x) => {
            if x.is_nan() { return "nan".to_owned(); }
            if x.is_infinite() { return if *x < 0.0 { "-inf".to_owned() } else { "inf".to_owned() }; }
            if *x == 0.0 && x.is_sign_negative() { return "-0.0".to_owned(); }
            let s = format!("{x:?}");
            if s.contains(['.', 'e', 'E']) { s } else { format!("{s}.0") }
        }
        _ => "<val>".to_owned(),
    }
}
