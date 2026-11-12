use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::rc::Rc;
mod parser;
mod codegen;
mod c_output;
pub(crate) use ae_syntax::{Kind, Token, lex};
use ae_syntax::error;
#[allow(unused_imports)]
use codegen::*;
use c_output::*;



#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Type { Int, Bool, F64, String, StrView, Vec, StrVec, VecF, VecRef, StrVecRef, VecFRef, Map, MapF, Array(usize), Struct(usize), Enum(&'static str, usize), Tokens }
impl Type {
    fn name(self) -> String {
        match self {
            Type::Int => "i64".to_owned(),
            Type::Bool => "bool".to_owned(),
            Type::F64 => "f64".to_owned(),
            Type::String => "string".to_owned(),
            Type::StrView => "string view".to_owned(),
            Type::Vec => "vec".to_owned(),
            Type::StrVec => "vec[string]".to_owned(),
            Type::VecF => "vec[f64]".to_owned(),
            Type::VecRef => "&vec".to_owned(),
            Type::StrVecRef => "&vec[string]".to_owned(),
            Type::VecFRef => "&vec[f64]".to_owned(),
            // One map shape for now: string keys, i64 values.
            Type::Map => "map[string, i64]".to_owned(),
            Type::MapF => "map[string, f64]".to_owned(),
            Type::Array(n) => format!("&[i64; {n}]"),
            Type::Struct(id) => format!("struct #{}", id),
            Type::Enum(name, _) => name.to_owned(),
            // A single-pass cursor over a string; it owns no memory and only
            // `for` consumes it (ADR-53).
            Type::Tokens => "token cursor".to_owned(),
        }
    }
    fn c_temp(self) -> String {
        match self {
            Type::Bool => "bool ".to_owned(),
            Type::F64 => "double ".to_owned(),
            Type::String => "ae_string ".to_owned(),
            Type::StrView => "ae_strview ".to_owned(),
            Type::Vec => "ae_vec ".to_owned(),
            Type::StrVec => "ae_strvec ".to_owned(),
            Type::VecF => "ae_f64vec ".to_owned(),
            Type::Map => "ae_map ".to_owned(),
            Type::MapF => "ae_mapf ".to_owned(),
            Type::Struct(k) => format!("ae_t{k} "),
            Type::Enum(_, k) => format!("ae_e{k} "),
            Type::Tokens => "ae_tokens ".to_owned(),
            _ => "int64_t ".to_owned(),
        }
    }
    // Owned types carry heap resources: they move, and every live slot is
    // released on scope exit and control-flow jumps. Enums own their payload
    // slots, which may hold strings.
    fn owned(self) -> bool {
        matches!(self, Type::String | Type::Vec | Type::StrVec | Type::VecF | Type::Map | Type::MapF | Type::Struct(_) | Type::Enum(..))
    }
}
enum MoveKind { Var(usize), Field, StrVecElement }
// Names owned by the prelude rather than by any file on disk.
const PRELUDE: &str = "<prelude>";
// Cap on enum layouts per program, prelude and instantiations included. Each
// layout is a tag enum plus one struct per payload variant, so an unbounded
// instantiation count is a real code-size hazard (ADR-21).
const MAX_ENUM_TYPES: usize = 256;
const MAX_STRUCT_TYPES: usize = 256;
// Built-in names are reserved so their semantics stay unambiguous.
// `pub` is reserved too: it is only meaningful before a top-level item.
const RESERVED: &[&str] = &["len", "push", "pop", "vec", "read_file", "write_file", "bytes", "contains", "slice", "split", "args", "join", "slice_view", "lines", "count_lines_with", "try_read_file", "try_write_file", "parse_int", "to_string", "try_list_dir", "path_exists", "path_is_dir", "path_join", "path_name", "path_parent", "try_file_info", "try_copy_file", "try_rename_path", "try_remove_file", "try_create_dir", "try_remove_dir", "map_new", "f64map_new", "map_put", "map_get", "map_has", "map_remove", "map_inc", "keys", "utf8_len", "utf8_prefix", "pub", "trim", "starts_with", "ends_with", "find", "sort", "reverse", "now_ms", "now_s", "env", "try_append_file", "exit", "read_stdin", "cwd", "json_get", "json_int", "json_bool", "json_len", "json_has", "strvec_new", "f64vec_new", "run_command", "set_cwd", "to_f64", "to_i64", "tokenize", "sqrt", "abs", "floor", "ceil", "round", "exp", "log", "sin", "cos", "pow", "atan2", "min", "max"];
// The math builtins (ADR-60): AE name, C symbol, arity. Every argument and
// the result is f64, and each call maps 1:1 onto C libm, so domain errors are
// IEEE NaN/-inf instead of language-level failures — unlike the checked `/`
// operator, whose contract is part of the language itself. `abs` maps to
// `fabs`: C's plain `abs` is the integer function in <stdlib.h>.
const MATH_FNS: &[(&str, &str, usize)] = &[
    ("sqrt", "sqrt", 1), ("abs", "fabs", 1), ("floor", "floor", 1),
    ("ceil", "ceil", 1), ("round", "round", 1), ("exp", "exp", 1),
    ("log", "log", 1), ("sin", "sin", 1), ("cos", "cos", 1),
    ("pow", "pow", 2), ("atan2", "atan2", 2),
    // min/max map to runtime symbols, not libm: the NaN-ignoring semantics
    // and the ±0.0 tie rule must be bit-identical on both platforms (ADR-64).
    ("min", "ae_f64_min", 2), ("max", "ae_f64_max", 2),
];
// Provisional prelude: fallible operations report failure as values instead of
// terminating the process (§4.5). These enums are registered before any user
// type, so a program can name them without importing anything. They are
// expected to move into a stdlib module once generics and a module search path
// exist (ADR-11).
fn prelude_enums() -> (Vec<EnumDef>, HashMap<String, usize>) {
    let defs: Vec<(&'static str, Vec<(&'static str, Vec<Type>)>)> = vec![
        ("ReadResult", vec![("Ok", vec![Type::String]), ("Err", vec![Type::String])]),
        ("WriteResult", vec![("Ok", vec![Type::Int]), ("Err", vec![Type::String])]),
        ("IntResult", vec![("Ok", vec![Type::Int]), ("Err", vec![Type::String])]),
        ("DirResult", vec![("Ok", vec![Type::StrVec]), ("Err", vec![Type::String])]),
        // Metadata: byte size, modification time in seconds since the Unix
        // epoch, and whether the target is a directory.
        ("FileResult", vec![("Ok", vec![Type::Int, Type::Int, Type::Bool]), ("Err", vec![Type::String])]),
        // Operations with no value to report: success carries nothing.
        ("StatusResult", vec![("Ok", vec![]), ("Err", vec![Type::String])]),
        // JSON answers: a document field can be a flag, so booleans need their
        // own result shape (there is no "true" placeholder like 0 or "").
        ("BoolResult", vec![("Ok", vec![Type::Bool]), ("Err", vec![Type::String])]),
        // A finished child process: a non-zero status is a result, not an
        // error, so the status travels in Ok next to the output it produced.
        ("CommandResult", vec![("Ok", vec![Type::Int, Type::String]), ("Err", vec![Type::String])]),
        // map[string, f64] lookups (ADR-65). Appended last so every existing
        // prelude enum keeps its id; the runtime names it ae_e8.
        ("FloatResult", vec![("Ok", vec![Type::F64]), ("Err", vec![Type::String])]),
    ];
    let mut enums = Vec::new();
    let mut names = HashMap::new();
    for (name, variants) in defs {
        names.insert(name.to_owned(), enums.len());
        enums.push(EnumDef { name, variants: variants.into_iter().map(|(v, fields)| (v.to_owned(), fields)).collect(), file: PRELUDE.to_owned(), public: true, pattern: name.to_owned() });
    }
    (enums, names)
}

// `Option[T]` ships with the prelude: "a value that may be absent" is the one
// generic shape every program needs, and building it in keeps programs from
// each declaring a slightly different `Maybe`.
fn prelude_generics() -> Generics {
    let mut generics = Generics::default();
    generics.names.insert("Option".to_owned(), 0);
    generics.defs.push(GenericEnum {
        name: "Option".to_owned(),
        params: vec!["T".to_owned()],
        variants: vec![("Some".to_owned(), vec![Payload::Param(0)]), ("None".to_owned(), Vec::new())],
        file: PRELUDE.to_owned(),
        public: true,
    });
    generics
}


// String literals keep their source text with escapes uninterpreted: the bytes
// between the quotes are already valid C string body, so emission can reuse them.
#[derive(Debug)]
enum Expr { Number(i64), FloatLit(f64), ToF64(Box<Expr>, usize, usize), ToI64(Box<Expr>, Type, usize, usize), FloatToString(Box<Expr>, usize, usize), BoolLit(bool), StrLit(String, usize, usize), Variable(usize, Type, usize, usize), Call(usize, Vec<Argument>, Type), Index(usize, usize, Box<Expr>, usize, usize), VecLit(Vec<Expr>, Type, usize, usize), VecIndex(usize, Box<Expr>, usize, usize, bool, Type), Len(Box<Expr>, usize, usize), ReadFile(Box<Expr>, usize, usize), WriteFile(Box<Expr>, Box<Expr>, usize, usize), Bytes(Box<Expr>, usize, usize), Contains(Box<Expr>, Box<Expr>, usize, usize), Slice(Box<Expr>, Box<Expr>, Box<Expr>, usize, usize), Split(Box<Expr>, Box<Expr>, usize, usize), Args(usize, usize), Join(usize, Box<Expr>, usize, usize), StrVecIndex(usize, Box<Expr>, usize, usize, bool), SliceView(usize, Option<String>, Box<Expr>, Box<Expr>, usize, usize),
    // Lazy token cursor: (base variable, base field, separator, line, column).
    Tokens(usize, Option<String>, Box<Expr>, usize, usize), Lines(Box<Expr>, usize, usize), CountLinesWith(Box<Expr>, Box<Expr>, usize, usize), StructLit(usize, Vec<Expr>), EnumLit(&'static str, usize, usize, Vec<Expr>), TryReadFile(Box<Expr>, Type, usize, usize), TryListDir(Box<Expr>, Type, usize, usize), PathExists(Box<Expr>, usize, usize), PathIsDir(Box<Expr>, usize, usize), TryWriteFile(Box<Expr>, Box<Expr>, Type, usize, usize), ParseInt(Box<Expr>, Type, usize, usize), ToString(Box<Expr>, usize, usize), PathJoin(Box<Expr>, Box<Expr>, usize, usize), PathName(Box<Expr>, usize, usize), PathParent(Box<Expr>, usize, usize), TryFileInfo(Box<Expr>, Type, usize, usize), TryCopyFile(Box<Expr>, Box<Expr>, Type, usize, usize), TryRenamePath(Box<Expr>, Box<Expr>, Type, usize, usize), TryRemoveFile(Box<Expr>, Type, usize, usize), TryRemoveDir(Box<Expr>, Type, usize, usize), TryCreateDir(Box<Expr>, Type, usize, usize), SetCwd(Box<Expr>, Type, usize, usize), MapNew(usize, usize, MapKind), MapGet(MapPlace, Box<Expr>, Type, usize, usize), MapHas(MapPlace, Box<Expr>, usize, usize), MapRemove(MapPlace, Box<Expr>, usize, usize), MapInc(MapPlace, Box<Expr>, Box<Expr>, usize, usize), MapKeys(MapPlace, usize, usize), Utf8Len(Box<Expr>, usize, usize), Utf8Prefix(Box<Expr>, Box<Expr>, usize, usize), Trim(Box<Expr>, usize, usize), StartsWith(Box<Expr>, Box<Expr>), EndsWith(Box<Expr>, Box<Expr>),
    TryAppendFile(Box<Expr>, Box<Expr>, Type, usize, usize), Env(Box<Expr>, Type, usize, usize), JsonGet(Box<Expr>, Box<Expr>, Type, usize, usize), JsonInt(Box<Expr>, Box<Expr>, Type, usize, usize), JsonBool(Box<Expr>, Box<Expr>, Type, usize, usize), JsonLen(Box<Expr>, Box<Expr>, Type, usize, usize), JsonHas(Box<Expr>, Box<Expr>, usize, usize), Find(Box<Expr>, Box<Expr>, Type, usize, usize), ReadStdin(Type, usize, usize), Cwd(Type, usize, usize), StrVecNew(usize, usize), F64VecNew(usize, usize), RunCommand(Box<Expr>, usize, Type, usize, usize), NowMs, NowS, Field(Box<Expr>, String, Type), StrEq(Box<Expr>, Box<Expr>, bool), Neg(Box<Expr>, usize, usize), Not(Box<Expr>),
    // Math builtin (ADR-60): index into MATH_FNS, arguments, source position.
    MathCall(u8, Vec<Box<Expr>>, usize, usize), Binary(Box<Expr>, Kind, Box<Expr>, usize, usize, Type) }
// Which value type a map holds: the builtins dispatch on it (ADR-65).
#[derive(Debug, Clone, Copy, PartialEq)]
enum MapKind { Int, Float }
// A map reachable without a move: either a variable slot or a field of one.
#[derive(Debug)]
enum MapPlace { Var(usize, MapKind), Field(usize, String, MapKind) }
impl MapPlace {
    fn text(&self) -> String {
        match self {
            MapPlace::Var(id, _) => format!("v{id}"),
            MapPlace::Field(id, name, _) => format!("v{id}.{name}"),
        }
    }
    fn slot(&self) -> usize {
        match self {
            MapPlace::Var(id, _) | MapPlace::Field(id, _, _) => *id,
        }
    }
    fn f64_value(&self) -> bool {
        matches!(self, MapPlace::Var(_, MapKind::Float) | MapPlace::Field(_, _, MapKind::Float))
    }
}
fn map_kind_of(ty: Type) -> MapKind {
    if ty == Type::MapF { MapKind::Float } else { MapKind::Int }
}
fn map_element_ty(kind: MapKind) -> Type {
    if kind == MapKind::Float { Type::F64 } else { Type::Int }
}
#[derive(Debug)]
enum Argument { Value(Expr), Borrow(usize, bool), View(Expr) } // Borrow bool: pass &slot vs array (decays); View: read-only string borrow
#[derive(Debug)]
enum Printed { Expr(Expr), Text(String) }
#[derive(Debug)]
enum Statement { Return(Expr, Vec<(usize, Type)>), Declare(usize, Expr), Array(usize, Vec<Expr>), Assign(usize, Expr, bool), Store(usize, usize, Expr, Expr, usize, usize), VecPush(usize, Expr, usize, usize), Sort(usize, bool, usize, usize), Reverse(usize, bool, usize, usize), VecPop(usize, usize, usize), VecStore(usize, Expr, Expr, usize, usize), StrVecPush(usize, Expr, usize, usize, bool), StrVecPop(usize, usize, usize), StrVecStore(usize, Expr, Expr, usize, usize, bool), VecFPush(usize, Expr, usize, usize), VecFPop(usize, usize, usize), VecFStore(usize, Expr, Expr, usize, usize), FieldStore(usize, String, Type, Expr), Print(Printed), Assert(Expr, usize, usize), If(Expr, Vec<Statement>, Vec<Statement>), While(Expr, Vec<Statement>), For(usize, usize, Vec<Statement>, usize, usize, usize), ForTokens(usize, usize, Vec<Statement>, usize, usize, usize), Match(usize, Type, Vec<MatchArm>), MapPut(MapPlace, Expr, Expr, usize, usize), Propagate(Expr, Type, Vec<Type>, Option<(usize, Type)>, Vec<(usize, Type)>), Break, Continue(Option<usize>), Exit(Expr, Vec<(usize, Type)>), Frees(Vec<(usize, Type)>) }
#[derive(Debug)]
struct Function { id: usize, params: Vec<(usize, Type)>, body: Vec<Statement>, is_main: bool, returns: Type }
fn returns(stmts: &[Statement]) -> bool {
    stmts.iter().any(|s| match s {
        Statement::Return(..) => true,
        // Terminating the process is a valid way to end a function that never
        // produces a value.
        Statement::Exit(..) => true,
        Statement::If(_, yes, no) => returns(yes) && returns(no),
        // Exhaustive coverage means the match always runs one arm, so the
        // match returns whenever every arm does.
        Statement::Match(_, _, arms) => !arms.is_empty() && arms.iter().all(|arm| returns(&arm.body)),
        _ => false,
    })
}
#[derive(Debug, Clone)]
struct StructDef { name: String, fields: Vec<(String, Type)>, file: String, public: bool }
// Enum layouts are closed: variants carry a fixed field list, and the first
// version restricts payload fields to i64/bool/string so no enum can contain
// itself and every payload has a trivial release.
#[derive(Debug, Clone)]
struct EnumDef { name: &'static str, variants: Vec<(String, Vec<Type>)>, file: String, public: bool, pattern: String }
// A payload type in a generic declaration: either fixed, or one of the
// declaration's type parameters. Substitution replaces every Param with a
// concrete argument, so Param never reaches code generation.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Payload { Concrete(Type), Param(usize) }
// `enum Option[T] { Some(T), None }` before it is instantiated. Kept apart from
// EnumDef because a declaration has no layout: only instantiations do.
#[derive(Debug, Clone)]
struct GenericEnum { name: String, params: Vec<String>, variants: Vec<(String, Vec<Payload>)>, file: String, public: bool }
// A trait declares the method signatures every impl must provide. The trait's
// type parameter is always spelled `T`; `Payload::Param(0)` marks the positions
// that stand for it, and every other type must be concrete. V1 deliberately
// has no generic impls and no default method bodies.
#[derive(Debug, Clone)]
struct TraitMethod { name: String, params: Vec<Payload>, returns: Payload }
#[derive(Debug, Clone)]
struct TraitDef { name: String, methods: Vec<TraitMethod>, file: String, public: bool }
// `struct Pair[K, V] { left: K, right: V }` before it is instantiated. Same
// reason as GenericEnum: a declaration is a shape, not a layout.
#[derive(Debug, Clone)]
struct GenericStruct { name: String, params: Vec<String>, fields: Vec<(String, Payload)>, file: String, public: bool }
// A constraint on a generic function's type parameter is a *shape check* at
// the call site, not a proof about the generic body (ADR-33). The body is still
// parsed and type-checked once per instantiation, so a constraint can only
// reject an argument earlier and with a better message.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Bound { Add, Eq, Ord, Copy }
impl Bound {
    fn name(self) -> &'static str {
        match self { Bound::Add => "Add", Bound::Eq => "Eq", Bound::Ord => "Ord", Bound::Copy => "Copy" }
    }
    fn allows(self, ty: Type) -> bool {
        match self {
            Bound::Copy => matches!(ty, Type::Int | Type::Bool | Type::F64),
            Bound::Eq => matches!(ty, Type::Int | Type::Bool | Type::String | Type::F64),
            Bound::Ord => matches!(ty, Type::Int | Type::F64),
            Bound::Add => matches!(ty, Type::Int | Type::String | Type::F64),
        }
    }
    fn allowed_types(self) -> &'static str {
        match self {
            Bound::Copy => "i64, bool, f64",
            Bound::Eq => "i64, bool, string, f64",
            Bound::Ord => "i64, f64",
            Bound::Add => "i64, string, f64",
        }
    }
}

// ADR-40. A generic declaration that writes constraints must not admit a type
// that its body cannot use. This is a diagnostic, not a proof: every
// instantiation is still parsed and fully type-checked (ADR-33). The audit can
// only reject a declaration; it never lets unchecked code through.
const AUDIT_TYPES: &[Type] = &[
    Type::Int, Type::Bool, Type::F64, Type::String, Type::StrView,
    Type::Vec, Type::StrVec, Type::VecF, Type::VecRef, Type::StrVecRef, Type::VecFRef, Type::Map, Type::MapF,
    Type::Array(0), Type::Struct(0), Type::Enum("audit", 0),
];

// Capabilities mirror the binary-operator rules in `expression`. They live
// next to `Bound::allows` so adding a scalar type cannot silently desync the
// constraint whitelist and the real type checker.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Capability { Add, Arith, Modulo, Ord, Eq, Logic }
impl Capability {
    fn from_op(op: &Kind) -> Option<Capability> {
        match op {
            Kind::Plus => Some(Capability::Add),
            Kind::Minus | Kind::Star | Kind::Slash => Some(Capability::Arith),
            Kind::Percent => Some(Capability::Modulo),
            Kind::Less | Kind::LessEq | Kind::Greater | Kind::GreaterEq => Some(Capability::Ord),
            Kind::EqEq | Kind::NotEq => Some(Capability::Eq),
            Kind::And | Kind::Or => Some(Capability::Logic),
            _ => None,
        }
    }
    fn symbol(self) -> &'static str {
        match self {
            Capability::Add => "'+'", Capability::Arith => "arithmetic",
            Capability::Modulo => "'%'", Capability::Ord => "ordering comparison",
            Capability::Eq => "equality comparison", Capability::Logic => "'&&' / '||'",
        }
    }
    fn supports(self, ty: Type) -> bool {
        match self {
            Capability::Add => matches!(ty, Type::Int | Type::F64 | Type::String | Type::StrView),
            Capability::Arith => matches!(ty, Type::Int | Type::F64),
            Capability::Modulo => matches!(ty, Type::Int),
            Capability::Ord => matches!(ty, Type::Int | Type::F64),
            Capability::Eq => matches!(ty, Type::Int | Type::F64 | Type::Bool | Type::String | Type::StrView),
            Capability::Logic => matches!(ty, Type::Bool),
        }
    }
    fn supported_types(self) -> &'static str {
        match self {
            Capability::Add => "i64, f64, string",
            Capability::Arith => "i64, f64",
            Capability::Modulo => "i64",
            Capability::Ord => "i64, f64",
            Capability::Eq => "i64, f64, bool, string",
            Capability::Logic => "bool",
        }
    }
}

static AUDIT_EOF: Kind = Kind::Eof;
// Borrow instead of cloning: the audit walks every token of a declaration, and
// cloning `Kind::Ident(String)` on each visit would add a heap allocation per
// identifier for a check that must stay cheap on the front-end path.
fn audit_kind(tokens: &[Token], i: usize) -> &Kind {
    tokens.get(i).map(|t| &t.kind).unwrap_or(&AUDIT_EOF)
}

// `start` points at the parameter list, `end` just past the closing brace.
// Only declarations with at least one constraint are audited: an unconstrained
// generic keeps ADR-33 behaviour, where the per-instantiation check reports the
// error. Unproven cases are skipped rather than rejected, so the check cannot
// break a valid program; correctness still rests on the full type check.
fn audit_generic_fn(
    file: &str, source: &str, tokens: &[Token], start: usize, end: usize,
    name: &str, params: &[String], bounds: &[Vec<Bound>], declared_at: usize,
) -> Result<(), ae_span::Diagnostic> {
    if !bounds.iter().any(|b| !b.is_empty()) { return Ok(()); }
    let at = |i: usize| audit_kind(tokens, i);
    if *at(start) != Kind::LParen { return Ok(()); }
    let mut depth = 0usize;
    let mut rparen = None;
    for i in start..end {
        match at(i) {
            Kind::LParen => depth += 1,
            Kind::RParen => { depth -= 1; if depth == 0 { rparen = Some(i); break; } }
            _ => {}
        }
    }
    let Some(rparen) = rparen else { return Ok(()); };

    // Bind each parameter variable to a type-parameter index. Concrete types,
    // `&T` and compound types are deliberately left unknown, so an operator can
    // never be attributed to the wrong type parameter.
    let mut vars: HashMap<String, Option<usize>> = HashMap::new();
    let mut piece = start + 1;
    let mut pieces = Vec::new();
    for i in (start + 1)..rparen {
        if *at(i) == Kind::Comma { pieces.push((piece, i)); piece = i + 1; }
    }
    pieces.push((piece, rparen));
    for (a, b) in pieces {
        let Some(colon) = (a..b).find(|&i| *at(i) == Kind::Colon) else { continue; };
        if colon == a { continue; }
        let Kind::Ident(var) = at(colon - 1) else { continue; };
        let bound_to = if b == colon + 2 {
            match at(colon + 1) {
                Kind::Ident(t) => params.iter().position(|p| p == t),
                _ => None,
            }
        } else { None };
        vars.insert(var.clone(), bound_to);
    }

    let Some(body_start) = ((rparen + 1)..end).find(|&i| *at(i) == Kind::LBrace) else { return Ok(()); };
    let mut depth = 0usize;
    let mut body_end = None;
    for i in body_start..end {
        match at(i) {
            Kind::LBrace => depth += 1,
            Kind::RBrace => { depth -= 1; if depth == 0 { body_end = Some(i); break; } }
            _ => {}
        }
    }
    let Some(body_end) = body_end else { return Ok(()); };

    let mut demands: Vec<(usize, Capability)> = Vec::new();
    let mut i = body_start + 1;
    while i < body_end {
        match at(i) {
            Kind::Let | Kind::Var => {
                if let Kind::Ident(var) = at(i + 1) {
                    let mut taint = None;
                    let mut unknown = false;
                    if let Some(eq) = ((i + 2)..body_end).find(|&j| *at(j) == Kind::Equal) {
                        for j in (eq + 1)..body_end {
                            match at(j) {
                                Kind::Semi | Kind::RBrace => break,
                                Kind::LBrace => { unknown = true; break; }
                                Kind::Ident(n) => match vars.get(n) {
                                    Some(Some(t)) => {
                                        if taint.is_some() && taint != Some(*t) { unknown = true; }
                                        taint = Some(*t);
                                    }
                                    _ => {}
                                },
                                _ => {}
                            }
                        }
                    } else { unknown = true; }
                    vars.insert(var.clone(), if unknown { None } else { taint });
                }
            }
            Kind::For => {
                if let Kind::Ident(var) = at(i + 1) { vars.insert(var.clone(), None); }
            }
            _ => {}
        }
        // Only an identifier immediately adjacent to the operator is treated as
        // an operand of a type parameter. Anything else (call results, indices,
        // fields, literals) is left to the per-instantiation check.
        if let Some(cap) = Capability::from_op(at(i)) {
            for neighbour in [i.wrapping_sub(1), i + 1] {
                if neighbour < body_start + 1 || neighbour >= body_end { continue; }
                if let Kind::Ident(n) = at(neighbour) {
                    if let Some(Some(t)) = vars.get(n) { demands.push((*t, cap)); }
                }
            }
        }
        i += 1;
    }

    for (index, cap) in demands {
        let declared = &bounds[index];
        if declared.is_empty() { continue; }
        let effective: Vec<Type> = AUDIT_TYPES.iter().copied()
            .filter(|ty| declared.iter().all(|b| b.allows(*ty)))
            .collect();
        let bad: Vec<String> = effective.iter().copied()
            .filter(|ty| !cap.supports(*ty))
            .map(|ty| ty.name())
            .collect();
        if bad.is_empty() { continue; }
        let constraint_text = declared.iter().map(|b| b.name()).collect::<Vec<_>>().join(" + ");
        let effective_text = effective.iter().map(|t| t.name()).collect::<Vec<_>>().join(", ");
        let end = tokens.binary_search_by_key(&declared_at, |token| token.offset)
            .ok().map_or(declared_at, |index| tokens[index].end);
        return Err(ae_syntax::diagnostic_range(file, source, declared_at..end, &format!(
            "generic function '{name}' declares {}: {} but its body applies {} to {}; the constraint still admits {}, which does not support it ({} allows {}; current effective set is {})",
            params[index], constraint_text, cap.symbol(), params[index], bad.join(", "),
            cap.symbol(), cap.supported_types(), effective_text)));
    }
    Ok(())
}

// `fn pick[T](a: T, b: T) -> T { ... }` before any call site picks arguments.
// The declaration keeps its own token slice and source text so an instantiation
// can be parsed later, even from another file (ADR-31). ADR-41: both are shared
// per source file with `Rc`, so N generic declarations in one file do not make
// N full copies of the token stream (an O(N^2) front-end cost).
#[derive(Debug, Clone)]
struct GenericFn { name: String, params: Vec<String>, bounds: Vec<Vec<Bound>>, trait_bounds: Vec<Vec<usize>>, start: usize, tokens: Rc<Vec<Token>>, source: Rc<str>, file: String, public: bool }
// An instantiated generic function: its id, parameter types and return type.
type Instance = (usize, Vec<Type>, Type);
// Instantiated generic enums, structs and functions, keyed by declaration and
// arguments (ADR-21, ADR-30, ADR-31). `next_fn_id` is the one id space shared
// by ordinary and instantiated functions, so C names never collide.
#[derive(Default, Clone)]
struct Generics { defs: Vec<GenericEnum>, names: HashMap<String, usize>, done: HashMap<String, usize>, struct_defs: Vec<GenericStruct>, struct_names: HashMap<String, usize>, struct_done: HashMap<String, usize>, fn_defs: Vec<GenericFn>, fn_names: HashMap<String, usize>, fn_done: HashMap<String, Instance>, fn_in_progress: std::collections::HashSet<String>, next_fn_id: usize, traits: Vec<TraitDef>, trait_names: HashMap<String, usize>, impls: std::collections::HashSet<(usize, String)> }
#[derive(Debug)]
struct MatchArm { variant: usize, bindings: Vec<(usize, Type, usize)>, body: Vec<Statement> }

// A function signature as seen by importers. `file` and `public` let a call
// site reject a name that exists but was not exported (ADR-18).
#[derive(Debug, Clone)]
struct Signature { id: usize, params: Vec<Type>, ret: Type, file: String, public: bool }

type SharedFunctions = HashMap<String, Signature>;

struct Parser<'a> { file: String, source: &'a str, tokens: Rc<Vec<Token>>, shared_source: Rc<str>, pos: usize, names: HashMap<String, (usize, bool, Type)>, binding_ranges: HashMap<usize, std::ops::Range<usize>>, next_var: usize, expression_depth: usize, expression_nodes: usize, block_depth: usize, loop_depth: usize, loop_scopes: Vec<usize>, loop_labels: Vec<Option<usize>>, next_label: usize, functions: &'a mut SharedFunctions, structs: &'a mut Vec<StructDef>, struct_names: &'a mut HashMap<String, usize>, enums: &'a mut Vec<EnumDef>, enum_names: &'a mut HashMap<String, usize>, generics: &'a mut Generics, id_order: Vec<usize>, in_main: bool, return_type: Type, moved: std::collections::HashSet<usize>, borrows: Vec<(usize, usize)>, scopes: Vec<Vec<(usize, Type)>>, subst: HashMap<String, Type>, trait_bounds: HashMap<String, Vec<usize>>, pending: Vec<Function> }
#[derive(Debug, Default)]
struct Program { files: Vec<Vec<Function>>, structs: Vec<StructDef>, enums: Vec<EnumDef> }

fn make_parser<'a>(file: &str, source: &'a str, tokens: Rc<Vec<Token>>, functions: &'a mut SharedFunctions, structs: &'a mut Vec<StructDef>, struct_names: &'a mut HashMap<String, usize>, enums: &'a mut Vec<EnumDef>, enum_names: &'a mut HashMap<String, usize>, generics: &'a mut Generics) -> Parser<'a> {
    Parser { file: file.to_owned(), source, tokens, shared_source: Rc::from(source), pos: 0, names: HashMap::new(), binding_ranges: HashMap::new(), next_var: 0, expression_depth: 0, expression_nodes: 0, block_depth: 0, loop_depth: 0, loop_scopes: Vec::new(), loop_labels: Vec::new(), next_label: 0, functions, structs, struct_names, enums, enum_names, generics, id_order: Vec::new(), in_main: false, return_type: Type::Int, moved: std::collections::HashSet::new(), borrows: Vec::new(), scopes: Vec::new(), subst: HashMap::new(), trait_bounds: HashMap::new(), pending: Vec::new() }
}

// Leading `use "path";` headers. Parsed from tokens before the Parser owns
// the shared function table, so imports can be loaded depth-first.
fn scan_uses(file: &str, source: &str, tokens: &[Token]) -> Result<Vec<String>, ae_span::Diagnostic> {
    let mut uses = Vec::new();
    let mut pos = 0;
    while matches!(tokens.get(pos).map(|t| &t.kind), Some(Kind::Use)) {
        pos += 1;
        match tokens.get(pos).map(|t| t.kind.clone()) {
            Some(Kind::Text(text)) => { uses.push(text); pos += 1; }
            _ => {
                let range = tokens.get(pos).map(|t| t.offset..t.end).unwrap_or(0..0);
                return Err(ae_syntax::diagnostic_range(file, source, range, "expected a string path after 'use'"));
            }
        }
        match tokens.get(pos).map(|t| &t.kind) {
            Some(Kind::Semi) => pos += 1,
            _ => {
                let range = tokens.get(pos).map(|t| t.offset..t.end).unwrap_or(0..0);
                return Err(ae_syntax::diagnostic_range(file, source, range, "expected ';' after 'use' path"));
            }
        }
    }
    Ok(uses)
}

#[allow(dead_code)] // exercised by in-memory unit tests
fn parse_source(file: &str, source: &str, shared: &mut SharedFunctions, structs: &mut Vec<StructDef>, struct_names: &mut HashMap<String, usize>, enums: &mut Vec<EnumDef>, enum_names: &mut HashMap<String, usize>, generics: &mut Generics, next_id: &mut usize) -> Result<Vec<Function>, String> {
    let tokens = Rc::new(lex(source)?);
    let uses = scan_uses(file, source, tokens.as_ref()).map_err(|error| error.to_string())?;
    if !uses.is_empty() {
        return Err(error(file, source, 0, "'use' is only supported when compiling a source file"));
    }
    let mut p = make_parser(file, source, tokens, shared, structs, struct_names, enums, enum_names, generics);
    p.collect_functions(next_id).map_err(|e| e.to_string())?;
    let functions = p.program().map_err(|e| e.to_string())?;
    if functions.iter().filter(|f| f.is_main).count() != 1 {
        return Err(error(file, source, 0, "missing main function"));
    }
    Ok(functions)
}

/// One source file in the import graph. `raw` matches the `file` field stored
/// on declarations; `label` is a platform-independent name for reports.
struct ModuleInfo { raw: String, label: String, uses: Vec<String> }

/// Source diagnostics retain structured locations; loader and other errors
/// that have no source location still use the legacy text representation.
pub use ae_span::Diagnostic as FrontendDiagnostic;
#[derive(Debug)]
pub enum FrontendError {
    Diagnostic(ae_span::Diagnostic),
    Diagnostics(Vec<ae_span::Diagnostic>),
    LexDiagnostics(Vec<ae_span::Diagnostic>),
    Legacy(String),
}
impl From<String> for FrontendError {
    fn from(value: String) -> Self { Self::Legacy(value) }
}
impl From<FrontendError> for String {
    fn from(value: FrontendError) -> Self {
        match value {
            FrontendError::Diagnostic(d) => d.to_string(),
            FrontendError::Diagnostics(ds) | FrontendError::LexDiagnostics(ds) => ds.into_iter().next().map_or_else(|| "compilation error".to_owned(), |d| d.to_string()),
            FrontendError::Legacy(s) => s,
        }
    }
}

fn module_label(path: &Path, root: &Path) -> String {
    let rel = path.strip_prefix(root).unwrap_or(path);
    rel.components().map(|part| part.as_os_str().to_string_lossy().into_owned()).collect::<Vec<_>>().join("/")
}

#[derive(Clone, Copy)]
enum ModuleState { Loading, Loaded }

struct RecoveryState {
    functions: SharedFunctions,
    structs: Vec<StructDef>,
    struct_names: HashMap<String, usize>,
    enums: Vec<EnumDef>,
    enum_names: HashMap<String, usize>,
    generics: Generics,
    ids: Vec<usize>,
}
impl RecoveryState {
    fn capture(parser: &Parser<'_>) -> Self {
        Self { functions: parser.functions.clone(), structs: parser.structs.clone(),
            struct_names: parser.struct_names.clone(), enums: parser.enums.clone(),
            enum_names: parser.enum_names.clone(), generics: parser.generics.clone(), ids: parser.id_order.clone() }
    }

    fn check_bodies(&self, file: &str, source: &str, tokens: Rc<Vec<Token>>) -> Vec<ae_span::Diagnostic> {
        let mut result = Vec::new();
        let (mut depth, mut id_index) = (0usize, 0usize);
        let (mut pending_impl, mut impl_depth) = (false, None);
        for (index, token) in tokens.iter().enumerate() {
            match &token.kind {
                Kind::Impl if depth == 0 => pending_impl = true,
                Kind::LBrace => {
                    depth += 1;
                    if pending_impl { impl_depth = Some(depth); pending_impl = false; }
                }
                Kind::RBrace => {
                    if impl_depth == Some(depth) { impl_depth = None; }
                    depth = depth.saturating_sub(1);
                }
                Kind::Fn if depth == 0 || impl_depth == Some(depth) => {
                    let Some(Token { kind: Kind::Ident(name), .. }) = tokens.get(index + 1) else { return Vec::new() };
                    if matches!(tokens.get(index + 2).map(|t| &t.kind), Some(Kind::LBracket)) { continue; }
                    if !matches!(tokens.get(index + 2).map(|t| &t.kind), Some(Kind::LParen)) { return Vec::new(); }
                    let Some(&id) = self.ids.get(id_index) else { return Vec::new() };
                    id_index += 1;
                    let (mut functions, mut structs, mut struct_names, mut enums, mut enum_names, mut generics) =
                        (self.functions.clone(), self.structs.clone(), self.struct_names.clone(), self.enums.clone(), self.enum_names.clone(), self.generics.clone());
                    let mut parser = make_parser(file, source, Rc::clone(&tokens), &mut functions, &mut structs, &mut struct_names, &mut enums, &mut enum_names, &mut generics);
                    parser.pos = index + 2;
                    if let Err(diagnostic) = parser.function_rest(depth == 0 && name == "main", id) {
                        if !result.contains(&diagnostic) { result.push(diagnostic); }
                        if result.len() == 32 { break; }
                    }
                }
                _ => {}
            }
        }
        if id_index != self.ids.len() && result.len() < 32 { return Vec::new(); }
        result
    }
}

struct LoadingFile {
    canonical: std::path::PathBuf,
    path: std::path::PathBuf,
    source: String,
    file: String,
    tokens: Rc<Vec<Token>>,
    uses: Vec<String>,
    next_import: usize,
}

fn begin_file(path: &Path, visited: &mut HashMap<std::path::PathBuf, ModuleState>) -> Result<Option<LoadingFile>, FrontendError> {
    let canonical = std::fs::canonicalize(path).map_err(|e| format!("cannot open {}: {e}", path.display()))?;
    match visited.get(&canonical) {
        Some(ModuleState::Loading) => return Err(format!("circular import of {}", path.display()).into()),
        Some(ModuleState::Loaded) => return Ok(None),
        None => { visited.insert(canonical.clone(), ModuleState::Loading); }
    }
    let source = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let file = path.display().to_string();
    let tokens = Rc::new(ae_syntax::lex_with_diagnostics(&source).map_err(|mut diagnostics| {
        for diagnostic in &mut diagnostics {
            if let Some(label) = &mut diagnostic.primary { label.span.file = Some(file.clone()); }
        }
        FrontendError::LexDiagnostics(diagnostics)
    })?);
    let uses = scan_uses(&file, &source, tokens.as_ref()).map_err(FrontendError::Diagnostic)?;
    Ok(Some(LoadingFile { canonical, path: path.to_owned(), source, file, tokens, uses, next_import: 0 }))
}

#[allow(clippy::too_many_arguments)]
fn load_file(path: &Path, root: &Path, visited: &mut HashMap<std::path::PathBuf, ModuleState>, shared: &mut SharedFunctions, structs: &mut Vec<StructDef>, struct_names: &mut HashMap<String, usize>, enums: &mut Vec<EnumDef>, enum_names: &mut HashMap<String, usize>, generics: &mut Generics, next_id: &mut usize, out: &mut Vec<Vec<Function>>, mains: &mut usize, modules: &mut Vec<ModuleInfo>, recover: bool) -> Result<(), FrontendError> {
    let Some(first) = begin_file(path, visited)? else { return Ok(()) };
    let mut stack = vec![first];
    while let Some(frame) = stack.last_mut() {
        if let Some(import) = frame.uses.get(frame.next_import) {
            frame.next_import += 1;
            let parent = frame.path.parent().unwrap_or(Path::new("."));
            let child = parent.join(import);
            if let Some(next) = begin_file(&child, visited)? { stack.push(next); }
            continue;
        }
        let frame = stack.pop().expect("stack is nonempty");
        let mut parser = make_parser(&frame.file, &frame.source, Rc::clone(&frame.tokens), shared, structs, struct_names, enums, enum_names, generics);
        parser.skip_uses();
        parser.collect_functions(next_id).map_err(FrontendError::Diagnostic)?;
        let snapshot = recover.then(|| RecoveryState::capture(&parser));
        let functions = match parser.program() {
            Ok(functions) => functions,
            Err(first) => {
                if let Some(snapshot) = snapshot {
                    let diagnostics = snapshot.check_bodies(&frame.file, &frame.source, Rc::clone(&frame.tokens));
                    if diagnostics.first() == Some(&first) && diagnostics.len() > 1 {
                        return Err(FrontendError::Diagnostics(diagnostics));
                    }
                }
                return Err(FrontendError::Diagnostic(first));
            }
        };
        *mains += functions.iter().filter(|f| f.is_main).count();
        out.push(functions);
        modules.push(ModuleInfo {
            raw: frame.file,
            label: module_label(&frame.path, root),
            uses: frame.uses.iter().map(|u| u.replace('\\', "/")).collect(),
        });
        visited.insert(frame.canonical, ModuleState::Loaded);
    }
    Ok(())
}

struct Loaded {
    program: Program,
    shared: SharedFunctions,
    generics: Generics,
    modules: Vec<ModuleInfo>,
}

fn load_program_full(entry: &Path, recover: bool) -> Result<Loaded, FrontendError> {
    let root = entry.parent().filter(|p| !p.as_os_str().is_empty()).unwrap_or(Path::new("."));
    let mut visited = HashMap::new();
    let mut shared: SharedFunctions = HashMap::new();
    let mut structs: Vec<StructDef> = Vec::new();
    let mut struct_names: HashMap<String, usize> = HashMap::new();
    // Prelude result types are registered first, so their ids are stable.
    let (mut enums, mut enum_names) = prelude_enums();
    let mut generics = prelude_generics();
    let mut next_id = 0usize;
    let mut files = Vec::new();
    let mut mains = 0usize;
    let mut modules = Vec::new();
    load_file(entry, root, &mut visited, &mut shared, &mut structs, &mut struct_names, &mut enums, &mut enum_names, &mut generics, &mut next_id, &mut files, &mut mains, &mut modules, recover)?;
    if mains != 1 { return Err(format!("program must define exactly one main function (found {mains})").into()); }
    Ok(Loaded { program: Program { files, structs, enums }, shared, generics, modules })
}

fn load_program(entry: &Path) -> Result<Program, String> {
    load_program_full(entry, false).map(|loaded| loaded.program).map_err(Into::into)
}

pub fn check_path(path: &Path) -> Result<(), String> {
    load_program(path).map(|_| ())
}

// Read-only supplemental traversal after an initial lexical error. A failed
// file cannot expose imports, but sibling branches can still be scanned.
fn collect_import_lex_errors(entry: &Path) -> Vec<ae_span::Diagnostic> {
    const LIMIT: usize = 32;
    let mut visited = HashMap::new();
    let mut diagnostics = Vec::new();
    let mut stack = match begin_file(entry, &mut visited) {
        Ok(Some(frame)) => vec![frame],
        Err(FrontendError::LexDiagnostics(errors)) => return errors,
        _ => return diagnostics,
    };
    while let Some(frame) = stack.last_mut() {
        if let Some(import) = frame.uses.get(frame.next_import) {
            frame.next_import += 1;
            let child = frame.path.parent().unwrap_or(Path::new(".")).join(import);
            match begin_file(&child, &mut visited) {
                Ok(Some(next)) => stack.push(next),
                Ok(None) => {},
                Err(FrontendError::LexDiagnostics(errors)) => {
                    diagnostics.extend(errors.into_iter().take(LIMIT - diagnostics.len()));
                    if diagnostics.len() == LIMIT { break; }
                    // Suppress repeat reports for aliases of the same failed file.
                    if let Ok(canonical) = std::fs::canonicalize(&child) {
                        visited.insert(canonical, ModuleState::Loaded);
                    } else { break; }
                }
                Err(_) => break,
            }
        } else {
            let frame = stack.pop().expect("stack is nonempty");
            visited.insert(frame.canonical, ModuleState::Loaded);
        }
    }
    diagnostics
}

pub fn check_path_diagnostic(path: &Path) -> Result<(), FrontendError> {
    match load_program_full(path, true) {
        Err(FrontendError::LexDiagnostics(first)) => {
            let recovered = collect_import_lex_errors(path);
            if recovered.len() > first.len() && recovered.starts_with(&first) {
                Err(FrontendError::LexDiagnostics(recovered))
            } else {
                Err(FrontendError::LexDiagnostics(first))
            }
        }
        other => other.map(|_| ()),
    }
}

/// A stable, platform-independent description of the import graph and every
/// module's public interface. Function bodies, comments and local variables
/// are deliberately absent: changing one must not change any interface hash.
/// This is a diagnosis and planning tool for package-level incrementality; it
/// does not yet participate in a cache decision, because the backend still
/// emits one translation unit per program.
pub fn describe_path(entry: &Path) -> Result<String, String> {
    let loaded = load_program_full(entry, false)?;
    let Loaded { program, shared, generics, modules } = loaded;
    let Program { structs, enums, .. } = program;
    let mut lines = Vec::new();
    let mut all = Vec::new();
    for module in &modules {
        let mut items: Vec<String> = Vec::new();
        for (name, sig) in &shared {
            if sig.file != module.raw || !sig.public { continue; }
            let params = sig.params.iter().map(|t| t.name()).collect::<Vec<_>>().join(", ");
            items.push(format!("fn {name}({params}) -> {}", sig.ret.name()));
        }
        for def in &structs {
            if def.file != module.raw || !def.public { continue; }
            let fields = def.fields.iter().map(|(n, t)| format!("{n}: {}", t.name())).collect::<Vec<_>>().join(", ");
            items.push(format!("struct {} {{ {fields} }}", def.name));
        }
        for def in &enums {
            if def.file != module.raw || !def.public { continue; }
            let variants = def.variants.iter().map(|(name, fields)| {
                let payload = fields.iter().map(|t| t.name()).collect::<Vec<_>>().join(", ");
                format!("{name}({payload})")
            }).collect::<Vec<_>>().join(", ");
            items.push(format!("enum {} {{ {variants} }}", def.name));
        }
        for def in &generics.fn_defs {
            if def.file != module.raw || !def.public { continue; }
            let params = def.params.iter().zip(&def.bounds)
                .map(|(n, bounds)| format!("{n}: {}", bounds.iter().map(|b| b.name()).collect::<Vec<_>>().join(" + ")))
                .collect::<Vec<_>>().join(", ");
            items.push(format!("generic fn {}[{params}]", def.name));
        }
        for def in &generics.defs {
            if def.file != module.raw || !def.public { continue; }
            items.push(format!("generic enum {}[{}]", def.name, def.params.join(", ")));
        }
        for def in &generics.struct_defs {
            if def.file != module.raw || !def.public { continue; }
            items.push(format!("generic struct {}[{}]", def.name, def.params.join(", ")));
        }
        items.sort();
        let digest = crate::sha256::hex(items.join("\n").as_bytes());
        let short = digest[..16].to_owned();
        all.push(format!("{} {short}", module.label));
        lines.push(format!("module {}", module.label));
        lines.push(format!("  interface {short}"));
        lines.push(format!("  exports {}", items.len()));
        for used in &module.uses { lines.push(format!("  uses {used}")); }
    }
    all.sort();
    lines.push(format!("fingerprint {}", crate::sha256::hex(all.join("\n").as_bytes())[..16].to_owned()));
    Ok(lines.join("\n") + "\n")
}
#[allow(dead_code)] // exercised by in-memory unit tests
pub fn check(source: &str) -> Result<(), String> {
    let mut shared: SharedFunctions = HashMap::new();
    let mut structs: Vec<StructDef> = Vec::new();
    let mut struct_names: HashMap<String, usize> = HashMap::new();
    // Prelude result types are registered first, so their ids are stable.
    let (mut enums, mut enum_names) = prelude_enums();
    let mut generics = prelude_generics();
    let mut next_id = 0usize;
    parse_source("", source, &mut shared, &mut structs, &mut struct_names, &mut enums, &mut enum_names, &mut generics, &mut next_id).map(|_| ())
}
#[allow(dead_code)] // exercised by in-memory unit tests
pub fn compile(source: &str) -> Result<String, String> {
    let mut shared: SharedFunctions = HashMap::new();
    let mut structs: Vec<StructDef> = Vec::new();
    let mut struct_names: HashMap<String, usize> = HashMap::new();
    // Prelude result types are registered first, so their ids are stable.
    let (mut enums, mut enum_names) = prelude_enums();
    let mut generics = prelude_generics();
    let mut next_id = 0usize;
    let functions = parse_source("", source, &mut shared, &mut structs, &mut struct_names, &mut enums, &mut enum_names, &mut generics, &mut next_id)?;
    emit_program(Program { files: vec![functions], structs, enums }).map(|g| g.full)
}
pub fn compile_path(path: &Path) -> Result<String, String> {
    compile_path_parts(path).map(|(runtime, user)| runtime + &user)
}
/// The build driver's incremental view: `(runtime_prefix, program_suffix)`.
/// Concatenating the two reproduces `compile_path` exactly, so `emit-c` stays
/// the authority for what a program means.
pub fn compile_path_parts(path: &Path) -> Result<(String, String), String> {
    let generated = emit_program(load_program(path)?)?;
    let GeneratedC { full, runtime_len } = generated;
    let user = full[runtime_len..].to_owned();
    let mut runtime = full;
    runtime.truncate(runtime_len);
    Ok((runtime, user))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test] fn rejects_invalid_semantics() {
        for s in ["fn main() { let x = 1; x = 2; }", "fn main() { print(x); }", "fn main() { let x = x; }", "fn main() { var x = 1; let x = 2; }", "fn main() { print(9223372036854775808); }", "fn main() { print(1) }", "fn main() {} junk"] { assert!(compile(s).is_err(), "{s}"); }
    }
    #[test] fn rejects_type_errors() {
        for s in [
            "fn main() { if 1 { print(1); } }",
            "fn main() { while 1 { break; } }",
            "fn main() { print(true + 1); }",
            "fn main() { if true < false { print(1); } }",
            "fn main() { if 1 < true { print(1); } }",
            "fn main() { let x = 1; x = true; }",
            "fn main() { let b = true; b = 1; }",
            "fn main() { print(!1); }",
            "fn main() { print(-true); }",
            "fn main() { assert(1); }",
            "fn main() { let a = [true]; }",
            "fn main() { let x = 1 && true; }",
            "fn main() { if 1 == 1 && 2 { print(1); } }",
        ] { assert!(compile(s).is_err(), "{s}"); }
    }
    #[test] fn accepts_typed_programs() {
        for s in [
            "fn main() { let flag = true; if flag && !false { print(true); } }",
            "fn main() { print(true == true); print(false != true); }",
            "fn is_even(n: i64) -> bool { return n % 2 == 0; } fn main() { print(is_even(4) || is_even(3)); }",
            "fn pick(flag: bool, a: i64, b: i64) -> i64 { if flag { return a; } else { return b; } } fn main() { print(pick(1 < 2, 3, 4)); }",
            "fn main() { print(\"hello, 世界\"); print(true); print(1 < 2 && 2 < 3); assert(1 == 1); }",
        ] { assert!(compile(s).is_ok(), "{s}"); }
    }
    #[test] fn rejects_invalid_string_literals() {
        for s in ["fn main() { print(\"bad \\q\"); }", "fn main() { print(\"open }", "fn main() { print(\"\n\"); }"] { assert!(compile(s).is_err(), "{s}"); }
    }
    #[test] fn diagnostics_include_unicode_aware_position() {
        let e = compile("// 中文\nfn main() { print(@); }").unwrap_err();
        assert!(e.starts_with("2:19:"), "{e}");
    }
    #[test] fn malformed_inputs_do_not_panic() {
        // Deterministic corpus of arbitrary UTF-8 tokens and punctuation.
        let alphabet = ["a", "0", " ", "\n", "{", "}", "[", "]", "(", ")", ";", "+", "=", "/", "\"", "\\", "中", "🙂", "\0"];
        let mut state = 0x953c_16a7_4d82_13efu64;
        for _ in 0..5000 {
            let mut text = String::new();
            state ^= state << 13; state ^= state >> 7; state ^= state << 17;
            let len = (state as usize) % 150;
            for _ in 0..len {
                state ^= state << 13; state ^= state >> 7; state ^= state << 17;
                text.push_str(alphabet[(state as usize) % alphabet.len()]);
            }
            let result = std::panic::catch_unwind(|| compile(&text));
            assert!(result.is_ok(), "compiler panicked on {text:?}");
        }
    }
    #[test] fn token_positions_handle_unicode_and_newlines() {
        let tokens = lex("// 😀\nfn main() {\n print(1 + 2);\n}").unwrap();
        let plus = tokens.iter().find(|t| t.kind == Kind::Plus).unwrap();
        assert_eq!((plus.line, plus.column), (3, 10));
        let eof = tokens.last().unwrap();
        assert_eq!((eof.line, eof.column), (4, 2));
    }
    #[test] fn arithmetic_and_variables_compile() {
        assert!(compile("fn main() { let x = 2; var y = x + 3 * 4; y = y / 2; print(y); }").is_ok());
    }
    #[test]
    fn runtime_units_attach_leading_comments_to_the_function() {
        // A `//` comment (and the blank lines above it) belongs to the function
        // below, so pruning that function also drops its explanation instead of
        // leaving it in every generated program.
        let units = runtime_units("// explanation\n\nstatic void ae_x(void) { }\n");
        let function = units.iter().find(|(name, _)| name.is_some()).expect("function unit");
        assert!(function.1.starts_with("// explanation"), "{}", function.1);
        // A comment above code that is not a function stays in the always-kept
        // text, otherwise the file header would vanish with the first pruned
        // function.
        let units = runtime_units("// header\n#include <stdio.h>\n");
        assert!(units.iter().all(|(name, _)| name.is_none()));
        assert!(units[0].1.contains("// header"));
        // A multi-line block comment is not moved into a function unit.
        let units = runtime_units("/* banner\n   here */\nstatic void ae_y(void) { }\n");
        let function = units.iter().find(|(name, _)| name.is_some()).expect("function unit");
        assert!(!function.1.contains("banner"), "{}", function.1);
    }

    #[test] fn prune_runtime_drops_unneeded_windows_header() {
        let prefix = concat!(
            "#ifdef _WIN32\n",
            "#ifndef WIN32_LEAN_AND_MEAN\n",
            "#define WIN32_LEAN_AND_MEAN\n",
            "#endif\n",
            "#include <windows.h>\n",
            "#endif\n",
            "static void ae_plain(void) { }\n",
            "static void ae_win(void) { /* win32-header */ }\n",
        );
        let (plain, len) = prune_runtime(prefix, "", &[], "int main(void) { ae_plain(); return 0; }\n");
        assert!(plain[..len].contains(WIN32_HEADER_OMITTED), "header should be dropped");
        assert!(!plain[..len].contains(WIN32_HEADER_INCLUDE), "include line must be gone");
        assert!(!plain.contains("ae_win"), "unreachable Win32 function must be pruned");

        let (win, len) = prune_runtime(prefix, "", &[], "int main(void) { ae_win(); return 0; }\n");
        assert!(win[..len].contains(WIN32_HEADER_INCLUDE), "header must stay for Win32 functions");
        assert!(win[..len].contains(WIN32_HEADER_MARKER));
    }
    #[test] fn every_win32_dependent_runtime_function_is_marked() {
        // Audit only: this token list does not drive the build. A future Win32
        // helper added without the marker either keeps the header (harmless) or
        // fails to compile with a missing declaration, which the generated-C
        // suite catches. A missing marker can never change runtime behaviour.
        const WIN32_DECLS: &[&str] = &[
            "wchar_t", "MultiByteToWideChar", "WideCharToMultiByte", "CP_UTF8",
            "FILETIME", "GetSystemTimeAsFileTime", "GetEnvironmentVariableW",
            "GetCurrentDirectoryW", "GetFileAttributesW", "WIN32_FIND_DATAW",
            "WIN32_FILE_ATTRIBUTE_DATA", "FindFirstFileW", "CreateProcessW",
            "STARTUPINFOW", "GetLastError", "RemoveDirectoryW", "DeleteFileW",
            "CreateDirectoryW", "MoveFileExW", "SetCurrentDirectoryW",
            "GetStdHandle", "GetConsoleMode", "WriteConsoleW",
        ];
        let sources = [("runtime.c", include_str!("../runtime.c")), ("runtime_ex.c", include_str!("../runtime_ex.c"))];
        assert!(sources[0].1.contains(WIN32_HEADER_INCLUDE), "include line must still exist in runtime.c");
        let mut checked = 0usize;
        let mut declared_users = 0usize;
        for (file, source) in sources {
            for (name, text) in runtime_units(source) {
                let Some(name) = name else { continue; };
                if !WIN32_DECLS.iter().any(|needle| text.contains(needle)) { continue; }
                checked += 1;
                // Either the header is kept for this function, or the runtime
                // declares the entry point itself -- and then the declaration has
                // to be there, or the program builds without a prototype.
                assert!(text.contains(WIN32_HEADER_MARKER) || text.contains(WIN32_DECLARED_MARKER),
                    "{file}: {name} uses a Win32 declaration but carries neither marker");
                if text.contains(WIN32_DECLARED_MARKER) && !text.contains(WIN32_HEADER_MARKER) {
                    declared_users += 1;
                }
            }
        }
        assert!(checked >= 16, "expected at least 16 Win32-dependent functions, found {checked}");
        assert!(declared_users >= 3, "expected the declared-marker functions to exist, found {declared_users}");
        // The other direction: every symbol the runtime declares for itself is
        // audited above, so a hand-written declaration cannot slip in unmarked.
        // The declarations live inside the functions that call them (top-level
        // text is always kept, so a program that never prints would carry them),
        // which is why this scan trims the indentation.
        let mut declared = 0usize;
        for line in sources[0].1.lines().map(|line| line.trim_start()) {
            if !line.starts_with("extern ") { continue; }
            let name = line[..line.find('(').expect("declaration has a parameter list")]
                .rsplit(' ').next().unwrap().trim_start_matches('*');
            assert!(WIN32_DECLS.contains(&name), "self-declared Win32 symbol not in the audit list: {name}");
            declared += 1;
        }
        assert_eq!(declared, 4, "expected four self-declared Win32 entry points");
    }

    #[test]
    fn capability_tables_match_the_real_operator_rules() {
        // ADR-40: a constraint whitelist must be a subset of the types that
        // actually support the operation, or the declaration would promise
        // something the per-instantiation type checker cannot deliver.
        for &ty in AUDIT_TYPES {
            if Bound::Add.allows(ty) { assert!(Capability::Add.supports(ty), "Add admits {}", ty.name()); }
            if Bound::Ord.allows(ty) { assert!(Capability::Ord.supports(ty), "Ord admits {}", ty.name()); }
            if Bound::Eq.allows(ty) { assert!(Capability::Eq.supports(ty), "Eq admits {}", ty.name()); }
        }
    }

    #[test]
    fn capability_audit_agrees_with_per_instantiation_checks() {
        // Differential test: with no constraint the audit steps aside and the
        // per-instantiation check decides. bool + bool must still fail while
        // i64, f64 and string pass.
        let body = "fn f[T](a: T, b: T) -> T { return a + b; }";
        for (ty, args, expected) in [
            ("i64", "1, 2", true), ("f64", "1.0, 2.0", true),
            ("string", "\"x\", \"y\"", true), ("bool", "true, false", false),
        ] {
            let source = format!("{body} fn main() {{ print(f[{ty}]({args})); }}");
            assert_eq!(compile(&source).is_ok(), expected, "{ty}: {source}");
        }
        // Copy still admits bool, so the declaration is rejected even when the
        // function is never instantiated: the error moves to the declaration.
        let weak = "fn f[T: Copy](a: T, b: T) -> T { return a + b; } fn main() { print(1); }";
        let err = compile(weak).unwrap_err();
        assert!(err.contains("declares T: Copy") && err.contains("bool"), "{err}");
        assert!(err.contains("1:5:"), "diagnostic must point at the constraint: {err}");
        // Narrowing the effective set to the supported types fixes it.
        assert!(compile("fn f[T: Add](a: T, b: T) -> T { return a + b; } fn main() { print(f[i64](1, 2)); }").is_ok());
        assert!(compile("fn f[T: Ord](a: T, b: T) -> T { return a + b; } fn main() { print(f[f64](1.0, 2.0)); }").is_ok());
    }

    #[test]
    fn capability_audit_rejects_weak_constraints_without_false_positives() {
        for source in [
            "fn f[T: Add](a: T, b: T) -> T { return a - b; } fn main() { print(1); }",
            "fn f[T: Add](a: T, b: T) -> T { return a % b; } fn main() { print(1); }",
            "fn f[T: Eq](a: T, b: T) -> bool { return a && b; } fn main() { print(1); }",
            "fn f[T: Copy](a: T, b: T) -> bool { return a < b; } fn main() { print(1); }",
            "fn f[T: Copy](a: T, b: T) -> T { let y = a; return y + b; } fn main() { print(1); }",
        ] {
            assert!(compile(source).is_err(), "weak constraint accepted: {source}");
        }
        for source in [
            "fn f[T: Add](a: T, b: T) -> T { return a + b; } fn main() { print(f[i64](1, 2)); }",
            "fn f[T: Ord](a: T, b: T) -> T { let t = a + b; if t > a { return t; } return b; } fn main() { print(f[i64](1, 2)); }",
            "fn f[T: Eq](a: T, b: T) -> bool { return a == b; } fn main() { print(f[bool](true, false)); }",
            "fn f[T: Copy](a: T) -> T { return a; } fn main() { print(f[i64](1)); }",
            // An operator on a concrete-typed variable is never attributed to T.
            "fn f[T: Eq](a: T, n: i64) -> i64 { return n + 1; } fn main() { print(f[i64](1, 2)); }",
            // An unconstrained generic keeps ADR-33 behaviour.
            "fn f[T](a: T, b: T) -> T { return a + b; } fn main() { print(f[i64](1, 2)); }",
        ] {
            if let Err(err) = compile(source) { panic!("valid declaration rejected: {source}: {err}"); }
        }
    }

    #[test]
    fn generic_declarations_share_one_token_buffer_per_file() {
        // ADR-41: before this change every declaration cloned the whole token
        // stream and source text, which is O(N^2) in the number of declarations.
        let mut shared = HashMap::new();
        let mut structs = Vec::new();
        let mut struct_names = HashMap::new();
        let (mut enums, mut enum_names) = prelude_enums();
        let mut generics = prelude_generics();
        let mut next_id = 0usize;
        let source = "fn f[T: Add](a: T, b: T) -> T { return a + b; }\n\
fn g[U: Ord](a: U, b: U) -> U { return a + b; }\n\
fn h[V: Eq](a: V, b: V) -> bool { return a == b; }\n\
fn main() { print(f[i64](1, 2)); print(g[i64](3, 4)); print(h[bool](true, true)); }";
        parse_source("", source, &mut shared, &mut structs, &mut struct_names,
            &mut enums, &mut enum_names, &mut generics, &mut next_id).unwrap();
        assert_eq!(generics.fn_defs.len(), 3, "expected three generic declarations");
        let tokens = Rc::clone(&generics.fn_defs[0].tokens);
        let text = Rc::clone(&generics.fn_defs[0].source);
        for def in &generics.fn_defs[1..] {
            assert!(Rc::ptr_eq(&tokens, &def.tokens), "each declaration copied the token stream");
            assert!(Rc::ptr_eq(&text, &def.source), "each declaration copied the source text");
        }
        assert_eq!(&*text, source);
    }
}
