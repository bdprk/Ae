use std::collections::{HashMap, HashSet};
use std::path::Path;

#[derive(Debug, Clone, PartialEq)]
enum Kind {
    Ident(String), Number(i64), Float(f64), Text(String), Fn, Let, Var, Print, If, Else, While, For, In, Return, Break, Continue, Assert, True, False, Use,
    EqEq, NotEq, Less, LessEq, Greater, GreaterEq, And, Or, Bang,
    LParen, RParen, LBrace, RBrace, LBracket, RBracket, Comma, Colon, Arrow, Amp, Semi, Equal, Plus, Minus, Star, Slash, Percent, Dot, DotDot, Struct,
    Enum, Match, ColonColon, FatArrow, Question, Eof,
}
#[derive(Debug, Clone)]
struct Token { kind: Kind, offset: usize, line: usize, column: usize }

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Type { Int, Bool, F64, String, StrView, Vec, StrVec, VecRef, StrVecRef, Map, Array(usize), Struct(usize), Enum(&'static str, usize) }
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
            Type::VecRef => "&vec".to_owned(),
            Type::StrVecRef => "&vec[string]".to_owned(),
            // One map shape for now: string keys, i64 values.
            Type::Map => "map[string, i64]".to_owned(),
            Type::Array(n) => format!("&[i64; {n}]"),
            Type::Struct(id) => format!("struct #{}", id),
            Type::Enum(name, _) => name.to_owned(),
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
            Type::Map => "ae_map ".to_owned(),
            Type::Struct(k) => format!("ae_t{k} "),
            Type::Enum(_, k) => format!("ae_e{k} "),
            _ => "int64_t ".to_owned(),
        }
    }
    // Owned types carry heap resources: they move, and every live slot is
    // released on scope exit and control-flow jumps. Enums own their payload
    // slots, which may hold strings.
    fn owned(self) -> bool {
        matches!(self, Type::String | Type::Vec | Type::StrVec | Type::Map | Type::Struct(_) | Type::Enum(..))
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
const RESERVED: &[&str] = &["len", "push", "pop", "vec", "read_file", "write_file", "bytes", "contains", "slice", "split", "args", "join", "slice_view", "lines", "count_lines_with", "try_read_file", "try_write_file", "parse_int", "to_string", "try_list_dir", "path_exists", "path_is_dir", "path_join", "path_name", "path_parent", "try_file_info", "try_copy_file", "try_rename_path", "try_remove_file", "try_create_dir", "try_remove_dir", "map_new", "map_put", "map_get", "map_has", "map_remove", "keys", "utf8_len", "utf8_prefix", "pub", "trim", "starts_with", "ends_with", "find", "sort", "reverse", "now_ms", "now_s", "env", "try_append_file", "exit", "read_stdin", "cwd", "json_get", "json_int", "json_bool", "json_len", "json_has", "strvec_new", "run_command", "set_cwd", "to_f64", "to_i64"];
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

fn location(source: &str, offset: usize) -> (usize, usize) {
    let prefix = &source[..offset];
    (prefix.bytes().filter(|b| *b == b'\n').count() + 1,
     prefix.rsplit('\n').next().unwrap_or("").chars().count() + 1)
}
// Lexer diagnostics carry no file name; load_file adds the file prefix.
fn lex_error(source: &str, offset: usize, message: &str) -> String {
    let (line, column) = location(source, offset);
    format!("{line}:{column}: {message}")
}
fn error(file: &str, source: &str, offset: usize, message: &str) -> String {
    let (line, column) = location(source, offset);
    // In-memory sources carry no file name; keep those diagnostics compact.
    if file.is_empty() { format!("{line}:{column}: {message}") } else { format!("{file}:{line}:{column}: {message}") }
}
fn lex(source: &str) -> Result<Vec<Token>, String> {
    let bytes = source.as_bytes();
    let mut i = 0;
    let mut result = Vec::new();
    let (mut location_offset, mut line, mut column) = (0, 1, 1);
    while i < bytes.len() {
        let start = i;
        let c = source[i..].chars().next().unwrap();
        if c.is_whitespace() { i += c.len_utf8(); continue; }
        if bytes[i..].starts_with(b"//") {
            i += 2;
            while i < bytes.len() && bytes[i] != b'\n' { i += 1; }
            continue;
        }
        let kind = if c == '"' {
            i += 1;
            while i < bytes.len() {
                match bytes[i] {
                    b'"' => break,
                    b'\n' | b'\0' => return Err(lex_error(source, i, "string literals must not contain raw newlines or NUL; use escapes")),
                    b'\\' => {
                        let escaped = bytes.get(i + 1).copied();
                        if !matches!(escaped, Some(b'n' | b't' | b'r' | b'\\' | b'"')) {
                            return Err(lex_error(source, i, "unsupported string escape; allowed: \\n \\t \\r \\\\ \\\""));
                        }
                        i += 2;
                    }
                    _ => i += 1,
                }
            }
            if i >= bytes.len() { return Err(lex_error(source, start, "unterminated string literal")); }
            i += 1;
            Kind::Text(source[start + 1..i - 1].to_owned())
        } else if c.is_ascii_alphabetic() || c == '_' {
            i += 1;
            while i < bytes.len() && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_') { i += 1; }
            match &source[start..i] {
                "fn" => Kind::Fn, "let" => Kind::Let, "var" => Kind::Var, "print" => Kind::Print,
                "if" => Kind::If, "else" => Kind::Else, "while" => Kind::While, "return" => Kind::Return,
                "for" => Kind::For, "in" => Kind::In,
                "break" => Kind::Break, "continue" => Kind::Continue, "assert" => Kind::Assert,
                "true" => Kind::True, "false" => Kind::False, "use" => Kind::Use, "struct" => Kind::Struct,
                "enum" => Kind::Enum, "match" => Kind::Match,
                s => Kind::Ident(s.to_owned()),
            }
        } else if c.is_ascii_digit() {
            i += 1;
            while i < bytes.len() && bytes[i].is_ascii_digit() { i += 1; }
            // A fraction needs a digit after the dot: `1..2` is a range and
            // `1.field` would be a field access, so neither may become a float.
            let mut floating = false;
            if i + 1 < bytes.len() && bytes[i] == b'.' && bytes[i + 1].is_ascii_digit() {
                floating = true;
                i += 1;
                while i < bytes.len() && bytes[i].is_ascii_digit() { i += 1; }
            }
            // An exponent also makes the literal a float: `1e3` is 1000.0.
            if i < bytes.len() && (bytes[i] == b'e' || bytes[i] == b'E') {
                let mut exponent = i + 1;
                if exponent < bytes.len() && (bytes[exponent] == b'+' || bytes[exponent] == b'-') { exponent += 1; }
                if exponent < bytes.len() && bytes[exponent].is_ascii_digit() {
                    floating = true;
                    i = exponent;
                    while i < bytes.len() && bytes[i].is_ascii_digit() { i += 1; }
                }
            }
            let text = &source[start..i];
            if floating {
                let value: f64 = text.parse().map_err(|_| lex_error(source, start, "invalid float literal"))?;
                if !value.is_finite() { return Err(lex_error(source, start, "float literal is not finite")); }
                // Rounding a nonzero literal down to zero would silently replace
                // the value the user wrote, so it is rejected instead of stored.
                let mantissa = text.split(['e', 'E']).next().unwrap_or(text);
                if value == 0.0 && mantissa.bytes().any(|b| b.is_ascii_digit() && b != b'0') {
                    return Err(lex_error(source, start, "float literal underflows to zero"));
                }
                Kind::Float(value)
            } else {
                Kind::Number(text.parse().map_err(|_| lex_error(source, start, "integer literal exceeds i64 maximum"))?)
            }
        } else if ["==", "!=", "<=", ">=", "->", "&&", "||", "..", "=>", "::"].iter().any(|op| source[i..].starts_with(op)) {
            i += 2;
            match &source[start..i] { "==" => Kind::EqEq, "!=" => Kind::NotEq, "<=" => Kind::LessEq, ">=" => Kind::GreaterEq, "->" => Kind::Arrow, "&&" => Kind::And, "||" => Kind::Or, ".." => Kind::DotDot, "=>" => Kind::FatArrow, "::" => Kind::ColonColon, _ => unreachable!() }
        } else {
            i += c.len_utf8();
            match c {
                '(' => Kind::LParen, ')' => Kind::RParen, '{' => Kind::LBrace, '}' => Kind::RBrace,
                '[' => Kind::LBracket, ']' => Kind::RBracket, ',' => Kind::Comma, ':' => Kind::Colon, '&' => Kind::Amp,
                ';' => Kind::Semi, '=' => Kind::Equal, '+' => Kind::Plus, '-' => Kind::Minus,
                '*' => Kind::Star, '/' => Kind::Slash, '%' => Kind::Percent, '<' => Kind::Less, '>' => Kind::Greater,
                '!' => Kind::Bang, '.' => Kind::Dot, '?' => Kind::Question,
                _ => return Err(lex_error(source, start, &format!("unexpected character {c:?}"))),
            }
        };
        for character in source[location_offset..start].chars() {
            if character == '\n' { line += 1; column = 1; } else { column += 1; }
        }
        location_offset = start;
        result.push(Token { kind, offset: start, line, column });
    }
    for character in source[location_offset..].chars() {
        if character == '\n' { line += 1; column = 1; } else { column += 1; }
    }
    result.push(Token { kind: Kind::Eof, offset: source.len(), line, column });
    Ok(result)
}

// String literals keep their source text with escapes uninterpreted: the bytes
// between the quotes are already valid C string body, so emission can reuse them.
#[derive(Debug)]
enum Expr { Number(i64), FloatLit(f64), ToF64(Box<Expr>, usize, usize), ToI64(Box<Expr>, Type, usize, usize), FloatToString(Box<Expr>, usize, usize), BoolLit(bool), StrLit(String, usize, usize), Variable(usize, Type, usize, usize), Call(usize, Vec<Argument>, Type), Index(usize, usize, Box<Expr>, usize, usize), VecLit(Vec<Expr>, usize, usize), VecIndex(usize, Box<Expr>, usize, usize, bool), Len(Box<Expr>, usize, usize), ReadFile(Box<Expr>, usize, usize), WriteFile(Box<Expr>, Box<Expr>, usize, usize), Bytes(Box<Expr>, usize, usize), Contains(Box<Expr>, Box<Expr>, usize, usize), Slice(Box<Expr>, Box<Expr>, Box<Expr>, usize, usize), Split(Box<Expr>, Box<Expr>, usize, usize), Args(usize, usize), Join(usize, Box<Expr>, usize, usize), StrVecIndex(usize, Box<Expr>, usize, usize, bool), SliceView(usize, Box<Expr>, Box<Expr>, usize, usize), Lines(Box<Expr>, usize, usize), CountLinesWith(Box<Expr>, Box<Expr>, usize, usize), StructLit(usize, Vec<Expr>), EnumLit(&'static str, usize, usize, Vec<Expr>), TryReadFile(Box<Expr>, Type, usize, usize), TryListDir(Box<Expr>, Type, usize, usize), PathExists(Box<Expr>, usize, usize), PathIsDir(Box<Expr>, usize, usize), TryWriteFile(Box<Expr>, Box<Expr>, Type, usize, usize), ParseInt(Box<Expr>, Type, usize, usize), ToString(Box<Expr>, usize, usize), PathJoin(Box<Expr>, Box<Expr>, usize, usize), PathName(Box<Expr>, usize, usize), PathParent(Box<Expr>, usize, usize), TryFileInfo(Box<Expr>, Type, usize, usize), TryCopyFile(Box<Expr>, Box<Expr>, Type, usize, usize), TryRenamePath(Box<Expr>, Box<Expr>, Type, usize, usize), TryRemoveFile(Box<Expr>, Type, usize, usize), TryRemoveDir(Box<Expr>, Type, usize, usize), TryCreateDir(Box<Expr>, Type, usize, usize), SetCwd(Box<Expr>, Type, usize, usize), MapNew(usize, usize), MapGet(MapPlace, Box<Expr>, Type, usize, usize), MapHas(MapPlace, Box<Expr>, usize, usize), MapRemove(MapPlace, Box<Expr>, usize, usize), MapKeys(MapPlace, usize, usize), Utf8Len(Box<Expr>, usize, usize), Utf8Prefix(Box<Expr>, Box<Expr>, usize, usize), Trim(Box<Expr>, usize, usize), StartsWith(Box<Expr>, Box<Expr>), EndsWith(Box<Expr>, Box<Expr>),
    TryAppendFile(Box<Expr>, Box<Expr>, Type, usize, usize), Env(Box<Expr>, Type, usize, usize), JsonGet(Box<Expr>, Box<Expr>, Type, usize, usize), JsonInt(Box<Expr>, Box<Expr>, Type, usize, usize), JsonBool(Box<Expr>, Box<Expr>, Type, usize, usize), JsonLen(Box<Expr>, Box<Expr>, Type, usize, usize), JsonHas(Box<Expr>, Box<Expr>, usize, usize), Find(Box<Expr>, Box<Expr>, Type, usize, usize), ReadStdin(Type, usize, usize), Cwd(Type, usize, usize), StrVecNew(usize, usize), RunCommand(Box<Expr>, usize, Type, usize, usize), NowMs, NowS, Field(Box<Expr>, String, Type), StrEq(Box<Expr>, Box<Expr>, bool), Neg(Box<Expr>, usize, usize), Not(Box<Expr>), Binary(Box<Expr>, Kind, Box<Expr>, usize, usize, Type) }
// A map reachable without a move: either a variable slot or a field of one.
#[derive(Debug)]
enum MapPlace { Var(usize), Field(usize, String) }
impl MapPlace {
    fn text(&self) -> String {
        match self {
            MapPlace::Var(id) => format!("v{id}"),
            MapPlace::Field(id, name) => format!("v{id}.{name}"),
        }
    }
    fn slot(&self) -> usize {
        match self {
            MapPlace::Var(id) | MapPlace::Field(id, _) => *id,
        }
    }
}
#[derive(Debug)]
enum Argument { Value(Expr), Borrow(usize, bool), View(Expr) } // Borrow bool: pass &slot vs array (decays); View: read-only string borrow
#[derive(Debug)]
enum Printed { Expr(Expr), Text(String) }
#[derive(Debug)]
enum Statement { Return(Expr, Vec<(usize, Type)>), Declare(usize, Expr), Array(usize, Vec<Expr>), Assign(usize, Expr, bool), Store(usize, usize, Expr, Expr, usize, usize), VecPush(usize, Expr, usize, usize), Sort(usize, bool, usize, usize), Reverse(usize, bool, usize, usize), VecPop(usize, usize, usize), VecStore(usize, Expr, Expr, usize, usize), StrVecPush(usize, Expr, usize, usize, bool), StrVecPop(usize, usize, usize), StrVecStore(usize, Expr, Expr, usize, usize, bool), FieldStore(usize, String, Type, Expr), Print(Printed), Assert(Expr, usize, usize), If(Expr, Vec<Statement>, Vec<Statement>), While(Expr, Vec<Statement>), For(usize, usize, Vec<Statement>, usize, usize, usize), Match(usize, Type, Vec<MatchArm>), MapPut(MapPlace, Expr, Expr, usize, usize), Propagate(Expr, Type, Vec<Type>, Option<(usize, Type)>, Vec<(usize, Type)>), Break, Continue(Option<usize>), Exit(Expr, Vec<(usize, Type)>), Frees(Vec<(usize, Type)>) }
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
// `fn pick[T](a: T, b: T) -> T { ... }` before any call site picks arguments.
// The declaration keeps its own token slice and source text so an instantiation
// can be parsed later, even from another file (ADR-31).
#[derive(Debug, Clone)]
struct GenericFn { name: String, params: Vec<String>, bounds: Vec<Vec<Bound>>, start: usize, tokens: Vec<Token>, source: String, file: String, public: bool }
// An instantiated generic function: its id, parameter types and return type.
type Instance = (usize, Vec<Type>, Type);
// Instantiated generic enums, structs and functions, keyed by declaration and
// arguments (ADR-21, ADR-30, ADR-31). `next_fn_id` is the one id space shared
// by ordinary and instantiated functions, so C names never collide.
#[derive(Default)]
struct Generics { defs: Vec<GenericEnum>, names: HashMap<String, usize>, done: HashMap<String, usize>, struct_defs: Vec<GenericStruct>, struct_names: HashMap<String, usize>, struct_done: HashMap<String, usize>, fn_defs: Vec<GenericFn>, fn_names: HashMap<String, usize>, fn_done: HashMap<String, Instance>, fn_in_progress: std::collections::HashSet<String>, next_fn_id: usize }
#[derive(Debug)]
struct MatchArm { variant: usize, bindings: Vec<(usize, Type, usize)>, body: Vec<Statement> }

// A function signature as seen by importers. `file` and `public` let a call
// site reject a name that exists but was not exported (ADR-18).
#[derive(Debug, Clone)]
struct Signature { id: usize, params: Vec<Type>, ret: Type, file: String, public: bool }

type SharedFunctions = HashMap<String, Signature>;

struct Parser<'a> { file: String, source: &'a str, tokens: Vec<Token>, pos: usize, names: HashMap<String, (usize, bool, Type)>, next_var: usize, expression_depth: usize, expression_nodes: usize, block_depth: usize, loop_depth: usize, loop_scopes: Vec<usize>, loop_labels: Vec<Option<usize>>, next_label: usize, functions: &'a mut SharedFunctions, structs: &'a mut Vec<StructDef>, struct_names: &'a mut HashMap<String, usize>, enums: &'a mut Vec<EnumDef>, enum_names: &'a mut HashMap<String, usize>, generics: &'a mut Generics, id_order: Vec<usize>, in_main: bool, return_type: Type, moved: std::collections::HashSet<usize>, borrows: Vec<(usize, usize)>, scopes: Vec<Vec<(usize, Type)>>, subst: HashMap<String, Type>, pending: Vec<Function> }
impl<'a> Parser<'a> {
    fn current(&self) -> &Kind { &self.tokens[self.pos].kind }
    fn fail(&self, message: &str) -> String { error(&self.file, self.source, self.tokens[self.pos].offset, message) }
    fn take(&mut self, expected: Kind) -> Result<(), String> {
        if *self.current() != expected { return Err(self.fail(&format!("expected {expected:?}, found {:?}", self.current()))); }
        self.pos += 1; Ok(())
    }
    fn identifier(&mut self) -> Result<String, String> {
        match self.current().clone() { Kind::Ident(s) => { self.pos += 1; Ok(s) }, _ => Err(self.fail("expected identifier")) }
    }
    fn position(&self) -> (usize, usize) {
        let token = &self.tokens[self.pos];
        (token.line, token.column)
    }
    // Prelude result types are always registered, so naming one cannot fail.
    fn prelude_type(&self, name: &str) -> Type {
        match self.enum_names.get(name) {
            Some(&id) => Type::Enum(self.enums[id].name, id),
            None => unreachable!("prelude enum {name} is registered for every program"),
        }
    }
    fn expression(&mut self, min: u8, moving: bool) -> Result<(Expr, Type), String> {
        if self.expression_depth == 0 { self.expression_nodes = 0; }
        if self.expression_depth >= 128 { return Err(self.fail("expression nesting limit exceeded (128)")); }
        self.expression_depth += 1;
        let result = self.expression_inner(min, moving);
        self.expression_depth -= 1;
        result
    }
    fn expression_inner(&mut self, min: u8, moving: bool) -> Result<(Expr, Type), String> {
        self.expression_nodes += 1;
        if self.expression_nodes > 128 { return Err(self.fail("expression complexity limit exceeded (128)")); }
        let mut lhs = match self.current().clone() {
            Kind::Number(n) => { self.pos += 1; (Expr::Number(n), Type::Int) }
            Kind::Float(x) => { self.pos += 1; (Expr::FloatLit(x), Type::F64) }
            Kind::True => { self.pos += 1; (Expr::BoolLit(true), Type::Bool) }
            Kind::False => { self.pos += 1; (Expr::BoolLit(false), Type::Bool) }
            Kind::Text(text) => {
                let (line, column) = self.position();
                self.pos += 1;
                (Expr::StrLit(text, line, column), Type::String)
            }
            Kind::Minus => {
                let (line, column) = self.position();
                self.pos += 1;
                // Operands bind tighter than any binary operator: -a * b is (-a) * b.
                let (operand, ty) = self.expression(6, false)?;
                if !matches!(ty, Type::Int | Type::F64) { return Err(self.fail("unary '-' requires an i64 or f64 operand")); }
                (Expr::Neg(Box::new(operand), line, column), ty)
            }
            Kind::Bang => {
                self.pos += 1;
                let (operand, ty) = self.expression(6, false)?;
                if ty != Type::Bool { return Err(self.fail("'!' requires a boolean operand")); }
                (Expr::Not(Box::new(operand)), Type::Bool)
            }
            Kind::Ident(name) => {
                let name_offset = self.tokens[self.pos].offset;
                let (vline, vcol) = self.position();
                self.pos += 1;
                if name == "vec" && *self.current() == Kind::LBracket {
                    let (line, column) = (vline, vcol);
                    self.pos += 1;
                    let mut values = Vec::new();
                    if *self.current() != Kind::RBracket {
                        loop {
                            if values.len() >= 65536 { return Err(self.fail("vector element limit exceeded (65536)")); }
                            let (element, element_ty) = self.expression(0, false)?;
                            if element_ty != Type::Int { return Err(self.fail("vector elements must be i64")); }
                            values.push(element);
                            if *self.current() != Kind::Comma { break; }
                            self.pos += 1;
                        }
                    }
                    self.take(Kind::RBracket)?;
                    // Bind and fall through to the binary-operator loop.
                    (Expr::VecLit(values, line, column), Type::Vec)
                } else if *self.current() == Kind::ColonColon {
                    // Enum::Variant or Enum::Variant(payload, ...)
                    let eid = *self.enum_names.get(&name)
                        .ok_or_else(|| error(&self.file, self.source, name_offset, &format!("'{name}' is not an enum; variants are written Enum::Variant")))?;
                    self.enum_literal(eid, name_offset)?
                } else if self.generics.names.contains_key(&name) && *self.current() == Kind::LBracket {
                    // Generic enum literal: the arguments pick the layout, so
                    // Option[string]::Some and Option[i64]::Some are different
                    // variants of different types (ADR-21).
                    let gid = self.generics.names[&name];
                    self.pos += 1;
                    let mut args = Vec::new();
                    loop {
                        args.push(self.type_argument()?);
                        if *self.current() != Kind::Comma { break; }
                        self.pos += 1;
                    }
                    self.take(Kind::RBracket)?;
                    let eid = self.instantiate_enum(gid, args, name_offset)?;
                    self.enum_literal(eid, name_offset)?
                } else if self.generics.struct_names.contains_key(&name) && *self.current() == Kind::LBracket {
                    // Generic struct literal: the arguments pick the layout, so
                    // Pair[string, i64] and Pair[i64, i64] are different types.
                    let gid = self.generics.struct_names[&name];
                    self.pos += 1;
                    let mut args = Vec::new();
                    loop {
                        args.push(self.named_type()?);
                        if *self.current() != Kind::Comma { break; }
                        self.pos += 1;
                    }
                    self.take(Kind::RBracket)?;
                    let sid = self.instantiate_struct(gid, args, name_offset)?;
                    self.require_public("struct", &self.structs[sid].name.clone(), &self.structs[sid].file.clone(), self.structs[sid].public)?;
                    if *self.current() != Kind::LBrace {
                        return Err(error(&self.file, self.source, name_offset, &format!("'{}' is a struct; construct it with '{}{{ ... }}'", self.structs[sid].name.clone(), self.structs[sid].name.clone())));
                    }
                    return self.struct_literal(sid, name_offset);
                } else if let Some(&sid) = self.struct_names.get(&name) {
                    self.require_public("struct", &self.structs[sid].name, &self.structs[sid].file, self.structs[sid].public)?;
                    if *self.current() == Kind::LBrace {
                        // A fresh literal takes no postfix; wrap it in
                        // parentheses for field access.
                        return self.struct_literal(sid, name_offset);
                    } else if *self.current() == Kind::LParen {
                        return Err(self.fail(&format!("'{}' is a struct, not a function", name)));
                    } else {
                        return Err(self.fail(&format!("'{}' is a struct name; construct it with '{} {{ ... }}'", name, name)));
                    }
                } else if name == "len" && *self.current() == Kind::LParen {
                    let (line, col) = (vline, vcol);
                    self.take(Kind::LParen)?;
                    // Fast paths for bare variables: array length is a
                    // compile-time constant; vec length borrows the slot.
                    let mut fast: Option<(Expr, Type)> = None;
                    if let Kind::Ident(arg_name) = self.current().clone() {
                        let after = self.tokens.get(self.pos + 1).map(|t| t.kind.clone());
                        if after == Some(Kind::RParen) {
                            if let Some(&(id, _, ty)) = self.names.get(&arg_name) {
                                if self.moved.contains(&id) { return Err(error(&self.file, self.source, name_offset, &format!("cannot use '{arg_name}' because it was moved"))); }
                                match ty {
                                    Type::Array(n) => { self.pos += 2; fast = Some((Expr::Number(n as i64), Type::Int)); }
                                    Type::Vec | Type::StrVec | Type::VecRef | Type::StrVecRef | Type::Map => {
                                        self.pos += 2;
                                        fast = Some((Expr::Len(Box::new(Expr::Variable(id, ty, vline, vcol)), line, col), Type::Int));
                                    }
                                    _ => {}
                                }
                            }
                        }
                    }
                    match fast {
                        Some(pair) => pair,
                        None => {
                            let (operand, ty) = self.expression(0, false)?;
                            self.take(Kind::RParen)?;
                            if !matches!(ty, Type::String | Type::StrView | Type::Vec | Type::StrVec | Type::VecRef | Type::Map) { return Err(self.fail("len expects a string, vec, map, or array variable")); }
                            (Expr::Len(Box::new(operand), line, col), Type::Int)
                        }
                    }
                } else if name == "slice_view" && *self.current() == Kind::LParen {
                    self.take(Kind::LParen)?;
                    let (base, base_ty) = self.expression(0, false)?;
                    let vid = match (&base, base_ty) {
                        (Expr::Variable(id, Type::String, _, _), Type::String) => *id,
                        _ => return Err(self.fail("slice_view expects a bare string variable (views borrow their base)")),
                    };
                    if self.moved.contains(&vid) { return Err(self.fail("cannot use a moved string in slice_view")); }
                    self.take(Kind::Comma)?;
                    let (start, start_ty) = self.expression(0, false)?;
                    if start_ty != Type::Int { return Err(self.fail("slice_view start must be i64")); }
                    self.take(Kind::Comma)?;
                    let (end, end_ty) = self.expression(0, false)?;
                    if end_ty != Type::Int { return Err(self.fail("slice_view end must be i64")); }
                    self.take(Kind::RParen)?;
                    // The base may not move or be reassigned while a view of
                    // it can still be read (until this scope ends).
                    let depth = self.scopes.len();
                    if !self.borrows.iter().any(|&(id, _)| id == vid) {
                        self.borrows.push((vid, depth));
                    }
                    (Expr::SliceView(vid, Box::new(start), Box::new(end), 0, 0), Type::StrView)
                } else if name == "count_lines_with" && *self.current() == Kind::LParen {
                    let (line, col) = (vline, vcol);
                    self.take(Kind::LParen)?;
                    let (text, text_ty) = self.expression(0, false)?;
                    if !matches!(text_ty, Type::String | Type::StrView) { return Err(self.fail("count_lines_with expects a string or string view")); }
                    self.take(Kind::Comma)?;
                    let (pat, pat_ty) = self.expression(0, false)?;
                    if !matches!(pat_ty, Type::String | Type::StrView) { return Err(self.fail("count_lines_with expects a string or string view pattern")); }
                    self.take(Kind::RParen)?;
                    (Expr::CountLinesWith(Box::new(text), Box::new(pat), line, col), Type::Int)
                } else if name == "lines" && *self.current() == Kind::LParen {
                    let (line, col) = (vline, vcol);
                    self.take(Kind::LParen)?;
                    let (text, text_ty) = self.expression(0, false)?;
                    if !matches!(text_ty, Type::String | Type::StrView) { return Err(self.fail("lines expects a string or string view")); }
                    self.take(Kind::RParen)?;
                    (Expr::Lines(Box::new(text), line, col), Type::StrVec)
                } else if name == "join" && *self.current() == Kind::LParen {
                    let (line, col) = (vline, vcol);
                    self.take(Kind::LParen)?;
                    // join borrows a bare string-vector variable.
                    let (parts, parts_ty) = self.expression(0, false)?;
                    let vid = match (&parts, parts_ty) {
                        (Expr::Variable(id, Type::StrVec, _, _), Type::StrVec) => *id,
                        _ => return Err(self.fail("join expects a bare vec[string] variable")),
                    };
                    if self.moved.contains(&vid) { return Err(self.fail("cannot use a moved vector in join")); }
                    self.take(Kind::Comma)?;
                    let (sep, sep_ty) = self.expression(0, false)?;
                    if !matches!(sep_ty, Type::String | Type::StrView) { return Err(self.fail("join expects a string or string view separator")); }
                    self.take(Kind::RParen)?;
                    (Expr::Join(vid, Box::new(sep), line, col), Type::String)
                } else if name == "args" && *self.current() == Kind::LParen {
                    let (line, col) = (vline, vcol);
                    self.take(Kind::LParen)?;
                    self.take(Kind::RParen)?;
                    (Expr::Args(line, col), Type::StrVec)
                } else if name == "try_read_file" && *self.current() == Kind::LParen {
                    let (line, col) = (vline, vcol);
                    let ty = self.prelude_type("ReadResult");
                    self.take(Kind::LParen)?;
                    let (path, path_ty) = self.expression(0, false)?;
                    if !matches!(path_ty, Type::String | Type::StrView) { return Err(self.fail("try_read_file expects a string path")); }
                    self.take(Kind::RParen)?;
                    (Expr::TryReadFile(Box::new(path), ty, line, col), ty)
                } else if name == "try_write_file" && *self.current() == Kind::LParen {
                    let (line, col) = (vline, vcol);
                    let ty = self.prelude_type("WriteResult");
                    self.take(Kind::LParen)?;
                    let (path, path_ty) = self.expression(0, false)?;
                    if !matches!(path_ty, Type::String | Type::StrView) { return Err(self.fail("try_write_file expects a string path")); }
                    self.take(Kind::Comma)?;
                    let (content, content_ty) = self.expression(0, false)?;
                    if !matches!(content_ty, Type::String | Type::StrView) { return Err(self.fail("try_write_file expects a string or string view content")); }
                    self.take(Kind::RParen)?;
                    (Expr::TryWriteFile(Box::new(path), Box::new(content), ty, line, col), ty)
                } else if name == "try_append_file" && *self.current() == Kind::LParen {
                    let (line, col) = (vline, vcol);
                    let ty = self.prelude_type("WriteResult");
                    self.take(Kind::LParen)?;
                    let (path, path_ty) = self.expression(0, false)?;
                    if !matches!(path_ty, Type::String | Type::StrView) { return Err(self.fail("try_append_file expects a string path")); }
                    self.take(Kind::Comma)?;
                    let (content, content_ty) = self.expression(0, false)?;
                    if !matches!(content_ty, Type::String | Type::StrView) { return Err(self.fail("try_append_file expects a string or string view content")); }
                    self.take(Kind::RParen)?;
                    (Expr::TryAppendFile(Box::new(path), Box::new(content), ty, line, col), ty)
                } else if name == "env" && *self.current() == Kind::LParen {
                    let (line, col) = (vline, vcol);
                    let ty = self.prelude_type("ReadResult");
                    self.take(Kind::LParen)?;
                    let (variable, variable_ty) = self.expression(0, false)?;
                    if !matches!(variable_ty, Type::String | Type::StrView) { return Err(self.fail("env expects a string variable name")); }
                    self.take(Kind::RParen)?;
                    (Expr::Env(Box::new(variable), ty, line, col), ty)
                } else if (name == "read_stdin" || name == "cwd") && *self.current() == Kind::LParen {
                    let (line, col) = (vline, vcol);
                    let ty = self.prelude_type("ReadResult");
                    self.take(Kind::LParen)?;
                    self.take(Kind::RParen)?;
                    (if name == "read_stdin" { Expr::ReadStdin(ty, line, col) } else { Expr::Cwd(ty, line, col) }, ty)
                } else if name == "strvec_new" && *self.current() == Kind::LParen {
                    let (line, col) = (vline, vcol);
                    self.take(Kind::LParen)?;
                    self.take(Kind::RParen)?;
                    (Expr::StrVecNew(line, col), Type::StrVec)
                } else if name == "run_command" && *self.current() == Kind::LParen {
                    let (line, col) = (vline, vcol);
                    let ty = self.prelude_type("CommandResult");
                    self.take(Kind::LParen)?;
                    let (program, program_ty) = self.expression(0, false)?;
                    if !matches!(program_ty, Type::String | Type::StrView) { return Err(self.fail("run_command expects a string program name")); }
                    self.take(Kind::Comma)?;
                    // Like join, the argument vector is borrowed, never moved:
                    // the callee only reads the elements during the call.
                    let (args, args_ty) = self.expression(0, false)?;
                    let vid = match (&args, args_ty) {
                        (Expr::Variable(id, Type::StrVec, _, _), Type::StrVec) => *id,
                        _ => return Err(self.fail("run_command expects a bare vec[string] variable for its arguments")),
                    };
                    if self.moved.contains(&vid) { return Err(self.fail("cannot use a moved vector in run_command")); }
                    self.take(Kind::RParen)?;
                    (Expr::RunCommand(Box::new(program), vid, ty, line, col), ty)
                } else if name == "find" && *self.current() == Kind::LParen {
                    // Byte offset of the first match, as a result value: a -1
                    // sentinel would let callers ignore "not found", while
                    // every other fallible built-in returns a result value
                    // whose error must be handled.
                    let (line, col) = (vline, vcol);
                    let ty = self.prelude_type("IntResult");
                    self.take(Kind::LParen)?;
                    let (haystack, haystack_ty) = self.expression(0, false)?;
                    if !matches!(haystack_ty, Type::String | Type::StrView) { return Err(self.fail("find expects a string to search in")); }
                    self.take(Kind::Comma)?;
                    let (needle, needle_ty) = self.expression(0, false)?;
                    if !matches!(needle_ty, Type::String | Type::StrView) { return Err(self.fail("find expects a string to search for")); }
                    self.take(Kind::RParen)?;
                    (Expr::Find(Box::new(haystack), Box::new(needle), ty, line, col), ty)
                } else if (name == "json_get" || name == "json_int" || name == "json_bool" || name == "json_len" || name == "json_has") && *self.current() == Kind::LParen {
                    let (line, col) = (vline, vcol);
                    let ty = match name.as_str() {
                        "json_get" => self.prelude_type("ReadResult"),
                        "json_has" => Type::Bool,
                        "json_bool" => self.prelude_type("BoolResult"),
                        _ => self.prelude_type("IntResult"),
                    };
                    self.take(Kind::LParen)?;
                    let (doc, doc_ty) = self.expression(0, false)?;
                    if !matches!(doc_ty, Type::String | Type::StrView) { return Err(self.fail(&format!("{name} expects a JSON document string"))); }
                    self.take(Kind::Comma)?;
                    let (path, path_ty) = self.expression(0, false)?;
                    if !matches!(path_ty, Type::String | Type::StrView) { return Err(self.fail(&format!("{name} expects a string path"))); }
                    self.take(Kind::RParen)?;
                    let (doc, path) = (Box::new(doc), Box::new(path));
                    (match name.as_str() {
                        "json_get" => Expr::JsonGet(doc, path, ty, line, col),
                        "json_int" => Expr::JsonInt(doc, path, ty, line, col),
                        "json_bool" => Expr::JsonBool(doc, path, ty, line, col),
                        "json_len" => Expr::JsonLen(doc, path, ty, line, col),
                        _ => Expr::JsonHas(doc, path, line, col),
                    }, ty)
                } else if (name == "now_ms" || name == "now_s") && *self.current() == Kind::LParen {
                    self.take(Kind::LParen)?;
                    self.take(Kind::RParen)?;
                    (if name == "now_ms" { Expr::NowMs } else { Expr::NowS }, Type::Int)
                } else if name == "try_list_dir" && *self.current() == Kind::LParen {
                    let (line, col) = (vline, vcol);
                    let ty = self.prelude_type("DirResult");
                    self.take(Kind::LParen)?;
                    let (path, path_ty) = self.expression(0, false)?;
                    if !matches!(path_ty, Type::String | Type::StrView) { return Err(self.fail("try_list_dir expects a string path")); }
                    self.take(Kind::RParen)?;
                    (Expr::TryListDir(Box::new(path), ty, line, col), ty)
                } else if name == "path_is_dir" && *self.current() == Kind::LParen {
                    let (line, col) = (vline, vcol);
                    self.take(Kind::LParen)?;
                    let (path, path_ty) = self.expression(0, false)?;
                    if !matches!(path_ty, Type::String | Type::StrView) { return Err(self.fail("path_is_dir expects a string path")); }
                    self.take(Kind::RParen)?;
                    (Expr::PathIsDir(Box::new(path), line, col), Type::Bool)
                } else if name == "path_exists" && *self.current() == Kind::LParen {
                    let (line, col) = (vline, vcol);
                    self.take(Kind::LParen)?;
                    let (path, path_ty) = self.expression(0, false)?;
                    if !matches!(path_ty, Type::String | Type::StrView) { return Err(self.fail("path_exists expects a string path")); }
                    self.take(Kind::RParen)?;
                    (Expr::PathExists(Box::new(path), line, col), Type::Bool)
                } else if name == "map_new" && *self.current() == Kind::LParen {
                    let (line, col) = (vline, vcol);
                    self.take(Kind::LParen)?;
                    self.take(Kind::RParen)?;
                    (Expr::MapNew(line, col), Type::Map)
                } else if matches!(name.as_str(), "map_get" | "map_has" | "map_remove" | "keys") && *self.current() == Kind::LParen {
                    let (line, col) = (vline, vcol);
                    let ty = if name == "map_get" { self.prelude_type("IntResult") } else if name == "keys" { Type::StrVec } else { Type::Bool };
                    self.take(Kind::LParen)?;
                    // A bare map variable keeps the borrow explicit: the map is
                    // read (or mutated) only for the duration of this call.
                    let (map, map_ty) = self.expression(0, false)?;
                    let place = match (&map, map_ty) {
                        (Expr::Variable(id, Type::Map, _, _), Type::Map) => MapPlace::Var(*id),
                        (Expr::Field(base, fname, Type::Map), Type::Map) => match **base {
                            Expr::Variable(id, _, _, _) => MapPlace::Field(id, fname.clone()),
                            _ => return Err(self.fail(&format!("{name} needs a map variable or a field of one"))),
                        },
                        _ => return Err(self.fail(&format!("{name} expects a map variable or a field of one"))),
                    };
                    if self.moved.contains(&place.slot()) { return Err(self.fail(&format!("cannot use '{name}' on a moved map"))); }
                    if name == "keys" {
                        self.take(Kind::RParen)?;
                        (Expr::MapKeys(place, line, col), ty)
                    } else {
                        self.take(Kind::Comma)?;
                        let (key, key_ty) = self.expression(0, false)?;
                        if !matches!(key_ty, Type::String | Type::StrView) { return Err(self.fail(&format!("{name} expects a string key"))); }
                        self.take(Kind::RParen)?;
                        let builtin = match name.as_str() {
                            "map_get" => Expr::MapGet(place, Box::new(key), ty, line, col),
                            "map_has" => Expr::MapHas(place, Box::new(key), line, col),
                            _ => Expr::MapRemove(place, Box::new(key), line, col),
                        };
                        (builtin, ty)
                    }
                } else if matches!(name.as_str(), "utf8_len" | "utf8_prefix") && *self.current() == Kind::LParen {
                    let (line, col) = (vline, vcol);
                    self.take(Kind::LParen)?;
                    let (text, text_ty) = self.expression(0, false)?;
                    if !matches!(text_ty, Type::String | Type::StrView) { return Err(self.fail(&format!("{name} expects a string or string view"))); }
                    if name == "utf8_len" {
                        self.take(Kind::RParen)?;
                        (Expr::Utf8Len(Box::new(text), line, col), Type::Int)
                    } else {
                        self.take(Kind::Comma)?;
                        let (count, count_ty) = self.expression(0, false)?;
                        if count_ty != Type::Int { return Err(self.fail("utf8_prefix expects an i64 character count")); }
                        self.take(Kind::RParen)?;
                        (Expr::Utf8Prefix(Box::new(text), Box::new(count), line, col), Type::String)
                    }
                } else if name == "path_join" && *self.current() == Kind::LParen {
                    let (line, col) = (vline, vcol);
                    self.take(Kind::LParen)?;
                    let (dir, dir_ty) = self.expression(0, false)?;
                    if !matches!(dir_ty, Type::String | Type::StrView) { return Err(self.fail("path_join expects a string directory")); }
                    self.take(Kind::Comma)?;
                    let (name, name_ty) = self.expression(0, false)?;
                    if !matches!(name_ty, Type::String | Type::StrView) { return Err(self.fail("path_join expects a string name")); }
                    self.take(Kind::RParen)?;
                    (Expr::PathJoin(Box::new(dir), Box::new(name), line, col), Type::String)
                } else if name == "path_name" && *self.current() == Kind::LParen {
                    let (line, col) = (vline, vcol);
                    self.take(Kind::LParen)?;
                    let (path, path_ty) = self.expression(0, false)?;
                    if !matches!(path_ty, Type::String | Type::StrView) { return Err(self.fail("path_name expects a string path")); }
                    self.take(Kind::RParen)?;
                    (Expr::PathName(Box::new(path), line, col), Type::String)
                } else if name == "path_parent" && *self.current() == Kind::LParen {
                    let (line, col) = (vline, vcol);
                    self.take(Kind::LParen)?;
                    let (path, path_ty) = self.expression(0, false)?;
                    if !matches!(path_ty, Type::String | Type::StrView) { return Err(self.fail("path_parent expects a string path")); }
                    self.take(Kind::RParen)?;
                    (Expr::PathParent(Box::new(path), line, col), Type::String)
                } else if name == "try_file_info" && *self.current() == Kind::LParen {
                    let (line, col) = (vline, vcol);
                    let ty = self.prelude_type("FileResult");
                    self.take(Kind::LParen)?;
                    let (path, path_ty) = self.expression(0, false)?;
                    if !matches!(path_ty, Type::String | Type::StrView) { return Err(self.fail("try_file_info expects a string path")); }
                    self.take(Kind::RParen)?;
                    (Expr::TryFileInfo(Box::new(path), ty, line, col), ty)
                } else if matches!(name.as_str(), "try_copy_file" | "try_rename_path") && *self.current() == Kind::LParen {
                    let (line, col) = (vline, vcol);
                    let ty = self.prelude_type("StatusResult");
                    self.take(Kind::LParen)?;
                    let (from, from_ty) = self.expression(0, false)?;
                    if !matches!(from_ty, Type::String | Type::StrView) { return Err(self.fail(&format!("{name} expects a string source path"))); }
                    self.take(Kind::Comma)?;
                    let (to, to_ty) = self.expression(0, false)?;
                    if !matches!(to_ty, Type::String | Type::StrView) { return Err(self.fail(&format!("{name} expects a string destination path"))); }
                    self.take(Kind::RParen)?;
                    let builtin = if name == "try_copy_file" {
                        Expr::TryCopyFile(Box::new(from), Box::new(to), ty, line, col)
                    } else {
                        Expr::TryRenamePath(Box::new(from), Box::new(to), ty, line, col)
                    };
                    (builtin, ty)
                } else if matches!(name.as_str(), "try_remove_file" | "try_create_dir" | "try_remove_dir" | "set_cwd") && *self.current() == Kind::LParen {
                    let (line, col) = (vline, vcol);
                    let ty = self.prelude_type("StatusResult");
                    self.take(Kind::LParen)?;
                    let (path, path_ty) = self.expression(0, false)?;
                    if !matches!(path_ty, Type::String | Type::StrView) { return Err(self.fail(&format!("{name} expects a string path"))); }
                    self.take(Kind::RParen)?;
                    let builtin = if name == "try_remove_file" {
                        Expr::TryRemoveFile(Box::new(path), ty, line, col)
                    } else if name == "try_remove_dir" {
                        Expr::TryRemoveDir(Box::new(path), ty, line, col)
                    } else if name == "set_cwd" {
                        Expr::SetCwd(Box::new(path), ty, line, col)
                    } else {
                        Expr::TryCreateDir(Box::new(path), ty, line, col)
                    };
                    (builtin, ty)
                } else if name == "parse_int" && *self.current() == Kind::LParen {
                    let (line, col) = (vline, vcol);
                    let ty = self.prelude_type("IntResult");
                    self.take(Kind::LParen)?;
                    let (text, text_ty) = self.expression(0, false)?;
                    if !matches!(text_ty, Type::String | Type::StrView) { return Err(self.fail("parse_int expects a string or string view")); }
                    self.take(Kind::RParen)?;
                    (Expr::ParseInt(Box::new(text), ty, line, col), ty)
                } else if name == "to_string" && *self.current() == Kind::LParen {
                    let (line, col) = (vline, vcol);
                    self.take(Kind::LParen)?;
                    let (value, value_ty) = self.expression(0, false)?;
                    let builtin = match value_ty {
                        Type::Int => Expr::ToString(Box::new(value), line, col),
                        Type::F64 => Expr::FloatToString(Box::new(value), line, col),
                        _ => return Err(self.fail("to_string expects an i64 or f64")),
                    };
                    self.take(Kind::RParen)?;
                    (builtin, Type::String)
                } else if name == "to_f64" && *self.current() == Kind::LParen {
                    let (line, col) = (vline, vcol);
                    self.take(Kind::LParen)?;
                    let (value, value_ty) = self.expression(0, false)?;
                    if value_ty != Type::Int { return Err(self.fail("to_f64 expects an i64")); }
                    self.take(Kind::RParen)?;
                    (Expr::ToF64(Box::new(value), line, col), Type::F64)
                } else if name == "to_i64" && *self.current() == Kind::LParen {
                    let (line, col) = (vline, vcol);
                    let ty = self.prelude_type("IntResult");
                    self.take(Kind::LParen)?;
                    let (value, value_ty) = self.expression(0, false)?;
                    if value_ty != Type::F64 { return Err(self.fail("to_i64 expects an f64")); }
                    self.take(Kind::RParen)?;
                    (Expr::ToI64(Box::new(value), ty, line, col), ty)
                } else if matches!(name.as_str(), "read_file" | "write_file" | "bytes" | "contains" | "starts_with" | "ends_with" | "slice" | "split" | "trim") && *self.current() == Kind::LParen {
                    let (line, col) = (vline, vcol);
                    self.pos += 1;
                    let (first, first_ty) = self.expression(0, false)?;
                    if !matches!(first_ty, Type::String | Type::StrView) { return Err(self.fail(&format!("{name} expects a string or string view argument"))); }
                    let builtin = if name == "read_file" {
                        self.take(Kind::RParen)?;
                        Expr::ReadFile(Box::new(first), line, col)
                    } else if name == "bytes" {
                        self.take(Kind::RParen)?;
                        Expr::Bytes(Box::new(first), line, col)
                    } else if name == "trim" {
                        self.take(Kind::RParen)?;
                        Expr::Trim(Box::new(first), line, col)
                    } else if name == "slice" {
                        self.take(Kind::Comma)?;
                        let (start, start_ty) = self.expression(0, false)?;
                        if start_ty != Type::Int { return Err(self.fail("slice start must be i64")); }
                        self.take(Kind::Comma)?;
                        let (end, end_ty) = self.expression(0, false)?;
                        if end_ty != Type::Int { return Err(self.fail("slice end must be i64")); }
                        self.take(Kind::RParen)?;
                        Expr::Slice(Box::new(first), Box::new(start), Box::new(end), line, col)
                    } else if name == "split" {
                        self.take(Kind::Comma)?;
                        let (second, second_ty) = self.expression(0, false)?;
                        if !matches!(second_ty, Type::String | Type::StrView) { return Err(self.fail("split expects a string or string view separator")); }
                        self.take(Kind::RParen)?;
                        Expr::Split(Box::new(first), Box::new(second), line, col)
                    } else if matches!(name.as_str(), "contains" | "starts_with" | "ends_with") {
                        self.take(Kind::Comma)?;
                        let (second, second_ty) = self.expression(0, false)?;
                        if !matches!(second_ty, Type::String | Type::StrView) { return Err(self.fail(&format!("{name} expects a string or string view pattern"))); }
                        self.take(Kind::RParen)?;
                        match name.as_str() {
                            "contains" => Expr::Contains(Box::new(first), Box::new(second), line, col),
                            "starts_with" => Expr::StartsWith(Box::new(first), Box::new(second)),
                            _ => Expr::EndsWith(Box::new(first), Box::new(second)),
                        }
                    } else {
                        self.take(Kind::Comma)?;
                        let (second, second_ty) = self.expression(0, false)?;
                        if !matches!(second_ty, Type::String | Type::StrView) { return Err(self.fail("write_file expects a string or string view content")); }
                        self.take(Kind::RParen)?;
                        Expr::WriteFile(Box::new(first), Box::new(second), line, col)
                    };
                    let ty = match name.as_str() { "write_file" => Type::Int, "bytes" => Type::Vec, "contains" | "starts_with" | "ends_with" => Type::Bool, "split" => Type::StrVec, _ => Type::String };
                    (builtin, ty)
                } else if !self.names.contains_key(&name) && self.generics.fn_names.contains_key(&name) {
                    // Generic call: the type arguments choose the body, so they
                    // are mandatory — the language has no type inference.
                    let gid = self.generics.fn_names[&name];
                    if *self.current() != Kind::LBracket {
                        return Err(error(&self.file, self.source, name_offset, &format!("'{name}' is a generic function; write its type arguments, e.g. {name}[i64](...)")));
                    }
                    self.pos += 1;
                    let mut type_args = Vec::new();
                    loop {
                        if type_args.len() >= 8 { return Err(self.fail("type argument limit exceeded (8)")); }
                        type_args.push(self.named_type()?);
                        if *self.current() != Kind::Comma { break; }
                        self.pos += 1;
                    }
                    self.take(Kind::RBracket)?;
                    let (id, types, ret) = self.instantiate_fn(gid, type_args, name_offset)?;
                    let args = self.call_arguments(&name, &types)?;
                    (Expr::Call(id, args, ret), ret)
                } else if *self.current() == Kind::LParen {
                    let signature = self.functions.get(&name).cloned().ok_or_else(|| error(&self.file, self.source, name_offset, &format!("unknown function '{name}'")))?;
                    if !signature.public && signature.file != PRELUDE && signature.file != self.file {
                        return Err(error(&self.file, self.source, name_offset, &format!("function '{name}' is private to {} (mark it 'pub' to export it)", signature.file)));
                    }
                    let (id, types, ret) = (signature.id, signature.params.clone(), signature.ret);
                    let args = self.call_arguments(&name, &types)?;
                    (Expr::Call(id, args, ret), ret)
                } else {
                    let (id, _, ty) = *self.names.get(&name).ok_or_else(|| {
                        // A generic enum used as a value: the arguments are
                        // mandatory, so say that instead of "unknown variable".
                        if self.generics.names.contains_key(&name) {
                            return error(&self.file, self.source, name_offset, &format!("'{name}' is a generic enum; write '{name}[string]::Variant'"));
                        }
                        error(&self.file, self.source, name_offset, &format!("unknown variable '{name}'"))
                    })?;
                    if self.moved.contains(&id) { return Err(error(&self.file, self.source, name_offset, &format!("cannot use '{name}' because it was moved"))); }
                    if let Type::Array(len) = ty {
                        let (line, col) = self.position(); self.take(Kind::LBracket)?;
                        let (index, index_ty) = self.expression(0, false)?;
                        if index_ty != Type::Int { return Err(self.fail("array index must be an integer")); }
                        self.take(Kind::RBracket)?;
                        (Expr::Index(id, len, Box::new(index), line, col), Type::Int)
                    } else if (ty == Type::StrVec || ty == Type::StrVecRef) && *self.current() == Kind::LBracket {
                        // Reading an element borrows it; only mutation is
                        // rejected while a borrow is live (see the store paths).
                        let (line, col) = self.position(); self.take(Kind::LBracket)?;
                        let (index, index_ty) = self.expression(0, false)?;
                        if index_ty != Type::Int { return Err(self.fail("vector index must be an integer")); }
                        self.take(Kind::RBracket)?;
                        (Expr::StrVecIndex(id, Box::new(index), line, col, ty == Type::StrVecRef), Type::StrView)
                    } else if (ty == Type::Vec || ty == Type::VecRef) && *self.current() == Kind::LBracket {
                        // Vec variables may also be used bare (moves, len);
                        // indexing is optional. Borrowed vectors are read-only.
                        let (line, col) = self.position(); self.take(Kind::LBracket)?;
                        let (index, index_ty) = self.expression(0, false)?;
                        if index_ty != Type::Int { return Err(self.fail("vector index must be an integer")); }
                        self.take(Kind::RBracket)?;
                        (Expr::VecIndex(id, Box::new(index), line, col, ty == Type::VecRef), Type::Int)
                    } else {
                        (Expr::Variable(id, ty, vline, vcol), ty)
                    }
                }
            }
            Kind::LParen => { self.pos += 1; let inner = self.expression(0, moving)?; self.take(Kind::RParen)?; inner }
            _ => return Err(self.fail("expected expression")),
        };
        // Postfix field access binds tighter than every operator.
        loop {
            if *self.current() != Kind::Dot { break; }
            self.pos += 1;
            let fname = self.identifier()?;
            let (base, base_ty) = lhs;
            let sid = match base_ty { Type::Struct(sid) => sid, other => return Err(self.fail(&format!("type {} has no fields", other.name()))) };
            let fty = {
                let def = &self.structs[sid];
                let fidx = def.fields.iter().position(|(n, _)| *n == fname)
                    .ok_or_else(|| self.fail(&format!("struct '{}' has no field '{}'", def.name, fname)))?;
                def.fields[fidx].1
            };
            lhs = (Expr::Field(Box::new(base), fname, fty), fty);
        }
        // Move bookkeeping happens once the full postfix chain is known:
        // bare owned variables move; owned fields are borrows and can never
        // be moved out of their struct.
        if moving {
            let move_target = match (&lhs.0, lhs.1) {
                (_, ty) if !ty.owned() => None,
                (Expr::Variable(id, _, _, _), _) => {
                    if self.borrows.iter().any(|&(bid, _)| bid == *id) {
                        return Err(self.fail("cannot move a value while it is borrowed"));
                    }
                    Some(MoveKind::Var(*id))
                }
                (Expr::Field(_, _, fty), _) if fty.owned() => Some(MoveKind::Field),
                (Expr::StrVecIndex(_, _, _, _, _), _) => Some(MoveKind::StrVecElement),
                _ => None,
            };
            match move_target {
                Some(MoveKind::Var(id)) => { if !self.moved.contains(&id) { self.moved.insert(id); } }
                Some(MoveKind::Field) => return Err(self.fail("cannot move a field out of a struct; use it where a borrow works")),
                Some(MoveKind::StrVecElement) => return Err(self.fail("cannot move an element out of a vector; use it where a borrow works")),
                None => {}
            }
        }
        loop {
            let (prec, op) = match self.current() {
                Kind::Or => (1u8, Kind::Or), Kind::And => (2, Kind::And),
                Kind::EqEq => (3, Kind::EqEq), Kind::NotEq => (3, Kind::NotEq),
                Kind::Less => (3, Kind::Less), Kind::LessEq => (3, Kind::LessEq),
                Kind::Greater => (3, Kind::Greater), Kind::GreaterEq => (3, Kind::GreaterEq),
                Kind::Plus => (4, Kind::Plus), Kind::Minus => (4, Kind::Minus),
                Kind::Star => (5, Kind::Star), Kind::Slash => (5, Kind::Slash), Kind::Percent => (5, Kind::Percent),
                _ => break,
            };
            if prec < min { break; }
            let (line, column) = self.position();
            self.pos += 1;
            // `+` always consumes its string operands, even in borrow contexts.
            let (rhs, rhs_ty) = self.expression(prec + 1, op == Kind::Plus)?;
            let (lhs_expr, lhs_ty) = lhs;
            let ty = match op {
                Kind::And | Kind::Or => {
                    if lhs_ty != Type::Bool || rhs_ty != Type::Bool { return Err(self.fail(&format!("'{}' requires boolean operands", if op == Kind::And { "&&" } else { "||" }))); }
                    Type::Bool
                }
                Kind::EqEq | Kind::NotEq | Kind::Less | Kind::LessEq | Kind::Greater | Kind::GreaterEq => {
                    let comparable = match (lhs_ty, rhs_ty) {
                        (Type::Int, Type::Int) => true,
                        (Type::F64, Type::F64) => true,
                        (Type::Bool, Type::Bool) => op == Kind::EqEq || op == Kind::NotEq,
                        (Type::String | Type::StrView, Type::String | Type::StrView) => op == Kind::EqEq || op == Kind::NotEq,
                        _ => false,
                    };
                    if !comparable {
                        return Err(self.fail("comparison requires two i64, two f64, or two strings/views with == or !="));
                    }
                    // String/view equality gets a dedicated node so emission
                    // never confuses it with C pointer comparison.
                    if (op == Kind::EqEq || op == Kind::NotEq) && matches!(lhs_ty, Type::String | Type::StrView) {
                        lhs = (Expr::StrEq(Box::new(lhs_expr), Box::new(rhs), op == Kind::NotEq), Type::Bool);
                        continue;
                    }
                    Type::Bool
                }
                Kind::Plus if matches!(lhs_ty, Type::String | Type::StrView) || matches!(rhs_ty, Type::String | Type::StrView) => {
                    // `+` builds a new owned string. Two views are read only and
                    // copied into the result, so no view can be consumed.
                    let stringlike = |ty: Type| matches!(ty, Type::String | Type::StrView);
                    if !stringlike(lhs_ty) || !stringlike(rhs_ty) { return Err(self.fail("'+' requires two strings, two string views, or two integers")); }
                    // The left operand was parsed in the caller's mode; using it
                    // here consumes it, so retroactively record the move. Only a
                    // plain owned variable is consumed: fields, vector elements
                    // and views are borrowed and get copied by the emitter.
                    if lhs_ty == Type::String {
                        if let Expr::Variable(id, ty, _, _) = &lhs_expr {
                            if ty.owned() && !self.moved.contains(id) { self.moved.insert(*id); }
                        }
                    }
                    Type::String
                }
                _ => {
                    if lhs_ty == Type::F64 || rhs_ty == Type::F64 {
                        if lhs_ty != Type::F64 || rhs_ty != Type::F64 {
                            return Err(self.fail("mixed i64/f64 arithmetic is not allowed; convert one side explicitly with to_f64 or to_i64"));
                        }
                        if op == Kind::Percent {
                            return Err(self.fail("'%' is defined for i64 operands only"));
                        }
                        Type::F64
                    } else {
                        if lhs_ty != Type::Int || rhs_ty != Type::Int { return Err(self.fail("arithmetic requires integer operands")); }
                        Type::Int
                    }
                }
            };
            lhs = (Expr::Binary(Box::new(lhs_expr), op, Box::new(rhs), line, column, ty), ty);
        }
        Ok(lhs)
    }
    fn named_type(&mut self) -> Result<Type, String> {
        let name = self.identifier()?;
        // Inside a generic instantiation a type parameter stands for the
        // argument the call site supplied; nothing must reach code generation
        // with a parameter still in it.
        if let Some(&concrete) = self.subst.get(&name) { return Ok(concrete); }
        match name.as_str() {
            "i64" => Ok(Type::Int),
            "f64" => Ok(Type::F64),
            "bool" => Ok(Type::Bool),
            "string" => Ok(Type::String),
            "vec" => {
                // `vec` alone is the i64 vector; `vec[string]` names the other
                // one, so a type annotation can say which it means instead of
                // leaving owned string vectors unnameable.
                if *self.current() != Kind::LBracket { return Ok(Type::Vec); }
                self.pos += 1;
                if self.identifier()? != "string" { return Err(self.fail("only vec[string] names an element type; vector elements are i64 by default")); }
                self.take(Kind::RBracket)?;
                Ok(Type::StrVec)
            }
            "map" => {
                // Only one shape exists, but the shape is spelled out so the
                // code says what it means and the parser can grow later.
                self.take(Kind::LBracket)?;
                if self.identifier()? != "string" { return Err(self.fail("map keys must be string")); }
                self.take(Kind::Comma)?;
                if self.identifier()? != "i64" { return Err(self.fail("map values must be i64")); }
                self.take(Kind::RBracket)?;
                Ok(Type::Map)
            }
            other => {
                if let Some(&gid) = self.generics.names.get(other) {
                    if *self.current() != Kind::LBracket {
                        return Err(self.fail(&format!("'{other}' is a generic enum; write its type arguments, e.g. {other}[string]")));
                    }
                    let offset = self.tokens[self.pos].offset;
                    self.pos += 1;
                    let mut args = Vec::new();
                    loop {
                        args.push(self.type_argument()?);
                        if *self.current() != Kind::Comma { break; }
                        self.pos += 1;
                    }
                    self.take(Kind::RBracket)?;
                    let eid = self.instantiate_enum(gid, args, offset)?;
                    let def = &self.enums[eid];
                    self.require_public("enum", def.name, &def.file.clone(), def.public)?;
                    return Ok(Type::Enum(def.name, eid));
                }
                if let Some(&gid) = self.generics.struct_names.get(other) {
                    if *self.current() != Kind::LBracket {
                        return Err(self.fail(&format!("'{other}' is a generic struct; write its type arguments, e.g. {other}[string]")));
                    }
                    let offset = self.tokens[self.pos].offset;
                    self.pos += 1;
                    let mut args = Vec::new();
                    loop {
                        args.push(self.named_type()?);
                        if *self.current() != Kind::Comma { break; }
                        self.pos += 1;
                    }
                    self.take(Kind::RBracket)?;
                    let sid = self.instantiate_struct(gid, args, offset)?;
                    let def = &self.structs[sid];
                    self.require_public("struct", &def.name.clone(), &def.file.clone(), def.public)?;
                    return Ok(Type::Struct(sid));
                }
                match self.struct_names.get(other) {
                Some(&id) => {
                    let def = &self.structs[id];
                    self.require_public("struct", &def.name.clone(), &def.file.clone(), def.public)?;
                    Ok(Type::Struct(id))
                }
                None => match self.enum_names.get(other) {
                    Some(&id) => {
                        let (name, file, public) = (self.enums[id].name, self.enums[id].file.clone(), self.enums[id].public);
                        self.require_public("enum", name, &file, public)?;
                        Ok(Type::Enum(name, id))
                    }
                    None => Err(self.fail(&format!("unknown type '{}'", other))),
                },
                }
            }
        }
    }
    // `::Variant(payload, ...)` once the enum type is known. Payload values are
    // moved in, so a variant owns whatever it carries.
    fn enum_literal(&mut self, eid: usize, offset: usize) -> Result<(Expr, Type), String> {
        self.take(Kind::ColonColon)?;
        let vname = self.identifier()?;
        let def = self.enums[eid].clone();
        self.require_public("enum", def.name, &def.file, def.public)?;
        let vidx = def.variants.iter().position(|(n, _)| *n == vname)
            .ok_or_else(|| error(&self.file, self.source, offset, &format!("enum '{}' has no variant '{}'", def.name, vname)))?;
        let fields = def.variants[vidx].1.clone();
        let mut values = Vec::new();
        if *self.current() == Kind::LParen {
            self.pos += 1;
            for (index, field) in fields.iter().enumerate() {
                // String payloads are moved into the variant.
                let (value, value_ty) = self.expression(0, *field == Type::String)?;
                if value_ty != *field {
                    return Err(self.fail(&format!("payload {} of '{}::{}' must be {}", index + 1, def.name, vname, field.name())));
                }
                values.push(value);
                if index + 1 < fields.len() { self.take(Kind::Comma)?; }
            }
            if *self.current() == Kind::Comma {
                return Err(self.fail(&format!("variant '{}::{}' takes {} payload value(s)", def.name, vname, fields.len())));
            }
            self.take(Kind::RParen)?;
        } else if !fields.is_empty() {
            return Err(self.fail(&format!("variant '{}::{}' needs {} payload value(s)", def.name, vname, fields.len())));
        }
        Ok((Expr::EnumLit(def.name, eid, vidx, values), Type::Enum(def.name, eid)))
    }
    // `Name { field: expr, ... }` once the layout is known. Fields may appear
    // in any order but each exactly once; owned fields are moved in.
    fn struct_literal(&mut self, sid: usize, offset: usize) -> Result<(Expr, Type), String> {
        let name = self.structs[sid].name.clone();
        self.take(Kind::LBrace)?;
        let def_fields = self.structs[sid].fields.clone();
        let mut slots: Vec<Option<Expr>> = (0..def_fields.len()).map(|_| None).collect();
        let mut seen = std::collections::HashSet::new();
        while *self.current() != Kind::RBrace {
            let fname = self.identifier()?;
            let fidx = def_fields.iter().position(|(n, _)| *n == fname)
                .ok_or_else(|| self.fail(&format!("struct '{}' has no field '{}'", name, fname)))?;
            if !seen.insert(fname.clone()) { return Err(self.fail(&format!("field '{}' specified twice", fname))); }
            self.take(Kind::Colon)?;
            let (value, vty) = self.expression(0, true)?;
            if vty != def_fields[fidx].1 { return Err(self.fail(&format!("field '{}' must be {}", fname, def_fields[fidx].1.name()))); }
            slots[fidx] = Some(value);
            if *self.current() == Kind::Comma { self.pos += 1; }
        }
        self.take(Kind::RBrace)?;
        let mut values = Vec::new();
        for (i, slot) in slots.into_iter().enumerate() {
            match slot {
                Some(v) => values.push(v),
                None => return Err(error(&self.file, self.source, offset, &format!("missing field '{}' in '{}' construction", def_fields[i].0, name))),
            }
        }
        Ok((Expr::StructLit(sid, values), Type::Struct(sid)))
    }
    // `enum Name { ... }` or `enum Name[T, U] { ... }`. A generic declaration
    // produces no layout at all; only instantiations do (ADR-21).
    fn parse_enum_def(&mut self, public: bool) -> Result<(), String> {
        self.pos += 1;
        let name = self.identifier()?;
        if RESERVED.contains(&name.as_str()) { return Err(self.fail(&format!("'{}' is a built-in name and cannot be an enum", name))); }
        if self.struct_names.contains_key(&name) || self.generics.struct_names.contains_key(&name) { return Err(self.fail(&format!("'{}' is already a struct", name))); }
        if self.enum_names.contains_key(&name) || self.generics.names.contains_key(&name) { return Err(self.fail(&format!("duplicate enum '{}'", name))); }
        let params = self.type_params()?;
        self.take(Kind::LBrace)?;
        let mut variants: Vec<(String, Vec<Payload>)> = Vec::new();
        let mut seen = std::collections::HashSet::new();
        while *self.current() != Kind::RBrace {
            let vname = self.identifier()?;
            if !seen.insert(vname.clone()) { return Err(self.fail(&format!("duplicate variant '{}' in enum '{}'", vname, name))); }
            let mut fields = Vec::new();
            if *self.current() == Kind::LParen {
                self.pos += 1;
                if *self.current() != Kind::RParen {
                    loop {
                        if fields.len() >= 16 { return Err(self.fail("variant payload limit exceeded (16)")); }
                        let fty = self.variant_payload(&params)?;
                        fields.push(fty);
                        if *self.current() != Kind::Comma { break; }
                        self.pos += 1;
                    }
                }
                self.take(Kind::RParen)?;
            }
            variants.push((vname, fields));
            if *self.current() == Kind::Comma { self.pos += 1; }
        }
        self.take(Kind::RBrace)?;
        if variants.is_empty() { return Err(self.fail(&format!("enum '{}' needs at least one variant", name))); }
        if variants.len() > 256 { return Err(self.fail("enum variant limit exceeded (256)")); }
        if params.is_empty() {
            let id = self.enums.len();
            // Type names are interned once per declaration so Type stays Copy and
            // diagnostics can print the declared name.
            let interned: &'static str = Box::leak(name.clone().into_boxed_str());
            self.enum_names.insert(name.clone(), id);
            let concrete = variants.into_iter().map(|(vname, fields)| (vname, fields.into_iter().map(|p| match p {
                Payload::Concrete(ty) => ty,
                Payload::Param(_) => unreachable!("a non-generic enum has no type parameters"),
            }).collect())).collect();
            self.enums.push(EnumDef { name: interned, variants: concrete, file: self.file.clone(), public, pattern: name });
        } else {
            let gid = self.generics.defs.len();
            self.generics.names.insert(name.clone(), gid);
            self.generics.defs.push(GenericEnum { name, params, variants, file: self.file.clone(), public });
        }
        Ok(())
    }
    // `[T, U]` after a declaration name; empty for a plain definition. Single
    // uppercase letters keep a parameter from being mistaken for a real type.
    // `[T, U]` or `[T: Add + Ord, U: Copy]`. Constraints are parsed here for
    // every generic declaration, but only generic functions may use them.
    fn type_params_with_bounds(&mut self) -> Result<Vec<(String, Vec<Bound>)>, String> {
        let mut params: Vec<(String, Vec<Bound>)> = Vec::new();
        if *self.current() != Kind::LBracket { return Ok(params); }
        self.pos += 1;
        loop {
            if params.len() >= 8 { return Err(self.fail("type parameter limit exceeded (8)")); }
            let param = self.identifier()?;
            if param.len() != 1 || !param.chars().all(|c| c.is_ascii_uppercase()) { return Err(self.fail("type parameters are single uppercase letters, e.g. 'T'")); }
            if params.iter().any(|(n, _)| n == &param) { return Err(self.fail(&format!("duplicate type parameter '{param}'"))); }
            let mut bounds = Vec::new();
            if *self.current() == Kind::Colon {
                self.pos += 1;
                loop {
                    let at = self.tokens[self.pos].offset;
                    let label = self.identifier()?;
                    let bound = match label.as_str() {
                        "Add" => Bound::Add, "Eq" => Bound::Eq,
                        "Ord" => Bound::Ord, "Copy" => Bound::Copy,
                        _ => return Err(error(&self.file, self.source, at, "unknown constraint; allowed: Add, Eq, Ord, Copy")),
                    };
                    if bounds.contains(&bound) { return Err(self.fail(&format!("duplicate constraint '{label}'"))); }
                    bounds.push(bound);
                    if *self.current() != Kind::Plus { break; }
                    self.pos += 1;
                }
            }
            params.push((param, bounds));
            if *self.current() != Kind::Comma { break; }
            self.pos += 1;
        }
        self.take(Kind::RBracket)?;
        Ok(params)
    }
    fn type_params(&mut self) -> Result<Vec<String>, String> {
        let params = self.type_params_with_bounds()?;
        if params.iter().any(|(_, bounds)| !bounds.is_empty()) {
            return Err(self.fail("constraints are only supported on generic functions"));
        }
        Ok(params.into_iter().map(|(name, _)| name).collect())
    }
    fn variant_payload(&mut self, params: &[String]) -> Result<Payload, String> {
        if let Kind::Ident(name) = self.current() {
            if let Some(index) = params.iter().position(|p| p == name) {
                self.pos += 1;
                return Ok(Payload::Param(index));
            }
        }
        let fty = self.named_type()?;
        // Payloads stay flat: a variant may carry scalars, strings or a vector,
        // but not a struct or another enum (no layout recursion).
        if !matches!(fty, Type::Int | Type::Bool | Type::F64 | Type::String | Type::Vec | Type::StrVec) {
            return Err(self.fail("enum variant payloads must be i64, bool, f64, string, vec, vec[string], or a type parameter"));
        }
        Ok(Payload::Concrete(fty))
    }
    // One C layout per (declaration, argument) pair, generated on first use. The
    // budget bounds code growth for a program that instantiates endlessly.
    fn instantiate_enum(&mut self, gid: usize, args: Vec<Type>, offset: usize) -> Result<usize, String> {
        let def = self.generics.defs[gid].clone();
        if args.len() != def.params.len() {
            return Err(error(&self.file, self.source, offset, &format!("'{}' takes {} type argument(s), found {}", def.name, def.params.len(), args.len())));
        }
        let key = format!("{gid}:{}", args.iter().map(|t| t.name()).collect::<Vec<_>>().join(","));
        if let Some(&eid) = self.generics.done.get(&key) { return Ok(eid); }
        if self.enums.len() >= MAX_ENUM_TYPES {
            return Err(error(&self.file, self.source, offset, &format!("generic instantiation budget exceeded ({MAX_ENUM_TYPES} enum types) at '{}'", def.name)));
        }
        let variants: Vec<(String, Vec<Type>)> = def.variants.iter().map(|(vname, fields)| {
            let types = fields.iter().map(|p| match p {
                Payload::Concrete(ty) => *ty,
                Payload::Param(index) => args[*index],
            }).collect();
            (vname.clone(), types)
        }).collect();
        let display = format!("{}[{}]", def.name, args.iter().map(|t| t.name()).collect::<Vec<_>>().join(", "));
        let interned: &'static str = Box::leak(display.into_boxed_str());
        let eid = self.enums.len();
        self.enums.push(EnumDef { name: interned, variants, file: def.file, public: def.public, pattern: def.name });
        self.generics.done.insert(key, eid);
        Ok(eid)
    }
    // A concrete type argument. Structs and enums are rejected for the same
    // reason variant payloads are: no recursion and no hidden layout.
    fn type_argument(&mut self) -> Result<Type, String> {
        let ty = self.named_type()?;
        if !matches!(ty, Type::Int | Type::Bool | Type::F64 | Type::String | Type::Vec | Type::StrVec) {
            return Err(self.fail("type arguments must be i64, bool, f64, string, vec, or vec[string]"));
        }
        Ok(ty)
    }
    // A field type in a generic declaration: a type parameter, or any named
    // type (which may itself be an instantiation such as `Option[string]`).
    fn field_type(&mut self, params: &[String]) -> Result<Payload, String> {
        if let Kind::Ident(name) = self.current() {
            if let Some(index) = params.iter().position(|p| p == name) {
                self.pos += 1;
                return Ok(Payload::Param(index));
            }
        }
        Ok(Payload::Concrete(self.named_type()?))
    }
    // One C layout per (declaration, argument) pair, generated on first use.
    fn instantiate_struct(&mut self, gid: usize, args: Vec<Type>, offset: usize) -> Result<usize, String> {
        let def = self.generics.struct_defs[gid].clone();
        if args.len() != def.params.len() {
            return Err(error(&self.file, self.source, offset, &format!("'{}' takes {} type argument(s), found {}", def.name, def.params.len(), args.len())));
        }
        let key = format!("{gid}:{}", args.iter().map(|t| t.name()).collect::<Vec<_>>().join(","));
        if let Some(&sid) = self.generics.struct_done.get(&key) { return Ok(sid); }
        if self.structs.len() >= MAX_STRUCT_TYPES {
            return Err(error(&self.file, self.source, offset, &format!("generic instantiation budget exceeded ({MAX_STRUCT_TYPES} struct types) at '{}'", def.name)));
        }
        let fields: Vec<(String, Type)> = def.fields.iter().map(|(fname, payload)| {
            let ty = match payload {
                Payload::Concrete(ty) => *ty,
                Payload::Param(index) => args[*index],
            };
            (fname.clone(), ty)
        }).collect();
        let display = format!("{}[{}]", def.name, args.iter().map(|t| t.name()).collect::<Vec<_>>().join(", "));
        let sid = self.structs.len();
        self.structs.push(StructDef { name: display, fields, file: def.file, public: def.public });
        self.generics.struct_done.insert(key, sid);
        Ok(sid)
    }
    fn parse_struct_def(&mut self, public: bool) -> Result<(), String> {
        self.pos += 1;
        let name = self.identifier()?;
        if RESERVED.contains(&name.as_str()) { return Err(self.fail(&format!("'{}' is a built-in name and cannot be a struct", name))); }
        if self.struct_names.contains_key(&name) || self.generics.struct_names.contains_key(&name) { return Err(self.fail(&format!("duplicate struct '{}'", name))); }
        if self.enum_names.contains_key(&name) || self.generics.names.contains_key(&name) { return Err(self.fail(&format!("'{}' is already an enum", name))); }
        let params = self.type_params()?;
        self.take(Kind::LBrace)?;
        let mut fields: Vec<(String, Payload)> = Vec::new();
        let mut seen = std::collections::HashSet::new();
        while *self.current() != Kind::RBrace {
            let fname = self.identifier()?;
            if !seen.insert(fname.clone()) { return Err(self.fail(&format!("duplicate field '{}' in struct '{}'", fname, name))); }
            self.take(Kind::Colon)?;
            let fty = self.field_type(&params)?;
            fields.push((fname, fty));
            if *self.current() == Kind::Comma { self.pos += 1; }
        }
        self.take(Kind::RBrace)?;
        if params.is_empty() {
            let id = self.structs.len();
            self.struct_names.insert(name.clone(), id);
            let concrete = fields.into_iter().map(|(fname, payload)| (fname, match payload {
                Payload::Concrete(ty) => ty,
                Payload::Param(_) => unreachable!("a non-generic struct has no type parameters"),
            })).collect();
            self.structs.push(StructDef { name, fields: concrete, file: self.file.clone(), public });
        } else {
            let gid = self.generics.struct_defs.len();
            self.generics.struct_names.insert(name.clone(), gid);
            self.generics.struct_defs.push(GenericStruct { name, params, fields, file: self.file.clone(), public });
        }
        Ok(())
    }
    // Arrays are borrowed read-only; strings and scalars pass by value.
    fn parameter_type(&mut self) -> Result<Type, String> {
        if *self.current() == Kind::Amp {
            let amp_offset = self.tokens[self.pos].offset;
            self.pos += 1;
            // `&T` is resolved when a generic function is instantiated: the
            // type parameter already stands for one concrete type, so this is
            // just a spelling for one of the three borrows below. No lifetime
            // or new borrow kind is introduced (ADR-35).
            let parameter: Option<(String, Type)> = match self.current() {
                Kind::Ident(name) => self.subst.get(name).map(|&ty| (name.clone(), ty)),
                _ => None,
            };
            if let Some((name, concrete)) = parameter {
                self.pos += 1;
                return match concrete {
                    Type::String => Ok(Type::StrView),
                    Type::Vec => Ok(Type::VecRef),
                    Type::StrVec => Ok(Type::StrVecRef),
                    _ => Err(error(&self.file, self.source, amp_offset, &format!(
                        "'&{name}' is '{}' here, which has no read-only borrow; only string, vec and vec[string] can be borrowed",
                        concrete.name()))),
                };
            }
            if matches!(self.current(), Kind::Ident(name) if name == "string") {
                // &string borrows a string read-only for the duration of the
                // call: no move, no copy, and it binds as an ordinary view.
                self.pos += 1;
                return Ok(Type::StrView);
            }
            if matches!(self.current(), Kind::Ident(name) if name == "vec") {
                self.pos += 1;
                // &vec[string] borrows a string vector read-only.
                if *self.current() == Kind::LBracket {
                    self.pos += 1;
                    if self.identifier()? != "string" { return Err(self.fail("only &vec[string] is supported; vector elements are i64 by default")); }
                    self.take(Kind::RBracket)?;
                    return Ok(Type::StrVecRef);
                }
                return Ok(Type::VecRef);
            }
            if matches!(self.current(), Kind::Ident(_)) {
                return Err(error(&self.file, self.source, amp_offset,
                    "unsupported borrow type; write &string, &vec, &vec[string], &[i64; N], or &T inside a generic function"));
            }
            self.take(Kind::LBracket)?;
            if self.identifier()? != "i64" { return Err(self.fail("array element type must be i64")); }
            self.take(Kind::Semi)?;
            let len = match self.current() {
                Kind::Number(n) if *n >= 1 && *n <= 4096 => *n as usize,
                _ => return Err(self.fail("array parameter length must be between 1 and 4096")),
            };
            self.pos += 1; self.take(Kind::RBracket)?;
            Ok(Type::Array(len))
        } else {
            self.named_type()
        }
    }
    fn return_annotation(&mut self) -> Result<Type, String> {
        self.named_type()
    }
    // First pass collects only signatures. Bodies are checked in the second pass.
    // scan_uses validated the headers; skip them before collecting.
    fn skip_uses(&mut self) {
        while *self.current() == Kind::Use { self.pos += 3; }
    }
    // `pub` marks an item as importable from other files. It is consumed by the
    // declaration passes, never by expression parsing.
    fn take_pub(&mut self) -> bool {
        let is_pub = matches!(self.current(), Kind::Ident(name) if name == "pub");
        if is_pub { self.pos += 1; }
        is_pub
    }
    // Cross-file access check: an item defined in another file must be exported.
    // Prelude names are public by construction, and a file always sees its own.
    fn check_export_types(&self, name: &str, params: &[Type], ret: Type) -> Result<(), String> {
        for ty in params.iter().copied().chain(std::iter::once(ret)) {
            let (kind, type_name, public) = match ty {
                Type::Struct(id) => ("struct", self.structs[id].name.clone(), self.structs[id].public),
                Type::Enum(_, id) => ("enum", self.enums[id].name.to_owned(), self.enums[id].public),
                _ => continue,
            };
            if !public { return Err(self.fail(&format!("public function '{name}' exposes private {kind} '{type_name}'"))); }
        }
        Ok(())
    }
    fn require_public(&self, kind: &str, name: &str, file: &str, public: bool) -> Result<(), String> {
        if public || file == PRELUDE || file == self.file { return Ok(()); }
        Err(self.fail(&format!("{kind} '{name}' is private to {file} (mark it 'pub' to export it)")))
    }
    // Second pass skips struct bodies (already collected, must not re-register).
    fn skip_struct_def(&mut self) {
        self.pos += 2; // 'struct' Name
        self.skip_type_params();
        self.pos += 1; // the opening brace
        let mut depth = 1usize;
        while depth > 0 {
            match self.current() {
                Kind::LBrace => depth += 1,
                Kind::RBrace => depth -= 1,
                Kind::Eof => return,
                _ => {}
            }
            self.pos += 1;
        }
    }
    // Enum bodies are collected in the first pass, skipped in the second.
    // Step over `[T, U]` when a declaration is skipped in the second pass.
    fn skip_type_params(&mut self) {
        if *self.current() != Kind::LBracket { return; }
        let mut depth = 0usize;
        while *self.current() != Kind::Eof {
            match self.current() {
                Kind::LBracket => depth += 1,
                Kind::RBracket => { depth -= 1; if depth == 0 { self.pos += 1; return; } }
                _ => {}
            }
            self.pos += 1;
        }
    }
    // Skip `(params) -> Ret { body }` without interpreting it. Used for a
    // generic declaration, whose body is parsed at each instantiation instead.
    fn skip_function_body(&mut self) -> Result<(), String> {
        while *self.current() != Kind::LBrace {
            if *self.current() == Kind::Eof { return Err(self.fail("expected '{' before end of file")); }
            self.pos += 1;
        }
        self.pos += 1;
        let mut depth = 1usize;
        while depth > 0 {
            match self.current() {
                Kind::LBrace => depth += 1,
                Kind::RBrace => depth -= 1,
                Kind::Eof => return Err(self.fail("expected '}' before end of file")),
                _ => {}
            }
            self.pos += 1;
        }
        Ok(())
    }
    fn skip_enum_def(&mut self) {
        self.pos += 2; // 'enum' Name
        self.skip_type_params();
        self.pos += 1; // the opening brace
        let mut depth = 1usize;
        while depth > 0 {
            match self.current() {
                Kind::LBrace => depth += 1,
                Kind::RBrace => depth -= 1,
                Kind::Eof => return,
                _ => {}
            }
            self.pos += 1;
        }
    }
    fn collect_functions(&mut self, next_id: &mut usize) -> Result<(), String> {
        let mut seen = std::collections::HashSet::new();
        while *self.current() != Kind::Eof {
            let public = self.take_pub();
            if *self.current() == Kind::Struct { self.parse_struct_def(public)?; continue; }
            if *self.current() == Kind::Enum { self.parse_enum_def(public)?; continue; }
            self.take(Kind::Fn)?;
            let name = self.identifier()?;
            if !seen.insert(name.clone()) { return Err(self.fail(&format!("duplicate function '{name}'"))); }
            if *self.current() == Kind::LBracket {
                // A generic function has no signature of its own: only its
                // instantiations exist, and each gets an id when constructed.
                if name == "main" { return Err(self.fail("main cannot be generic")); }
                let declared = self.type_params_with_bounds()?;
                let params = declared.iter().map(|(n, _)| n.clone()).collect();
                let bounds = declared.into_iter().map(|(_, b)| b).collect();
                let gid = self.generics.fn_defs.len();
                self.generics.fn_names.insert(name.clone(), gid);
                self.generics.fn_defs.push(GenericFn { name, params, bounds, start: self.pos, tokens: self.tokens.clone(), source: self.source.to_owned(), file: self.file.clone(), public });
                self.skip_function_body()?;
                continue;
            }
            self.take(Kind::LParen)?;
            let mut types = Vec::new();
            if *self.current() != Kind::RParen {
                loop {
                    if types.len() >= 64 { return Err(self.fail("function parameter limit exceeded (64)")); }
                    if *self.current() == Kind::Var { self.pos += 1; }
                    let parameter = self.identifier()?;
                    if RESERVED.contains(&parameter.as_str()) { return Err(self.fail(&format!("'{parameter}' is a built-in name and cannot be a parameter"))); }
                    self.take(Kind::Colon)?;
                    types.push(self.parameter_type()?);
                    if *self.current() != Kind::Comma { break; }
                    self.pos += 1;
                }
            }
            self.take(Kind::RParen)?;
            if RESERVED.contains(&name.as_str()) { return Err(self.fail(&format!("'{name}' is a built-in and cannot be redefined"))); }
            // Ids are unique across all files of a program, and share one
            // space with generic instantiations, so keep the counter in the
            // shared state and fold the per-file value in.
            self.generics.next_fn_id = self.generics.next_fn_id.max(*next_id);
            let id = self.generics.next_fn_id;
            self.generics.next_fn_id += 1;
            *next_id = self.generics.next_fn_id;
            self.id_order.push(id);
            if name == "main" {
                if !types.is_empty() { return Err(self.fail("main cannot have parameters")); }
            } else {
                self.take(Kind::Arrow)?;
                let ret = self.return_annotation()?;
                // A public signature may not name a type importers cannot see;
                // catching it here beats a puzzling error at every call site.
                if public { self.check_export_types(&name, &types, ret)?; }
                let signature = Signature { id, params: types, ret, file: self.file.clone(), public };
                if self.functions.insert(name.clone(), signature).is_some() {
                    return Err(self.fail(&format!("duplicate function '{name}' (already defined in an imported file)")));
                }
            }
            self.take(Kind::LBrace)?;
            let mut depth = 1usize;
            while depth > 0 {
                match self.current() {
                    Kind::LBrace => depth += 1,
                    Kind::RBrace => depth -= 1,
                    Kind::Eof => return Err(self.fail("expected '}' before end of file")),
                    _ => {}
                }
                self.pos += 1;
            }
        }
        // main presence is checked once per program, not per file.
        self.pos = 0;
        self.skip_uses();
        Ok(())
    }
    fn program(&mut self) -> Result<Vec<Function>, String> {
        let mut result = Vec::new();
        let mut main_found = false;
        while *self.current() != Kind::Eof {
            self.take_pub();
            if *self.current() == Kind::Struct { self.skip_struct_def(); continue; }
            if *self.current() == Kind::Enum { self.skip_enum_def(); continue; }
            self.take(Kind::Fn)?;
            let name = self.identifier()?;
            // A generic declaration has no body of its own: the signature pass
            // recorded it and an instantiation parses it later (ADR-31).
            if *self.current() == Kind::LBracket { self.skip_function_body()?; continue; }
            let is_main = name == "main";
            if is_main && main_found { return Err(self.fail("duplicate main function")); }
            if is_main { main_found = true; }
            let id = self.id_order[result.len()];
            result.push(self.function_rest(is_main, id)?);
        }
        // Instantiations discovered while parsing bodies have no place in the
        // file's own order, but they must still be emitted.
        result.append(&mut self.pending);
        // main presence is checked once per program, not per file.
        Ok(result)
    }
    // `(params) -> Ret { body }` for one function, starting at the parameter
    // list. Shared by the file's own functions and by generic instantiations,
    // which parse the same tokens with type parameters substituted.
    fn function_rest(&mut self, is_main: bool, id: usize) -> Result<Function, String> {
        self.take(Kind::LParen)?;
        // Variable ids restart per function, so move tracking must too.
        self.names.clear(); self.next_var = 0; self.moved.clear(); self.scopes.clear(); self.borrows.clear();
        let mut params = Vec::new();
        if *self.current() != Kind::RParen {
            loop {
                if params.len() >= 64 { return Err(self.fail("function parameter limit exceeded (64)")); }
                // `var` marks the parameter slot writable. The value was moved
                // in at the call site, so mutating this copy cannot be seen by
                // the caller; that difference is what keeps it safe.
                let mutable = *self.current() == Kind::Var;
                if mutable { self.pos += 1; }
                let param = self.identifier()?;
                if RESERVED.contains(&param.as_str()) { return Err(self.fail(&format!("'{param}' is a built-in name and cannot be a parameter"))); }
                if self.names.contains_key(&param) { return Err(self.fail(&format!("duplicate parameter '{param}'"))); }
                self.take(Kind::Colon)?;
                let ty = self.parameter_type()?;
                let var_id = self.next_var; self.next_var += 1;
                self.names.insert(param, (var_id, mutable, ty)); params.push((var_id, ty));
                if *self.current() != Kind::Comma { break; }
                self.pos += 1;
            }
        }
        self.take(Kind::RParen)?;
        let returns_type;
        if is_main {
            if !params.is_empty() { return Err(self.fail("main cannot have parameters")); }
            returns_type = Type::Int;
        } else {
            self.take(Kind::Arrow)?;
            returns_type = self.return_annotation()?;
        }
        self.in_main = is_main;
        self.return_type = returns_type;
        // Parameter scope: lives for the whole body so return paths can
        // release owned parameters.
        self.scopes.push(params.iter().map(|&(var_id, ty)| (var_id, ty)).collect());
        let body = self.block();
        let params_scope = self.scopes.pop().unwrap();
        let mut body = body?;
        let param_owned: Vec<(usize, Type)> = params_scope.iter().filter(|(_, ty)| ty.owned()).copied().collect();
        if !param_owned.is_empty() { body.push(Statement::Frees(param_owned)); }
        if !is_main && !returns(&body) { return Err(self.fail("function may exit without returning a value")); }
        Ok(Function { id, params, body, is_main, returns: returns_type })
    }
    // Parse a generic function's own tokens once per argument list. The
    // sub-parser shares this program's maps and id space, so a nested
    // instantiation behaves exactly like one written at the top level.
    fn instantiate_fn(&mut self, gid: usize, args: Vec<Type>, offset: usize) -> Result<Instance, String> {
        let def = self.generics.fn_defs[gid].clone();
        if args.len() != def.params.len() {
            return Err(error(&self.file, self.source, offset, &format!("'{}' takes {} type argument(s), found {}", def.name, def.params.len(), args.len())));
        }
        self.require_public("function", &def.name, &def.file, def.public)?;
        for (index, bounds) in def.bounds.iter().enumerate() {
            for &bound in bounds {
                if !bound.allows(args[index]) {
                    return Err(error(&self.file, self.source, offset, &format!(
                        "type argument '{}' for '{}' does not satisfy '{}'; that constraint allows {}",
                        args[index].name(), def.name, bound.name(), bound.allowed_types())));
                }
            }
        }
        let key = format!("{gid}:{}", args.iter().map(|t| t.name()).collect::<Vec<_>>().join(","));
        if let Some(instance) = self.generics.fn_done.get(&key) { return Ok(instance.clone()); }
        if self.generics.fn_in_progress.contains(&key) {
            return Err(error(&self.file, self.source, offset, &format!("'{}' is already being instantiated with these type arguments; the expansion would never finish", def.name)));
        }
        self.generics.fn_in_progress.insert(key.clone());
        let id = self.generics.next_fn_id;
        self.generics.next_fn_id += 1;
        let mut subst = HashMap::new();
        for (index, param) in def.params.iter().enumerate() { subst.insert(param.clone(), args[index]); }
        let mut sub = Parser {
            file: def.file.clone(), source: def.source.as_str(), tokens: def.tokens, pos: def.start,
            names: HashMap::new(), next_var: 0, expression_depth: 0, expression_nodes: 0, block_depth: 0,
            loop_depth: 0, loop_scopes: Vec::new(), loop_labels: Vec::new(), next_label: 0,
            functions: &mut *self.functions, structs: &mut *self.structs, struct_names: &mut *self.struct_names,
            enums: &mut *self.enums, enum_names: &mut *self.enum_names, generics: &mut *self.generics,
            id_order: Vec::new(), in_main: false, return_type: Type::Int, moved: std::collections::HashSet::new(),
            borrows: Vec::new(), scopes: Vec::new(), subst, pending: Vec::new(),
        };
        let parsed = sub.function_rest(false, id);
        let mut nested = std::mem::take(&mut sub.pending);
        drop(sub);
        let function = parsed?;
        let instance: Instance = (id, function.params.iter().map(|&(_, ty)| ty).collect(), function.returns);
        self.pending.push(function);
        self.pending.append(&mut nested);
        self.generics.fn_done.insert(key.clone(), instance.clone());
        self.generics.fn_in_progress.remove(&key);
        Ok(instance)
    }
    // The argument list of a call, starting at '(' (ADR-31 shares it between
    // ordinary calls and generic instantiations).
    fn call_arguments(&mut self, name: &str, types: &[Type]) -> Result<Vec<Argument>, String> {
        self.pos += 1;
        let mut args = Vec::new();
        if *self.current() != Kind::RParen {
            loop {
                if args.len() >= 64 { return Err(self.fail("function argument limit exceeded (64)")); }
                let expected = *types.get(args.len()).ok_or_else(|| self.fail(&format!("too many arguments to '{name}'")))?;
                if expected == Type::StrView {
                    let (value, ty) = self.expression(0, false)?;
                    if !matches!(ty, Type::String | Type::StrView) {
                        return Err(self.fail(&format!("argument must be a string or string view, found {}", ty.name())));
                    }
                    args.push(Argument::View(value));
                } else if expected == Type::VecRef || expected == Type::StrVecRef {
                    self.take(Kind::Amp)?;
                    let arg = self.identifier()?;
                    let (var, _, actual) = *self.names.get(&arg).ok_or_else(|| self.fail(&format!("unknown variable '{arg}'")))?;
                    let owned_shape = if expected == Type::VecRef { Type::Vec } else { Type::StrVec };
                    let borrowed_shape = if expected == Type::VecRef { Type::VecRef } else { Type::StrVecRef };
                    if actual != owned_shape && actual != borrowed_shape {
                        return Err(self.fail(&format!("argument must be {}, found {}", owned_shape.name(), actual.name())));
                    }
                    if self.moved.contains(&var) { return Err(self.fail(&format!("cannot borrow '{arg}' because it was moved"))); }
                    // An owned vector needs its address taken; a parameter that
                    // is already `const ae_vec *` is forwarded unchanged.
                    // The flag means "take the address", not "is a vector".
                    args.push(Argument::Borrow(var, actual == owned_shape));
                } else if let Type::Array(len) = expected {
                    self.take(Kind::Amp)?;
                    let arg = self.identifier()?;
                    let (var, _, actual) = *self.names.get(&arg).ok_or_else(|| self.fail(&format!("unknown array '{arg}'")))?;
                    if actual != Type::Array(len) { return Err(self.fail(&format!("argument requires array of length {len}"))); }
                    args.push(Argument::Borrow(var, false));
                } else {
                    // By-value arguments always consume their value:
                    // the callee releases the parameter, so the caller
                    // must not use the slot afterwards even when the
                    // surrounding statement is not a move context.
                    let (value, ty) = self.expression(0, true)?;
                    if ty != expected { return Err(self.fail(&format!("argument {} to '{name}' must be {}", args.len() + 1, expected.name()))); }
                    args.push(Argument::Value(value));
                }
                if *self.current() != Kind::Comma { break; }
                self.pos += 1;
            }
        }
        self.take(Kind::RParen)?;
        if args.len() != types.len() { return Err(self.fail(&format!("function '{name}' expects {} arguments, got {}", types.len(), args.len()))); }
        Ok(args)
    }
    fn block(&mut self) -> Result<Vec<Statement>, String> {
        if self.block_depth >= 64 { return Err(self.fail("block nesting limit exceeded (64)")); }
        self.block_depth += 1;
        self.scopes.push(Vec::new());
        let result = self.block_inner();
        let scope = self.scopes.pop().unwrap();
        self.block_depth -= 1;
        let mut result = result?;
        // Owned values declared in this block are released at its end.
        // Releases are idempotent (slots are zeroed), so overlapping paths
        // stay safe.
        let owned: Vec<(usize, Type)> = scope.iter().filter(|(_, ty)| ty.owned()).copied().collect();
        if !owned.is_empty() { result.push(Statement::Frees(owned)); }
        // Moved markers for names that left scope are no longer meaningful,
        // and borrows recorded in this scope end with it.
        let live: std::collections::HashSet<usize> = self.names.values().map(|&(id, _, _)| id).collect();
        self.moved.retain(|id| live.contains(id));
        let depth_now = self.scopes.len();
        self.borrows.retain(|&(_, depth)| depth <= depth_now);
        Ok(result)
    }
    // Views borrow their base: element reads and slice_views of variable
    // bases register the owner so it cannot be modified or moved while a
    // view can still be read.
    fn record_borrow_bases(&mut self, e: &Expr) {
        match e {
            Expr::StrVecIndex(id, idx, _, _, _) => {
                if !self.borrows.iter().any(|&(bid, _)| bid == *id) {
                    self.borrows.push((*id, self.scopes.len()));
                }
                self.record_borrow_bases(idx);
            }
            Expr::SliceView(_, a, b, _, _) => { self.record_borrow_bases(a); self.record_borrow_bases(b); }
            Expr::StructLit(_, values) => { for v in values { self.record_borrow_bases(v); } }
            Expr::EnumLit(_, _, _, values) => { for v in values { self.record_borrow_bases(v); } }
            Expr::Binary(a, _, b, _, _, _) => { self.record_borrow_bases(a); self.record_borrow_bases(b); }
            Expr::Neg(a, _, _) | Expr::Not(a) | Expr::Len(a, _, _) => self.record_borrow_bases(a),
            Expr::Index(_, _, a, _, _) => self.record_borrow_bases(a),
            Expr::VecIndex(_, a, _, _, _) => self.record_borrow_bases(a),
            Expr::Call(_, args, _) => {
                for arg in args {
                    match arg {
                        Argument::Value(e) => self.record_borrow_bases(e),
                        Argument::View(e) => self.record_borrow_bases(e),
                        Argument::Borrow(_, _) => {}
                    }
                }
            }
            Expr::Contains(a, b, _, _) | Expr::Split(a, b, _, _) | Expr::StrEq(a, b, _) | Expr::Find(a, b, _, _, _) => { self.record_borrow_bases(a); self.record_borrow_bases(b); }
            Expr::WriteFile(a, b, _, _) | Expr::Slice(_, a, b, _, _) => { self.record_borrow_bases(a); self.record_borrow_bases(b); }
            Expr::MapGet(_, key, _, _, _) | Expr::MapHas(_, key, _, _) | Expr::MapRemove(_, key, _, _) => self.record_borrow_bases(key),
            Expr::Join(_, sep, _, _) => self.record_borrow_bases(sep),
            Expr::Field(base, _, _) => self.record_borrow_bases(base),
            _ => {}
        }
    }
    fn live_owned(&self) -> Vec<(usize, Type)> {
        self.scopes.iter().flatten().filter(|(_, ty)| ty.owned()).map(|&(id, ty)| (id, ty)).collect()
    }
    // A loop body runs again, so a value moved out of a variable declared
    // outside the loop must be re-initialized before the body ends; otherwise
    // the next iteration would read a moved-from handle. Without a fixed point
    // over the body (ADR-20) the move is rejected instead of guessed at.
    fn close_loop_moves(&mut self, base: usize, before: std::collections::HashSet<usize>, offset: usize) -> Result<(), String> {
        let after = std::mem::replace(&mut self.moved, before.clone());
        let outer: Vec<usize> = self.scopes[..base.min(self.scopes.len())].iter().flatten().map(|&(id, _)| id).collect();
        for id in &after {
            // Values already moved before the loop are not this body's doing.
            if before.contains(id) || !outer.contains(id) { continue; }
            let name = self.names.iter().find(|(_, (vid, _, _))| vid == id).map(|(n, _)| n.clone()).unwrap_or_else(|| "?".to_owned());
            return Err(error(&self.file, self.source, offset, &format!("cannot move '{name}' inside a loop body: the next iteration would read it after the move (re-initialize it before the body ends)")));
        }
        // Moves of loop-local values die with the body; nothing to merge.
        Ok(())
    }
    // break/continue leave the loop body: only owned values declared inside it die.
    fn loop_local_owned(&self) -> Vec<(usize, Type)> {
        let base = *self.loop_scopes.last().unwrap_or(&0);
        self.scopes[base.min(self.scopes.len())..].iter().flatten().filter(|(_, ty)| ty.owned()).map(|&(id, ty)| (id, ty)).collect()
    }
    fn block_inner(&mut self) -> Result<Vec<Statement>, String> {
        self.take(Kind::LBrace)?;
        let mut declared = Vec::new();
        let mut result = Vec::new();
        while *self.current() != Kind::RBrace {
            match self.current().clone() {
                Kind::Let | Kind::Var => {
                    let mutable = *self.current() == Kind::Var; self.pos += 1;
                    let name = self.identifier()?;
                    if RESERVED.contains(&name.as_str()) { return Err(self.fail(&format!("'{name}' is a built-in name and cannot be a variable"))); }
                    if self.struct_names.contains_key(&name) { return Err(self.fail(&format!("'{name}' names a struct and cannot be a variable"))); }
                    if self.enum_names.contains_key(&name) { return Err(self.fail(&format!("'{name}' names an enum and cannot be a variable"))); }
                    if self.names.contains_key(&name) { return Err(self.fail(&format!("duplicate variable '{name}'"))); }
                    self.take(Kind::Equal)?;
                    let id = self.next_var; self.next_var += 1;
                    let (stmt, ty) = if *self.current() == Kind::LBracket {
                        self.pos += 1;
                        let mut values = Vec::new();
                        if *self.current() == Kind::RBracket { return Err(self.fail("array must contain at least one element")); }
                        loop {
                            if values.len() == 4096 { return Err(self.fail("array element limit exceeded (4096)")); }
                            let (element, element_ty) = self.expression(0, false)?;
                            if element_ty != Type::Int { return Err(self.fail("array elements must be i64")); }
                            values.push(element);
                            if *self.current() != Kind::Comma { break; }
                            self.pos += 1;
                        }
                        self.take(Kind::RBracket)?; self.take(Kind::Semi)?;
                        let len = values.len(); (Statement::Array(id, values), Type::Array(len))
                    } else {
                        // The initializer consumes string values.
                        let (value, ty) = self.expression(0, true)?;
                        // `let x = call()?;` binds the Ok payload and returns the
                        // error to the caller instead of continuing.
                        if *self.current() == Kind::Question {
                            let (statement, bound_ty) = self.propagate_statement(value, ty, Some(id))?;
                            self.pos += 1;
                            self.take(Kind::Semi)?;
                            let bound_ty = bound_ty.expect("a binding propagation always produces a payload type");
                            self.names.insert(name.clone(), (id, mutable, bound_ty));
                            self.scopes.last_mut().unwrap().push((id, bound_ty));
                            declared.push(name);
                            result.push(statement);
                            continue;
                        }
                        self.take(Kind::Semi)?;
                        // A borrow has no storage of its own: it lives only as a
                        // parameter or a pattern binding, never as a variable.
                        if matches!(ty, Type::VecRef | Type::StrVecRef) {
                            return Err(self.fail("cannot store a borrowed vector in a variable; pass it directly to a parameter that borrows"));
                        }
                        (Statement::Declare(id, value), ty)
                    };
                    self.names.insert(name.clone(), (id, mutable, ty));
                    self.scopes.last_mut().unwrap().push((id, ty));
                    declared.push(name);
                    if let Statement::Declare(_, init) = &stmt { self.record_borrow_bases(init); }
                    result.push(stmt);
                }
                Kind::Ident(name) => {
                    // `exit(code);` terminates the process. It releases the
                    // live owned values first, so the allocation account stays
                    // balanced and buffered output still flushes.
                    if name == "exit" && self.tokens.get(self.pos + 1).map(|t| t.kind.clone()) == Some(Kind::LParen) {
                        self.pos += 2;
                        let (code, code_ty) = self.expression(0, false)?;
                        if code_ty != Type::Int { return Err(self.fail("exit expects an i64 status code")); }
                        self.take(Kind::RParen)?; self.take(Kind::Semi)?;
                        result.push(Statement::Exit(code, self.live_owned()));
                        continue;
                    }
                    // Built-in mutation statements come before assignment so
                    // they stay unambiguous: push(v, x); pop(v);
                    if matches!(name.as_str(), "push" | "pop" | "sort" | "reverse") && self.tokens.get(self.pos + 1).map(|t| t.kind.clone()) == Some(Kind::LParen) {
                        let (line, col) = self.position();
                        self.pos += 2;
                        let target = self.identifier()?;
                        let (id, mutable, ty) = *self.names.get(&target).ok_or_else(|| self.fail(&format!("unknown variable '{target}'")))?;
                        if self.moved.contains(&id) { return Err(self.fail(&format!("cannot use '{target}' because it was moved"))); }
                        if !matches!(ty, Type::Vec | Type::StrVec) { return Err(self.fail(&format!("{name} expects a vec, found {}", ty.name()))); }
                        if !mutable { return Err(self.fail(&format!("cannot modify immutable variable '{target}'"))); }
                        if self.borrows.iter().any(|&(bid, _)| bid == id) { return Err(self.fail(&format!("cannot modify '{target}' while it is borrowed"))); }
                        // Ordering in place: qsort for i64, byte order for
                        // strings, so sorted output is identical on both
                        // platforms. The vector keeps its length.
                        if name == "sort" || name == "reverse" {
                            self.take(Kind::RParen)?; self.take(Kind::Semi)?;
                            let strings = ty == Type::StrVec;
                            result.push(if name == "sort" { Statement::Sort(id, strings, line, col) } else { Statement::Reverse(id, strings, line, col) });
                            continue;
                        }
                        if name == "push" {
                            self.take(Kind::Comma)?;
                            let (value, value_ty) = self.expression(0, ty == Type::StrVec)?;
                            // A borrow is fine: the element is a copy, so a view
                            // can be stored without outliving its source.
                            let ok = match ty { Type::Vec => value_ty == Type::Int, _ => matches!(value_ty, Type::String | Type::StrView) };
                            if !ok { return Err(self.fail(&format!("vector elements must be {}", if ty == Type::Vec { "i64" } else { "string" }))); }
                            self.take(Kind::RParen)?; self.take(Kind::Semi)?;
                            if ty == Type::StrVec {
                                result.push(Statement::StrVecPush(id, value, line, col, value_ty == Type::StrView));
                            } else {
                                result.push(Statement::VecPush(id, value, line, col));
                            }
                        } else {
                            self.take(Kind::RParen)?; self.take(Kind::Semi)?;
                            if ty == Type::StrVec {
                                result.push(Statement::StrVecPop(id, line, col));
                            } else {
                                result.push(Statement::VecPop(id, line, col));
                            }
                        }
                        continue;
                    }
                    if !self.names.contains_key(&name) {
                        // A call used only for its effect. The value must be
                        // propagated with '?', so a failure is never dropped.
                        let (value, ty) = self.expression(0, true)?;
                        if *self.current() != Kind::Question {
                            return Err(self.fail("a statement that only calls a function must end with '?' to propagate its result"));
                        }
                        let (statement, _) = self.propagate_statement(value, ty, None)?;
                        self.pos += 1;
                        self.take(Kind::Semi)?;
                        result.push(statement);
                        continue;
                    }
                    let (id, mutable, ty) = *self.names.get(&name).ok_or_else(|| self.fail(&format!("unknown variable '{name}'")))?;
                    if !mutable { return Err(self.fail(&format!("cannot assign to immutable variable '{name}'"))); }
                    self.pos += 1;
                    if ty == Type::Map && *self.current() == Kind::LBracket {
                        // `m[key] = value;` inserts or overwrites. The key is
                        // copied by the runtime, so a view or temporary is fine.
                        if self.borrows.iter().any(|&(bid, _)| bid == id) {
                            return Err(self.fail(&format!("cannot modify '{name}' while it is borrowed")));
                        }
                        let (line, col) = self.position();
                        self.take(Kind::LBracket)?;
                        let (key, key_ty) = self.expression(0, false)?;
                        if !matches!(key_ty, Type::String | Type::StrView) { return Err(self.fail("map keys must be strings")); }
                        self.take(Kind::RBracket)?; self.take(Kind::Equal)?;
                        let (value, value_ty) = self.expression(0, false)?; self.take(Kind::Semi)?;
                        if value_ty != Type::Int { return Err(self.fail("map values must be i64")); }
                        result.push(Statement::MapPut(MapPlace::Var(id), key, value, line, col));
                    } else if let Type::Array(len) = ty {
                        let (line, col) = self.position(); self.take(Kind::LBracket)?;
                        let (index, index_ty) = self.expression(0, false)?;
                        if index_ty != Type::Int { return Err(self.fail("array index must be an integer")); }
                        self.take(Kind::RBracket)?; self.take(Kind::Equal)?;
                        let (value, value_ty) = self.expression(0, false)?; self.take(Kind::Semi)?;
                        if value_ty != Type::Int { return Err(self.fail("array elements must be i64")); }
                        result.push(Statement::Store(id, len, index, value, line, col));
                    } else if ty == Type::StrVec && *self.current() == Kind::LBracket {
                        // Overwriting an element releases the old string, so a
                        // live borrow of any element blocks it.
                        if self.borrows.iter().any(|&(bid, _)| bid == id) {
                            return Err(self.fail(&format!("cannot modify '{name}' while it is borrowed")));
                        }
                        let (line, col) = self.position(); self.take(Kind::LBracket)?;
                        let (index, index_ty) = self.expression(0, false)?;
                        if index_ty != Type::Int { return Err(self.fail("vector index must be an integer")); }
                        self.take(Kind::RBracket)?; self.take(Kind::Equal)?;
                        let (value, value_ty) = self.expression(0, true)?; self.take(Kind::Semi)?;
                        if !matches!(value_ty, Type::String | Type::StrView) { return Err(self.fail("vector elements must be string")); }
                        result.push(Statement::StrVecStore(id, index, value, line, col, value_ty == Type::StrView));
                    } else if ty == Type::Vec && *self.current() == Kind::LBracket {
                        let (line, col) = self.position(); self.take(Kind::LBracket)?;
                        let (index, index_ty) = self.expression(0, false)?;
                        if index_ty != Type::Int { return Err(self.fail("vector index must be an integer")); }
                        self.take(Kind::RBracket)?; self.take(Kind::Equal)?;
                        let (value, value_ty) = self.expression(0, false)?; self.take(Kind::Semi)?;
                        if value_ty != Type::Int { return Err(self.fail("vector elements must be i64")); }
                        result.push(Statement::VecStore(id, index, value, line, col));
                    } else if let Type::Enum(_, _) = ty {
                        // Assigning an enum releases the old payload first.
                        if self.borrows.iter().any(|&(bid, _)| bid == id) {
                            return Err(self.fail(&format!("cannot assign to '{name}' while it is borrowed")));
                        }
                        self.take(Kind::Equal)?;
                        let moved_before = self.moved.contains(&id);
                        let (value, value_ty) = self.expression(0, true)?; self.take(Kind::Semi)?;
                        if value_ty != ty { return Err(self.fail(&format!("cannot assign a {} value to {} variable", value_ty.name(), ty.name()))); }
                        if self.moved.contains(&id) && !moved_before {
                            return Err(self.fail(&format!("cannot assign to '{name}' because it was moved in the same statement")));
                        }
                        self.moved.remove(&id);
                        result.push(Statement::Assign(id, value, true));
                    } else if let Type::Struct(sid) = ty {
                        if *self.current() == Kind::Dot {
                            self.take(Kind::Dot)?;
                            let fname = self.identifier()?;
                            let fty = {
                                let def = self.structs[sid].clone();
                                let fidx = def.fields.iter().position(|(n, _)| *n == fname)
                                    .ok_or_else(|| self.fail(&format!("struct '{}' has no field '{}'", def.name, fname)))?;
                                def.fields[fidx].1
                            };
                            if fty == Type::Map && *self.current() == Kind::LBracket {
                                // `s.counts[key] = n;` writes through the field,
                                // so the record itself stays in place.
                                if self.borrows.iter().any(|&(bid, _)| bid == id) {
                                    return Err(self.fail(&format!("cannot modify '{name}' while it is borrowed")));
                                }
                                let (line, col) = self.position();
                                self.take(Kind::LBracket)?;
                                let (key, key_ty) = self.expression(0, false)?;
                                if !matches!(key_ty, Type::String | Type::StrView) { return Err(self.fail("map keys must be strings")); }
                                self.take(Kind::RBracket)?; self.take(Kind::Equal)?;
                                let (value, value_ty) = self.expression(0, false)?; self.take(Kind::Semi)?;
                                if value_ty != Type::Int { return Err(self.fail("map values must be i64")); }
                                result.push(Statement::MapPut(MapPlace::Field(id, fname), key, value, line, col));
                                continue;
                            }
                            self.take(Kind::Equal)?;
                            let (value, value_ty) = self.expression(0, false)?; self.take(Kind::Semi)?;
                            if value_ty != fty { return Err(self.fail(&format!("field '{}' must be {}", fname, fty.name()))); }
                            result.push(Statement::FieldStore(id, fname, fty, value));
                            continue;
                        }
                        // Whole-struct assignment moves a fresh value in.
                        if self.borrows.iter().any(|&(bid, _)| bid == id) {
                            return Err(self.fail(&format!("cannot assign to '{name}' while it is borrowed")));
                        }
                        self.take(Kind::Equal)?;
                        let moved_before = self.moved.contains(&id);
                        let (value, value_ty) = self.expression(0, true)?; self.take(Kind::Semi)?;
                        if value_ty != ty { return Err(self.fail(&format!("cannot assign a {} value to {} variable", value_ty.name(), ty.name()))); }
                        if self.moved.contains(&id) && !moved_before {
                            return Err(self.fail(&format!("cannot assign to '{name}' because it was moved in the same statement")));
                        }
                        self.moved.remove(&id);
                        result.push(Statement::Assign(id, value, true));
                    } else {
                        if self.borrows.iter().any(|&(bid, _)| bid == id) {
                            return Err(self.fail(&format!("cannot assign to '{name}' while it is borrowed")));
                        }
                        self.take(Kind::Equal)?;
                        let moved_before = self.moved.contains(&id);
                        let (value, value_ty) = self.expression(0, true)?; self.take(Kind::Semi)?;
                        if value_ty != ty { return Err(self.fail(&format!("cannot assign a {} value to {} variable '{name}'", value_ty.name(), ty.name()))); }
                        // The right-hand side may not consume the target itself;
                        // assigning to an already-moved variable is a legal
                        // re-initialization, so the move flag clears either way.
                        if self.moved.contains(&id) && !moved_before {
                            return Err(self.fail(&format!("cannot assign to '{name}' because it was moved in the same statement")));
                        }
                        self.moved.remove(&id);
                        // Every owned slot releases its old value before the new
                        // one moves in; scalars have nothing to release.
                        result.push(Statement::Assign(id, value, ty.owned()));
                    }
                }
                Kind::Return => {
                    if self.in_main { return Err(self.fail("return is not supported in main")); }
                    self.pos += 1;
                    let (value, ty) = self.expression(0, true)?; self.take(Kind::Semi)?;
                    if ty != self.return_type { return Err(self.fail(&format!("return type must be {}", self.return_type.name()))); }
                    // Every live owned value that is not returned is released.
                    result.push(Statement::Return(value, self.live_owned()));
                }
                Kind::If => result.push(self.if_statement()?),
                Kind::Match => result.push(self.match_statement()?),
                Kind::While => {
                    let loop_offset = self.tokens[self.pos].offset;
                    self.pos += 1;
                    let (condition, ty) = self.expression(0, false)?;
                    if ty != Type::Bool { return Err(self.fail("condition must be a boolean expression")); }
                    if self.loop_depth >= 64 { return Err(self.fail("loop nesting limit exceeded (64)")); }
                    self.loop_depth += 1;
                    let base = self.scopes.len();
                    let moved_before = self.moved.clone();
                    self.loop_scopes.push(base);
                    self.loop_labels.push(None);
                    let body = self.block();
                    self.loop_labels.pop();
                    self.loop_scopes.pop();
                    self.loop_depth -= 1;
                    let body = body?;
                    self.close_loop_moves(base, moved_before, loop_offset)?;
                    result.push(Statement::While(condition, body));
                }
                Kind::For => {
                    let loop_offset = self.tokens[self.pos].offset;
                    self.pos += 1;
                    let var_name = self.identifier()?;
                    if RESERVED.contains(&var_name.as_str()) { return Err(self.fail(&format!("'{var_name}' is a built-in name and cannot be a loop variable"))); }
                    if self.names.contains_key(&var_name) { return Err(self.fail(&format!("duplicate variable '{var_name}'"))); }
                    self.take(Kind::In)?;
                    let (first, first_ty) = self.expression(0, false)?;
                    if *self.current() != Kind::DotDot {
                        // `for x in vec-variable` iterates owned elements by
                        // value: desugars to a hidden range loop plus an
                        // immutable element declaration per iteration.
                        // Borrowed string vectors iterate the same way, with
                        // element access going through the pointer.
                        let (v_id, elem_ty, by_ref) = match (&first, first_ty) {
                            (Expr::Variable(id, Type::Vec, _, _), Type::Vec) => (*id, Type::Int, false),
                            (Expr::Variable(id, Type::StrVec, _, _), Type::StrVec) => (*id, Type::StrView, false),
                            (Expr::Variable(id, Type::StrVecRef, _, _), Type::StrVecRef) => (*id, Type::StrView, true),
                            (Expr::Variable(id, Type::VecRef, _, _), Type::VecRef) => (*id, Type::Int, false),
                            _ => return Err(self.fail("for-in needs a bare vec variable or an int range (start..end)")),
                        };
                        if self.loop_depth >= 64 { return Err(self.fail("loop nesting limit exceeded (64)")); }
                        let (line, col) = self.position();
                        let end_var = self.next_var; self.next_var += 1;
                        let counter = self.next_var; self.next_var += 1;
                        let var = self.next_var; self.next_var += 1;
                        let label = self.next_label; self.next_label += 1;
                        self.loop_depth += 1;
                        let base = self.scopes.len();
                        let moved_before = self.moved.clone();
                        self.scopes.push(vec![(var, elem_ty), (counter, Type::Int), (end_var, Type::Int)]);
                        self.loop_scopes.push(self.scopes.len());
                        self.loop_labels.push(Some(label));
                        self.names.insert(var_name.clone(), (var, false, elem_ty));
                        // The container is borrowed for the whole loop.
                        let container_depth = self.scopes.len();
                        self.borrows.push((v_id, container_depth));
                        let body = self.block();
                        self.loop_labels.pop();
                        self.loop_scopes.pop();
                        self.scopes.pop().unwrap();
                        self.loop_depth -= 1;
                        self.names.remove(&var_name);
                        self.borrows.retain(|&(id, _)| id != v_id);
                        let mut body = body?;
                        if self.moved.contains(&v_id) { return Err(self.fail(&format!("cannot move the vector being iterated (a move of it happens in this loop)"))); }
                        self.close_loop_moves(base, moved_before, loop_offset)?;
                        // Per-iteration element binding, then the user's body.
                        let element = match elem_ty {
                            Type::StrView => Expr::StrVecIndex(v_id, Box::new(Expr::Variable(counter, Type::Int, line, col)), line, col, by_ref),
                            _ => Expr::VecIndex(v_id, Box::new(Expr::Variable(counter, Type::Int, line, col)), line, col, first_ty == Type::VecRef),
                        };
                        body.insert(0, Statement::Declare(var, element));
                        result.push(Statement::Declare(end_var, Expr::Len(Box::new(first), line, col)));
                        result.push(Statement::Declare(counter, Expr::Number(0)));
                        result.push(Statement::For(counter, end_var, body, label, line, col));
                        continue;
                    }
                    if first_ty != Type::Int { return Err(self.fail("range bounds must be i64")); }
                    self.take(Kind::DotDot)?;
                    let (end, end_ty) = self.expression(0, false)?;
                    if end_ty != Type::Int { return Err(self.fail("range bounds must be i64")); }
                    if self.loop_depth >= 64 { return Err(self.fail("loop nesting limit exceeded (64)")); }
                    let (line, col) = self.position();
                    // Hidden storage: bounds evaluate once, in order.
                    let end_var = self.next_var; self.next_var += 1;
                    let var = self.next_var; self.next_var += 1;
                    let label = self.next_label; self.next_label += 1;
                    self.loop_depth += 1;
                    let base = self.scopes.len();
                    let moved_before = self.moved.clone();
                    self.scopes.push(vec![(var, Type::Int)]);
                    self.loop_scopes.push(self.scopes.len());
                    self.loop_labels.push(Some(label));
                    self.names.insert(var_name.clone(), (var, false, Type::Int));
                    let body = self.block();
                    self.loop_labels.pop();
                    self.loop_scopes.pop();
                    let for_scope = self.scopes.pop().unwrap();
                    self.loop_depth -= 1;
                    self.names.remove(&var_name);
                    let body = body?;
                    self.close_loop_moves(base, moved_before, loop_offset)?;
                    debug_assert!(for_scope.iter().all(|(_, ty)| *ty != Type::String));
                    // Bounds evaluate once, left to right, before any iteration.
                    result.push(Statement::Declare(var, first));
                    result.push(Statement::Declare(end_var, end));
                    result.push(Statement::For(var, end_var, body, label, line, col));
                }
                Kind::Break | Kind::Continue => {
                    let keyword = if *self.current() == Kind::Break { "break" } else { "continue" };
                    if self.loop_depth == 0 { return Err(self.fail(&format!("'{keyword}' outside of a loop"))); }
                    self.pos += 1; self.take(Kind::Semi)?;
                    let owned = self.loop_local_owned();
                    if !owned.is_empty() { result.push(Statement::Frees(owned)); }
                    result.push(if keyword == "break" { Statement::Break } else { Statement::Continue(self.loop_labels.last().copied().flatten()) });
                }
                Kind::Assert => {
                    let (line, col) = self.position();
                    self.pos += 1; self.take(Kind::LParen)?;
                    let (condition, ty) = self.expression(0, false)?;
                    if ty != Type::Bool { return Err(self.fail("assert requires a boolean condition")); }
                    self.take(Kind::RParen)?; self.take(Kind::Semi)?;
                    result.push(Statement::Assert(condition, line, col));
                }
                Kind::Print => {
                    self.pos += 1; self.take(Kind::LParen)?;
                    let is_bare_text = matches!(self.current(), Kind::Text(_))
                        && matches!(self.tokens.get(self.pos + 1).map(|t| &t.kind), Some(Kind::RParen));
                    let printed = if is_bare_text {
                        if let Kind::Text(text) = self.current().clone() { self.pos += 1; Printed::Text(text) } else { unreachable!() }
                    } else {
                        // print borrows its value; only `+` consumes strings.
                        let (value, ty) = self.expression(0, false)?;
                        if !matches!(ty, Type::Int | Type::Bool | Type::F64 | Type::String | Type::StrView) {
                            return Err(self.fail(&format!("cannot print a value of type {}", ty.name())));
                        }
                        Printed::Expr(value)
                    };
                    self.take(Kind::RParen)?; self.take(Kind::Semi)?;
                    result.push(Statement::Print(printed));
                }
                _ => return Err(self.fail("expected declaration, assignment, print, or '}'")),
            };
        }
        self.take(Kind::RBrace)?;
        for name in declared { self.names.remove(&name); }
        Ok(result)
    }
    fn if_statement(&mut self) -> Result<Statement, String> {
        self.pos += 1;
        let (condition, ty) = self.expression(0, false)?;
        if ty != Type::Bool { return Err(self.fail("condition must be a boolean expression")); }
        let snapshot = self.moved.clone();
        let yes = self.block()?;
        let after_yes = std::mem::replace(&mut self.moved, snapshot.clone());
        let no = if *self.current() == Kind::Else {
            self.pos += 1;
            // `else if` chains recurse instead of nesting braces.
            if *self.current() == Kind::If { vec![self.if_statement()?] } else { self.block()? }
        } else { Vec::new() };
        // A variable moved in either branch is treated as moved after the if.
        for id in after_yes { self.moved.insert(id); }
        Ok(Statement::If(condition, yes, no))
    }
    // `expr?` propagates the error variant of a result enum: on `Err` every
    // live owned value is released and the enum itself is returned, so a caller
    // never observes a half-finished result. On `Ok` the payload is bound (or
    // released when the statement only wanted the side effect).
    fn propagate_statement(&mut self, value: Expr, ty: Type, binding: Option<usize>) -> Result<(Statement, Option<Type>), String> {
        let eid = match ty {
            Type::Enum(_, eid) => eid,
            _ => return Err(self.fail(&format!("'?' needs a result or option value, found {}", ty.name()))),
        };
        let def = self.enums[eid].clone();
        // Both two-variant shapes can be propagated: a result hands its Err
        // back, an Option hands its None back. Neither variant has to be a
        // prelude enum - a user enum with the same shape behaves identically.
        let result_shaped = def.variants.len() == 2
            && def.variants[0].0 == "Ok" && def.variants[1].0 == "Err"
            && def.variants[1].1 == [Type::String];
        let option_shaped = def.variants.len() == 2
            && def.variants[0].0 == "Some" && def.variants[1].0 == "None"
            && def.variants[1].1.is_empty() && def.variants[0].1.len() == 1;
        if !result_shaped && !option_shaped {
            return Err(self.fail(&format!("'?' needs an enum shaped {{ Ok(...), Err(string) }} or {{ Some(T), None }}, and '{}' is not one", def.name)));
        }
        if self.in_main || self.return_type != ty {
            return Err(self.fail(&format!("'?' needs the enclosing function to return {}, so the failure can be handed on", def.name)));
        }
        let fields = def.variants[0].1.clone();
        let bound = match binding {
            Some(id) => {
                if fields.len() != 1 {
                    return Err(self.fail(&format!("'{}::{}' carries {} values; a binding `let` needs exactly one (use a bare statement to discard)", def.name, def.variants[0].0, fields.len())));
                }
                Some((id, fields[0]))
            }
            None => None,
        };
        let bound_ty = bound.map(|(_, ty)| ty);
        Ok((Statement::Propagate(value, ty, fields, bound, self.live_owned()), bound_ty))
    }
    // `match subject { Enum::Variant(a, b) => { ... } ... }` is a statement.
    // Coverage must be exhaustive and each variant may appear once, so there is
    // no fall-through path to reason about. The subject is borrowed for the
    // whole match: arms can read payloads but never move or overwrite it.
    fn match_statement(&mut self) -> Result<Statement, String> {
        self.pos += 1;
        let (subject, subject_ty) = self.expression(0, false)?;
        let (sid, eid) = match (&subject, subject_ty) {
            (Expr::Variable(id, Type::Enum(_, _), _, _), Type::Enum(_, k)) => (*id, k),
            _ => return Err(self.fail("match needs a bare enum variable as its subject")),
        };
        if self.moved.contains(&sid) { return Err(self.fail("cannot match a value that was moved")); }
        let def = self.enums[eid].clone();
        self.take(Kind::LBrace)?;
        let mut arms: Vec<MatchArm> = Vec::new();
        let mut seen = vec![false; def.variants.len()];
        self.borrows.push((sid, self.scopes.len()));
        while *self.current() != Kind::RBrace {
            let pattern = self.identifier()?;
            // An instantiated enum is matched by its declaration name:
            // `Option::Some(v)` covers Option[string] and Option[i64] alike.
            if pattern != def.pattern {
                return Err(self.fail(&format!("pattern must name the matched enum ('{}::Variant')", def.pattern)));
            }
            self.take(Kind::ColonColon)?;
            let vname = self.identifier()?;
            let vidx = def.variants.iter().position(|(n, _)| *n == vname)
                .ok_or_else(|| self.fail(&format!("enum '{}' has no variant '{}'", def.name, vname)))?;
            if seen[vidx] { return Err(self.fail(&format!("variant '{}::{}' is matched twice", def.name, vname))); }
            seen[vidx] = true;
            let fields = def.variants[vidx].1.clone();
            self.scopes.push(Vec::new());
            let mut bindings = Vec::new();
            let mut bound: Vec<String> = Vec::new();
            if *self.current() == Kind::LParen {
                if fields.is_empty() { return Err(self.fail(&format!("variant '{}::{}' has no payload", def.name, vname))); }
                self.pos += 1;
                for (index, field) in fields.iter().copied().enumerate() {
                    let bname = self.identifier()?;
                    if RESERVED.contains(&bname.as_str()) { return Err(self.fail(&format!("'{bname}' is a built-in name and cannot be a pattern binding"))); }
                    if self.names.contains_key(&bname) { return Err(self.fail(&format!("duplicate variable '{bname}'"))); }
                    let id = self.next_var; self.next_var += 1;
                    // String payloads bind as read-only views and vec payloads
                    // as read-only borrows: nothing can be moved out of the
                    // matched enum.
                    let bty = match field {
                        Type::String => Type::StrView,
                        Type::Vec => Type::VecRef,
                        Type::StrVec => Type::StrVecRef,
                        other => other,
                    };
                    self.names.insert(bname.clone(), (id, false, bty));
                    self.scopes.last_mut().unwrap().push((id, bty));
                    bound.push(bname);
                    bindings.push((id, field, index));
                    if index + 1 < fields.len() { self.take(Kind::Comma)?; }
                }
                if *self.current() == Kind::Comma {
                    return Err(self.fail(&format!("variant '{}::{}' binds {} payload value(s)", def.name, vname, fields.len())));
                }
                self.take(Kind::RParen)?;
            } else if !fields.is_empty() {
                return Err(self.fail(&format!("variant '{}::{}' needs {} binding(s)", def.name, vname, fields.len())));
            }
            self.take(Kind::FatArrow)?;
            let body = self.block()?;
            self.scopes.pop().unwrap();
            for name in bound { self.names.remove(&name); }
            arms.push(MatchArm { variant: vidx, bindings, body });
        }
        self.take(Kind::RBrace)?;
        self.borrows.pop();
        if let Some(missing) = seen.iter().position(|covered| !covered) {
            return Err(self.fail(&format!("match is not exhaustive: '{}::{}' is not handled", def.name, def.variants[missing].0)));
        }
        Ok(Statement::Match(sid, subject_ty, arms))
    }
}

// Expression emission tracks ownership of string values so every buffer is
// released exactly once: moved variables are nulled after hand-off, owned
// temporaries are consumed or freed by their statement, and releases are
// idempotent even if analysis and emission ever disagree.
#[derive(Debug)]
struct Emitted { text: String, ty: Type, owned: bool, moves: Vec<(usize, Type)>, materialized: bool }
impl Emitted {
    fn value(text: String, ty: Type) -> Self { Emitted { text, ty, owned: false, moves: Vec::new(), materialized: true } }
    fn moved(id: usize, ty: Type, text: String) -> Self { Emitted { text, ty, owned: true, moves: vec![(id, ty)], materialized: true } }
}

// Rust's Debug formatting is the shortest decimal that round-trips and always
// carries a '.' or an exponent, so it is a valid C double literal. Non-finite
// values never reach here: the lexer rejects them (ADR-37).
fn c_float_literal(value: f64) -> String {
    let text = format!("{value:?}");
    if text.contains(['.', 'e', 'E']) { text } else { format!("{text}.0") }
}

fn emit_expr(expr: &Expr, out: &mut String, next: &mut usize, moving: bool) -> Emitted {
    let emitted = match expr {
        Expr::Number(n) => Emitted::value(format!("INT64_C({n})"), Type::Int),
        Expr::FloatLit(x) => Emitted::value(c_float_literal(*x), Type::F64),
        Expr::ToF64(value, line, column) => {
            let t = emit_expr(value, out, next, false);
            let id = *next; *next += 1;
            out.push_str(&format!("    double t{id} = ae_f64_from_i64({}, {line}, {column});\n", t.text));
            Emitted { text: format!("t{id}"), ty: Type::F64, owned: false, moves: t.moves, materialized: true }
        }
        Expr::ToI64(value, ty, line, column) => {
            let t = emit_expr(value, out, next, false);
            let id = *next; *next += 1;
            out.push_str(&format!("    {}t{id} = ae_f64_to_i64({}, {line}, {column});\n", ty.c_temp(), t.text));
            Emitted { text: format!("t{id}"), ty: *ty, owned: true, moves: Vec::new(), materialized: true }
        }
        Expr::FloatToString(value, line, column) => {
            let t = emit_expr(value, out, next, false);
            let id = *next; *next += 1;
            out.push_str(&format!("    ae_string t{id} = ae_f64_to_string({}, {line}, {column});\n", t.text));
            Emitted { text: format!("t{id}"), ty: Type::String, owned: true, moves: Vec::new(), materialized: true }
        }
        Expr::BoolLit(b) => Emitted::value(if *b { "true" } else { "false" }.to_owned(), Type::Bool),
        Expr::StrLit(text, line, column) => Emitted {
            text: format!("ae_str_from(\"{text}\", {line}, {column})"),
            ty: Type::String, owned: true, moves: Vec::new(), materialized: false,
        },
        Expr::Variable(id, ty, line, column) => {
            if *ty == Type::VecRef {
                out.push_str(&format!("    if (!v{id}->data) ae_fail(\"use of moved vector\", {line}, {column});
"));
                Emitted { text: format!("(*v{id})"), ty: Type::Vec, owned: false, moves: Vec::new(), materialized: true }
            } else if *ty == Type::StrVecRef {
                out.push_str(&format!("    if (!v{id}->data) ae_fail(\"use of moved vector\", {line}, {column});\n"));
                Emitted { text: format!("(*v{id})"), ty: Type::StrVec, owned: false, moves: Vec::new(), materialized: true }
            } else if ty.owned() {
                // Runtime backstop: a moved value's slot is empty.
                match ty {
                    // Structs and enums have no runtime sentinel: the compile-time
                    // move set is their only guard (documented limitation).
                    Type::Struct(_) | Type::Enum(_, _) => {}
                    _ => {
                        let probe = match ty { Type::Vec | Type::StrVec => format!("!v{id}.data"), Type::Map => format!("!v{id}.slots"), _ => format!("!v{id}.ptr") };
                        let message = match ty { Type::Vec | Type::StrVec => "use of moved vector", Type::Map => "use of moved map", _ => "use of moved string" };
                        out.push_str(&format!("    if ({probe}) ae_fail(\"{message}\", {line}, {column});\n"));
                    }
                }
                if moving {
                    let t = *next; *next += 1;
                    out.push_str(&format!("    {}t{t} = v{id};\n", ty.c_temp()));
                    Emitted::moved(*id, *ty, format!("t{t}"))
                } else {
                    Emitted { text: format!("v{id}"), ty: *ty, owned: false, moves: Vec::new(), materialized: true }
                }
            } else {
                Emitted::value(format!("v{id}"), *ty)
            }
        }
        Expr::Call(id, args, ret) => {
            let mut moves = Vec::new();
            // A read-only argument only borrows its value, so a temporary
            // (literal or `+` result) stays owned by this expression and is
            // released after the call. Without this, `f("a" + b)` leaks.
            let mut view_temps: Vec<String> = Vec::new();
            let arguments = args.iter().map(|arg| match arg {
                Argument::Value(e) => { let t = emit_expr(e, out, next, true); moves.extend(t.moves); t.text }
                Argument::Borrow(id, is_vec) => if *is_vec { format!("&v{id}") } else { format!("v{id}") },
                Argument::View(e) => {
                    let t = emit_expr(e, out, next, false);
                    moves.extend(t.moves);
                    if t.owned { view_temps.push(t.text.clone()); }
                    format!("AE_VIEW({})", t.text)
                }
            }).collect::<Vec<_>>().join(", ");
            let t = *next; *next += 1;
            out.push_str(&format!("    {}t{t} = ae_fn_{id}({arguments});\n", ret.c_temp()));
            for text in &view_temps { out.push_str(&format!("    ae_str_release(&{text});\n")); }
            null_moves(out, &moves);
            Emitted { text: format!("t{t}"), ty: *ret, owned: ret.owned(), moves: Vec::new(), materialized: true }
        }
        Expr::VecLit(values, line, column) => {
            let t = *next; *next += 1;
            out.push_str(&format!("    ae_vec t{t} = ae_vec_new({line}, {column});\n"));
            for value in values {
                let element = emit_expr(value, out, next, false);
                out.push_str(&format!("    ae_vec_push(&t{t}, {}, {line}, {column});\n", element.text));
                null_moves(out, &element.moves);
            }
            Emitted { text: format!("t{t}"), ty: Type::Vec, owned: true, moves: Vec::new(), materialized: true }
        }
        Expr::VecIndex(id, index, line, col, by_ref) => {
            let (check, receiver) = if *by_ref {
                (format!("if (!v{id}->data) ae_fail(\"use of moved vector\", {line}, {col});"), format!("(*v{id})"))
            } else {
                (format!("if (!v{id}.data) ae_fail(\"use of moved vector\", {line}, {col});"), format!("v{id}"))
            };
            out.push_str(&format!("    {check}\n"));
            let i = emit_expr(index, out, next, false);
            Emitted { text: format!("ae_vec_get({receiver}, {}, {line}, {col})", i.text), ty: Type::Int, owned: false, moves: i.moves, materialized: false }
        }
        Expr::Index(id, len, index, line, col) => {
            let i = emit_expr(index, out, next, false);
            Emitted { text: format!("v{id}[ae_index({}, {len}, {line}, {col})]", i.text), ty: Type::Int, owned: false, moves: i.moves, materialized: false }
        }
        Expr::Len(e, line, col) => {
            let t = emit_expr(e, out, next, false);
            let id = *next; *next += 1;
            let call = match t.ty {
                Type::String | Type::StrView => format!("ae_str_len(AE_VIEW({}), {line}, {col})", t.text),
                Type::StrVec => format!("ae_strvec_len({}, {line}, {col})", t.text),
                Type::Map => format!("ae_map_len(&{}, {line}, {col})", t.text),
                _ => format!("ae_vec_len({}, {line}, {col})", t.text),
            };
            out.push_str(&format!("    int64_t t{id} = {call};\n"));
            if t.owned { drop_owned_temp(out, &t); }
            Emitted { text: format!("t{id}"), ty: Type::Int, owned: false, moves: Vec::new(), materialized: true }
        }
        Expr::Neg(a, line, column) => {
            let t = emit_expr(a, out, next, false);
            if t.ty == Type::F64 {
                Emitted { text: format!("-({})", t.text), ty: Type::F64, owned: false, moves: t.moves, materialized: false }
            } else {
                Emitted { text: format!("ae_neg({}, {line}, {column})", t.text), ty: Type::Int, owned: false, moves: t.moves, materialized: false }
            }
        }
        Expr::Not(a) => {
            let t = emit_expr(a, out, next, false);
            Emitted { text: format!("!{}", t.text), ty: Type::Bool, owned: false, moves: t.moves, materialized: false }
        }
        Expr::ReadFile(e, line, col) => {
            let t = emit_expr(e, out, next, false);
            let id = *next; *next += 1;
            out.push_str(&format!("    ae_string t{id} = ae_read_file({}.ptr, {line}, {col});
", t.text));
            // The path is only read: an owned temporary is released here, a
            // named slot keeps owning its buffer.
            if t.owned { drop_owned_temp(out, &t); }
            Emitted { text: format!("t{id}"), ty: Type::String, owned: true, moves: Vec::new(), materialized: true }
        }
        Expr::WriteFile(path, content, line, col) => {
            let p = emit_expr(path, out, next, false);
            let c = emit_expr(content, out, next, false);
            let id = *next; *next += 1;
            out.push_str(&format!("    int64_t t{id} = ae_write_file({}, AE_VIEW({}), {line}, {col});
", p.text, c.text));
            // Both operands are read, never consumed by the call.
            if p.owned { drop_owned_temp(out, &p); }
            if c.owned { drop_owned_temp(out, &c); }
            Emitted { text: format!("t{id}"), ty: Type::Int, owned: false, moves: Vec::new(), materialized: true }
        }
        Expr::Bytes(e, line, col) => {
            let t = emit_expr(e, out, next, false);
            let id = *next; *next += 1;
            out.push_str(&format!("    ae_vec t{id} = ae_bytes(AE_VIEW({}), {line}, {col});
", t.text));
            if t.owned { drop_owned_temp(out, &t); }
            Emitted { text: format!("t{id}"), ty: Type::Vec, owned: true, moves: Vec::new(), materialized: true }
        }
        Expr::Slice(e, start, end, line, col) => {
            let t = emit_expr(e, out, next, false);
            let st = emit_expr(start, out, next, false);
            let en = emit_expr(end, out, next, false);
            let id = *next; *next += 1;
            out.push_str(&format!("    ae_string t{id} = ae_slice(AE_VIEW({}), {}, {}, {line}, {col});
", t.text, st.text, en.text));
            if t.owned { drop_owned_temp(out, &t); }
            Emitted { text: format!("t{id}"), ty: Type::String, owned: true, moves: Vec::new(), materialized: true }
        }
        Expr::Contains(haystack, needle, line, col) => {
            let h = emit_expr(haystack, out, next, false);
            let n = emit_expr(needle, out, next, false);
            let id = *next; *next += 1;
            out.push_str(&format!("    bool t{id} = ae_contains(AE_VIEW({}), AE_VIEW({}), {line}, {col});
", h.text, n.text));
            if h.owned { drop_owned_temp(out, &h); }
            if n.owned { drop_owned_temp(out, &n); }
            Emitted { text: format!("t{id}"), ty: Type::Bool, owned: false, moves: Vec::new(), materialized: true }
        }
        Expr::Find(haystack, needle, ty, line, col) => {
            let h = emit_expr(haystack, out, next, false);
            let n = emit_expr(needle, out, next, false);
            let id = *next; *next += 1;
            out.push_str(&format!("    {}t{id} = ae_find(AE_VIEW({}), AE_VIEW({}), {line}, {col});\n", ty.c_temp(), h.text, n.text));
            if n.owned { drop_owned_temp(out, &n); }
            if h.owned { drop_owned_temp(out, &h); }
            Emitted { text: format!("t{id}"), ty: *ty, owned: true, moves: Vec::new(), materialized: true }
        }
        Expr::StartsWith(s, prefix) => {
            let h = emit_expr(s, out, next, false);
            let p = emit_expr(prefix, out, next, false);
            let id = *next; *next += 1;
            out.push_str(&format!("    bool t{id} = ae_starts_with(AE_VIEW({}), AE_VIEW({}));\n", h.text, p.text));
            if h.owned { drop_owned_temp(out, &h); }
            if p.owned { drop_owned_temp(out, &p); }
            Emitted { text: format!("t{id}"), ty: Type::Bool, owned: false, moves: Vec::new(), materialized: true }
        }
        Expr::EndsWith(s, suffix) => {
            let h = emit_expr(s, out, next, false);
            let p = emit_expr(suffix, out, next, false);
            let id = *next; *next += 1;
            out.push_str(&format!("    bool t{id} = ae_ends_with(AE_VIEW({}), AE_VIEW({}));\n", h.text, p.text));
            if h.owned { drop_owned_temp(out, &h); }
            if p.owned { drop_owned_temp(out, &p); }
            Emitted { text: format!("t{id}"), ty: Type::Bool, owned: false, moves: Vec::new(), materialized: true }
        }
        Expr::Trim(s, line, col) => {
            let t = emit_expr(s, out, next, false);
            let id = *next; *next += 1;
            out.push_str(&format!("    ae_string t{id} = ae_trim(AE_VIEW({}), {line}, {col});\n", t.text));
            if t.owned { drop_owned_temp(out, &t); }
            Emitted { text: format!("t{id}"), ty: Type::String, owned: true, moves: Vec::new(), materialized: true }
        }
        Expr::Split(s, sep, line, col) => {
            let h = emit_expr(s, out, next, false);
            let n = emit_expr(sep, out, next, false);
            let id = *next; *next += 1;
            out.push_str(&format!("    ae_strvec t{id} = ae_split(AE_VIEW({}), AE_VIEW({}), {line}, {col});\n", h.text, n.text));
            if h.owned { drop_owned_temp(out, &h); }
            if n.owned { drop_owned_temp(out, &n); }
            Emitted { text: format!("t{id}"), ty: Type::StrVec, owned: true, moves: Vec::new(), materialized: true }
        }
        Expr::Args(line, col) => {
            let id = *next; *next += 1;
            out.push_str(&format!("    ae_strvec t{id} = ae_args({line}, {col});\n"));
            Emitted { text: format!("t{id}"), ty: Type::StrVec, owned: true, moves: Vec::new(), materialized: true }
        }
        Expr::StrVecIndex(id, index, line, col, by_ref) => {
            let (check, access) = if *by_ref {
                (format!("if (!v{id}->data) ae_fail(\"use of moved vector\", {line}, {col});"), format!("ae_strvec_get_const(v{id}"))
            } else {
                (format!("if (!v{id}.data) ae_fail(\"use of moved vector\", {line}, {col});"), format!("ae_strvec_get(&v{id}"))
            };
            out.push_str(&format!("    {check}\n"));
            let i = emit_expr(index, out, next, false);
            // Elements are borrow-only: the pointer view never owns.
            Emitted { text: format!("(*{access}, {}, {line}, {col}))", i.text), ty: Type::String, owned: false, moves: i.moves, materialized: true }
        }
        Expr::SliceView(vid, start, end, line, col) => {
            // The base is a borrowed bare string variable; it cannot move or
            // be reassigned while the view exists (borrow tracker).
            let st = emit_expr(start, out, next, false);
            let en = emit_expr(end, out, next, false);
            let id = *next; *next += 1;
            out.push_str(&format!("    if (!v{vid}.ptr) ae_fail(\"use of moved string\", {line}, {col});
"));
            out.push_str(&format!("    ae_strview t{id} = ae_slice_view(AE_VIEW(v{vid}), {}, {}, {line}, {col});
", st.text, en.text));
            Emitted { text: format!("t{id}"), ty: Type::StrView, owned: false, moves: Vec::new(), materialized: true }
        }
        Expr::Join(vid, sep, line, col) => {
            out.push_str(&format!("    if (!v{vid}.data) ae_fail(\"use of moved vector\", {line}, {col});\n"));
            let n = emit_expr(sep, out, next, false);
            let id = *next; *next += 1;
            out.push_str(&format!("    ae_string t{id} = ae_join(&v{vid}, AE_VIEW({}), {line}, {col});\n", n.text));
            if n.owned { drop_owned_temp(out, &n); }
            Emitted { text: format!("t{id}"), ty: Type::String, owned: true, moves: Vec::new(), materialized: true }
        }
        Expr::Lines(s, line, col) => {
            let t = emit_expr(s, out, next, false);
            let id = *next; *next += 1;
            out.push_str(&format!("    ae_strvec t{id} = ae_lines(AE_VIEW({}), {line}, {col});
", t.text));
            if t.owned { drop_owned_temp(out, &t); }
            Emitted { text: format!("t{id}"), ty: Type::StrVec, owned: true, moves: Vec::new(), materialized: true }
        }
        Expr::CountLinesWith(s, pat, line, col) => {
            let t = emit_expr(s, out, next, false);
            let pt = emit_expr(pat, out, next, false);
            let id = *next; *next += 1;
            out.push_str(&format!("    int64_t t{id} = ae_count_lines_with(AE_VIEW({}), AE_VIEW({}), {line}, {col});\n", t.text, pt.text));
            if t.owned { drop_owned_temp(out, &t); }
            if pt.owned { drop_owned_temp(out, &pt); }
            Emitted { text: format!("t{id}"), ty: Type::Int, owned: false, moves: Vec::new(), materialized: true }
        }
        Expr::StructLit(sid, values) => {
            let mut parts = Vec::new();
            let mut moves = Vec::new();
            for v in values {
                let e = emit_expr(v, out, next, true);
                moves.extend(e.moves);
                parts.push(e.text);
            }
            let id = *next; *next += 1;
            out.push_str(&format!("    ae_t{sid} t{id} = {{ {} }};\n", parts.join(", ")));
            null_moves(out, &moves);
            Emitted { text: format!("t{id}"), ty: Type::Struct(*sid), owned: true, moves: Vec::new(), materialized: true }
        }
        Expr::EnumLit(_name, eid, vidx, values) => {
            let mut parts = Vec::new();
            let mut moves = Vec::new();
            for value in values.iter() {
                let e = emit_expr(value, out, next, true);
                moves.extend(e.moves);
                parts.push(e.text);
            }
            let id = *next; *next += 1;
            let payload = if parts.is_empty() { "{ 0 }".to_owned() } else { format!("{{ .v{vidx} = {{ {} }} }}", parts.join(", ")) };
            out.push_str(&format!("    ae_e{eid} t{id} = {{ AE_E{eid}_{vidx}, {payload} }};\n"));
            null_moves(out, &moves);
            Emitted { text: format!("t{id}"), ty: Type::Enum(_name, *eid), owned: true, moves: Vec::new(), materialized: true }
        }
        Expr::TryReadFile(path, ty, line, col) => {
            let p = emit_expr(path, out, next, false);
            let id = *next; *next += 1;
            out.push_str(&format!("    {}t{id} = ae_try_read_file({}.ptr, {line}, {col});\n", ty.c_temp(), p.text));
            if p.owned { drop_owned_temp(out, &p); }
            Emitted { text: format!("t{id}"), ty: *ty, owned: true, moves: Vec::new(), materialized: true }
        }
        Expr::TryWriteFile(path, content, ty, line, col) => {
            let p = emit_expr(path, out, next, false);
            let c = emit_expr(content, out, next, false);
            let id = *next; *next += 1;
            out.push_str(&format!("    {}t{id} = ae_try_write_file({}.ptr, AE_VIEW({}), {line}, {col});\n", ty.c_temp(), p.text, c.text));
            if c.owned { drop_owned_temp(out, &c); }
            // The path may be an owned temporary too (path_join of two views,
            // for instance); it is consumed by the call, not by the caller.
            if p.owned { drop_owned_temp(out, &p); }
            Emitted { text: format!("t{id}"), ty: *ty, owned: true, moves: Vec::new(), materialized: true }
        }
        Expr::TryAppendFile(path, content, ty, line, col) => {
            let p = emit_expr(path, out, next, false);
            let c = emit_expr(content, out, next, false);
            let id = *next; *next += 1;
            out.push_str(&format!("    {}t{id} = ae_try_append_file({}.ptr, AE_VIEW({}), {line}, {col});\n", ty.c_temp(), p.text, c.text));
            if c.owned { drop_owned_temp(out, &c); }
            if p.owned { drop_owned_temp(out, &p); }
            Emitted { text: format!("t{id}"), ty: *ty, owned: true, moves: Vec::new(), materialized: true }
        }
        Expr::JsonGet(doc, path, ty, line, col) => {
            let d = emit_expr(doc, out, next, false);
            let p = emit_expr(path, out, next, false);
            let id = *next; *next += 1;
            out.push_str(&format!("    {}t{id} = ae_json_get({}.ptr, {}.len, {}.ptr, {}.len, {line}, {col});\n", ty.c_temp(), d.text, d.text, p.text, p.text));
            if p.owned { drop_owned_temp(out, &p); }
            if d.owned { drop_owned_temp(out, &d); }
            Emitted { text: format!("t{id}"), ty: *ty, owned: true, moves: Vec::new(), materialized: true }
        }
        Expr::JsonInt(doc, path, ty, line, col) => {
            let d = emit_expr(doc, out, next, false);
            let p = emit_expr(path, out, next, false);
            let id = *next; *next += 1;
            out.push_str(&format!("    {}t{id} = ae_json_int({}.ptr, {}.len, {}.ptr, {}.len, {line}, {col});\n", ty.c_temp(), d.text, d.text, p.text, p.text));
            if p.owned { drop_owned_temp(out, &p); }
            if d.owned { drop_owned_temp(out, &d); }
            Emitted { text: format!("t{id}"), ty: *ty, owned: true, moves: Vec::new(), materialized: true }
        }
        Expr::JsonBool(doc, path, ty, line, col) => {
            let d = emit_expr(doc, out, next, false);
            let p = emit_expr(path, out, next, false);
            let id = *next; *next += 1;
            out.push_str(&format!("    {}t{id} = ae_json_bool({}.ptr, {}.len, {}.ptr, {}.len, {line}, {col});\n", ty.c_temp(), d.text, d.text, p.text, p.text));
            if p.owned { drop_owned_temp(out, &p); }
            if d.owned { drop_owned_temp(out, &d); }
            Emitted { text: format!("t{id}"), ty: *ty, owned: true, moves: Vec::new(), materialized: true }
        }
        Expr::JsonLen(doc, path, ty, line, col) => {
            let d = emit_expr(doc, out, next, false);
            let p = emit_expr(path, out, next, false);
            let id = *next; *next += 1;
            out.push_str(&format!("    {}t{id} = ae_json_len({}.ptr, {}.len, {}.ptr, {}.len, {line}, {col});\n", ty.c_temp(), d.text, d.text, p.text, p.text));
            if p.owned { drop_owned_temp(out, &p); }
            if d.owned { drop_owned_temp(out, &d); }
            Emitted { text: format!("t{id}"), ty: *ty, owned: true, moves: Vec::new(), materialized: true }
        }
        Expr::JsonHas(doc, path, line, col) => {
            let d = emit_expr(doc, out, next, false);
            let p = emit_expr(path, out, next, false);
            let id = *next; *next += 1;
            out.push_str(&format!("    const bool t{id} = ae_json_has({}.ptr, {}.len, {}.ptr, {}.len, {line}, {col});\n", d.text, d.text, p.text, p.text));
            if p.owned { drop_owned_temp(out, &p); }
            if d.owned { drop_owned_temp(out, &d); }
            Emitted { text: format!("t{id}"), ty: Type::Bool, owned: false, moves: Vec::new(), materialized: true }
        }
        Expr::Env(variable, ty, line, col) => {
            let v = emit_expr(variable, out, next, false);
            let id = *next; *next += 1;
            out.push_str(&format!("    {}t{id} = ae_try_env({}.ptr, {line}, {col});\n", ty.c_temp(), v.text));
            if v.owned { drop_owned_temp(out, &v); }
            Emitted { text: format!("t{id}"), ty: *ty, owned: true, moves: Vec::new(), materialized: true }
        }
        Expr::RunCommand(program, vid, ty, line, col) => {
            out.push_str(&format!("    if (!v{vid}.data) ae_fail(\"use of moved vector\", {line}, {col});\n"));
            let p = emit_expr(program, out, next, false);
            let id = *next; *next += 1;
            out.push_str(&format!("    {}t{id} = ae_run_command({}.ptr, &v{vid}, {line}, {col});\n", ty.c_temp(), p.text));
            if p.owned { drop_owned_temp(out, &p); }
            Emitted { text: format!("t{id}"), ty: *ty, owned: true, moves: Vec::new(), materialized: true }
        }
        Expr::StrVecNew(line, col) => Emitted {
            text: format!("ae_strvec_new({line}, {col})"),
            ty: Type::StrVec, owned: true, moves: Vec::new(), materialized: false,
        },
        Expr::ReadStdin(ty, line, col) => {
            let id = *next; *next += 1;
            out.push_str(&format!("    {}t{id} = ae_try_read_stdin({line}, {col});\n", ty.c_temp()));
            Emitted { text: format!("t{id}"), ty: *ty, owned: true, moves: Vec::new(), materialized: true }
        }
        Expr::Cwd(ty, line, col) => {
            let id = *next; *next += 1;
            out.push_str(&format!("    {}t{id} = ae_try_cwd({line}, {col});\n", ty.c_temp()));
            Emitted { text: format!("t{id}"), ty: *ty, owned: true, moves: Vec::new(), materialized: true }
        }
        Expr::NowMs => {
            let id = *next; *next += 1;
            out.push_str(&format!("    int64_t t{id} = ae_now_ms();\n"));
            Emitted { text: format!("t{id}"), ty: Type::Int, owned: false, moves: Vec::new(), materialized: true }
        }
        Expr::NowS => {
            let id = *next; *next += 1;
            out.push_str(&format!("    int64_t t{id} = ae_now_s();\n"));
            Emitted { text: format!("t{id}"), ty: Type::Int, owned: false, moves: Vec::new(), materialized: true }
        }
        Expr::TryListDir(path, ty, line, col) => {
            let p = emit_expr(path, out, next, false);
            let id = *next; *next += 1;
            out.push_str(&format!("    {}t{id} = ae_try_list_dir({}.ptr, {line}, {col});\n", ty.c_temp(), p.text));
            if p.owned { drop_owned_temp(out, &p); }
            Emitted { text: format!("t{id}"), ty: *ty, owned: true, moves: Vec::new(), materialized: true }
        }
        Expr::PathJoin(dir, name, line, col) => {
            let d = emit_expr(dir, out, next, false);
            let n = emit_expr(name, out, next, false);
            let id = *next; *next += 1;
            out.push_str(&format!("    ae_string t{id} = ae_path_join(AE_VIEW({}), AE_VIEW({}), {line}, {col});\n", d.text, n.text));
            if d.owned { drop_owned_temp(out, &d); }
            if n.owned { drop_owned_temp(out, &n); }
            Emitted { text: format!("t{id}"), ty: Type::String, owned: true, moves: Vec::new(), materialized: true }
        }
        Expr::PathName(path, line, col) => {
            let p = emit_expr(path, out, next, false);
            let id = *next; *next += 1;
            out.push_str(&format!("    ae_string t{id} = ae_path_name(AE_VIEW({}), {line}, {col});\n", p.text));
            if p.owned { drop_owned_temp(out, &p); }
            Emitted { text: format!("t{id}"), ty: Type::String, owned: true, moves: Vec::new(), materialized: true }
        }
        Expr::PathParent(path, line, col) => {
            let p = emit_expr(path, out, next, false);
            let id = *next; *next += 1;
            out.push_str(&format!("    ae_string t{id} = ae_path_parent(AE_VIEW({}), {line}, {col});\n", p.text));
            if p.owned { drop_owned_temp(out, &p); }
            Emitted { text: format!("t{id}"), ty: Type::String, owned: true, moves: Vec::new(), materialized: true }
        }
        Expr::TryFileInfo(path, ty, line, col) => {
            let p = emit_expr(path, out, next, false);
            let id = *next; *next += 1;
            out.push_str(&format!("    {}t{id} = ae_try_file_info({}.ptr, {line}, {col});\n", ty.c_temp(), p.text));
            if p.owned { drop_owned_temp(out, &p); }
            Emitted { text: format!("t{id}"), ty: *ty, owned: true, moves: Vec::new(), materialized: true }
        }
        Expr::TryCopyFile(from, to, ty, line, col) => {
            let a = emit_expr(from, out, next, false);
            let b = emit_expr(to, out, next, false);
            let id = *next; *next += 1;
            out.push_str(&format!("    {}t{id} = ae_try_copy_file({}.ptr, {}.ptr, {line}, {col});\n", ty.c_temp(), a.text, b.text));
            if a.owned { drop_owned_temp(out, &a); }
            if b.owned { drop_owned_temp(out, &b); }
            Emitted { text: format!("t{id}"), ty: *ty, owned: true, moves: Vec::new(), materialized: true }
        }
        Expr::TryRenamePath(from, to, ty, line, col) => {
            let a = emit_expr(from, out, next, false);
            let b = emit_expr(to, out, next, false);
            let id = *next; *next += 1;
            out.push_str(&format!("    {}t{id} = ae_try_rename_path({}.ptr, {}.ptr, {line}, {col});\n", ty.c_temp(), a.text, b.text));
            if a.owned { drop_owned_temp(out, &a); }
            if b.owned { drop_owned_temp(out, &b); }
            Emitted { text: format!("t{id}"), ty: *ty, owned: true, moves: Vec::new(), materialized: true }
        }
        Expr::TryRemoveFile(path, ty, line, col) => {
            let p = emit_expr(path, out, next, false);
            let id = *next; *next += 1;
            out.push_str(&format!("    {}t{id} = ae_try_remove_file({}.ptr, {line}, {col});\n", ty.c_temp(), p.text));
            if p.owned { drop_owned_temp(out, &p); }
            Emitted { text: format!("t{id}"), ty: *ty, owned: true, moves: Vec::new(), materialized: true }
        }
        Expr::MapNew(line, col) => {
            let id = *next; *next += 1;
            out.push_str(&format!("    ae_map t{id} = ae_map_new({line}, {col});\n"));
            Emitted { text: format!("t{id}"), ty: Type::Map, owned: true, moves: Vec::new(), materialized: true }
        }
        Expr::MapGet(place, key, ty, line, col) => {
            let k = emit_expr(key, out, next, false);
            let id = *next; *next += 1;
            out.push_str(&format!("    {}t{id} = ae_map_get(&{}, AE_VIEW({}), {line}, {col});\n", ty.c_temp(), place.text(), k.text));
            if k.owned { drop_owned_temp(out, &k); }
            Emitted { text: format!("t{id}"), ty: *ty, owned: true, moves: Vec::new(), materialized: true }
        }
        Expr::MapHas(place, key, line, col) => {
            let k = emit_expr(key, out, next, false);
            let id = *next; *next += 1;
            out.push_str(&format!("    bool t{id} = ae_map_has(&{}, AE_VIEW({}), {line}, {col});\n", place.text(), k.text));
            if k.owned { drop_owned_temp(out, &k); }
            Emitted { text: format!("t{id}"), ty: Type::Bool, owned: false, moves: Vec::new(), materialized: true }
        }
        Expr::MapRemove(place, key, line, col) => {
            let k = emit_expr(key, out, next, false);
            let id = *next; *next += 1;
            out.push_str(&format!("    bool t{id} = ae_map_remove(&{}, AE_VIEW({}), {line}, {col});\n", place.text(), k.text));
            if k.owned { drop_owned_temp(out, &k); }
            Emitted { text: format!("t{id}"), ty: Type::Bool, owned: false, moves: Vec::new(), materialized: true }
        }
        Expr::MapKeys(place, line, col) => {
            let id = *next; *next += 1;
            out.push_str(&format!("    ae_strvec t{id} = ae_map_keys(&{}, {line}, {col});\n", place.text()));
            Emitted { text: format!("t{id}"), ty: Type::StrVec, owned: true, moves: Vec::new(), materialized: true }
        }
        Expr::TryRemoveDir(path, ty, line, col) => {
            let p = emit_expr(path, out, next, false);
            let id = *next; *next += 1;
            out.push_str(&format!("    {}t{id} = ae_try_remove_dir({}.ptr, {line}, {col});\n", ty.c_temp(), p.text));
            if p.owned { drop_owned_temp(out, &p); }
            Emitted { text: format!("t{id}"), ty: *ty, owned: true, moves: Vec::new(), materialized: true }
        }
        Expr::TryCreateDir(path, ty, line, col) => {
            let p = emit_expr(path, out, next, false);
            let id = *next; *next += 1;
            out.push_str(&format!("    {}t{id} = ae_try_create_dir({}.ptr, {line}, {col});\n", ty.c_temp(), p.text));
            if p.owned { drop_owned_temp(out, &p); }
            Emitted { text: format!("t{id}"), ty: *ty, owned: true, moves: Vec::new(), materialized: true }
        }
        Expr::SetCwd(path, ty, line, col) => {
            let p = emit_expr(path, out, next, false);
            let id = *next; *next += 1;
            out.push_str(&format!("    {}t{id} = ae_set_cwd({}.ptr, {line}, {col});\n", ty.c_temp(), p.text));
            if p.owned { drop_owned_temp(out, &p); }
            Emitted { text: format!("t{id}"), ty: *ty, owned: true, moves: Vec::new(), materialized: true }
        }
        Expr::Utf8Len(e, line, col) => {
            let t = emit_expr(e, out, next, false);
            let id = *next; *next += 1;
            out.push_str(&format!("    int64_t t{id} = ae_utf8_len(AE_VIEW({}), {line}, {col});\n", t.text));
            if t.owned { drop_owned_temp(out, &t); }
            Emitted { text: format!("t{id}"), ty: Type::Int, owned: false, moves: Vec::new(), materialized: true }
        }
        Expr::Utf8Prefix(e, count, line, col) => {
            let t = emit_expr(e, out, next, false);
            let n = emit_expr(count, out, next, false);
            let id = *next; *next += 1;
            out.push_str(&format!("    ae_string t{id} = ae_utf8_prefix(AE_VIEW({}), {}, {line}, {col});\n", t.text, n.text));
            if t.owned { drop_owned_temp(out, &t); }
            Emitted { text: format!("t{id}"), ty: Type::String, owned: true, moves: Vec::new(), materialized: true }
        }
        Expr::PathExists(path, line, col) => {
            let p = emit_expr(path, out, next, false);
            let id = *next; *next += 1;
            out.push_str(&format!("    bool t{id} = ae_path_exists({}.ptr, {line}, {col});\n", p.text));
            if p.owned { drop_owned_temp(out, &p); }
            Emitted { text: format!("t{id}"), ty: Type::Bool, owned: false, moves: Vec::new(), materialized: true }
        }
        Expr::PathIsDir(path, line, col) => {
            let p = emit_expr(path, out, next, false);
            let id = *next; *next += 1;
            out.push_str(&format!("    bool t{id} = ae_path_is_dir({}.ptr, {line}, {col});\n", p.text));
            if p.owned { drop_owned_temp(out, &p); }
            Emitted { text: format!("t{id}"), ty: Type::Bool, owned: false, moves: Vec::new(), materialized: true }
        }
        Expr::ParseInt(text, ty, line, col) => {
            let t = emit_expr(text, out, next, false);
            let id = *next; *next += 1;
            out.push_str(&format!("    {}t{id} = ae_parse_int(AE_VIEW({}), {line}, {col});\n", ty.c_temp(), t.text));
            if t.owned { drop_owned_temp(out, &t); }
            Emitted { text: format!("t{id}"), ty: *ty, owned: true, moves: Vec::new(), materialized: true }
        }
        Expr::ToString(value, line, col) => {
            let t = emit_expr(value, out, next, false);
            let id = *next; *next += 1;
            out.push_str(&format!("    ae_string t{id} = ae_to_string({}, {line}, {col});\n", t.text));
            if t.owned { drop_owned_temp(out, &t); }
            Emitted { text: format!("t{id}"), ty: Type::String, owned: true, moves: Vec::new(), materialized: true }
        }
        Expr::Field(base, fname, fty) => {
            let b = emit_expr(base, out, next, false);
            if fty.owned() {
                // Owned fields are borrows: inline, consumer never releases.
                Emitted { text: format!("{}.{}", b.text, fname), ty: *fty, owned: false, moves: b.moves, materialized: true }
            } else {
                // Scalars copy immediately: a later release of the base must
                // not clobber the value (return paths release before return).
                Emitted { text: format!("{}.{}", b.text, fname), ty: *fty, owned: false, moves: b.moves, materialized: false }
            }
        }
        Expr::StrEq(a, b, negated) => {
            let l = emit_expr(a, out, next, false);
            let r = emit_expr(b, out, next, false);
            let id = *next; *next += 1;
            let cmp = format!("ae_str_eq(AE_VIEW({}), AE_VIEW({}))", l.text, r.text);
            let text = if *negated { format!("!{cmp}") } else { cmp };
            out.push_str(&format!("    bool t{id} = {text};
"));
            if l.owned { drop_owned_temp(out, &l); }
            if r.owned { drop_owned_temp(out, &r); }
            Emitted { text: format!("t{id}"), ty: Type::Bool, owned: false, moves: Vec::new(), materialized: true }
        }
        Expr::Binary(a, op, b, line, column, ty) if *op == Kind::And || *op == Kind::Or => {
            let l = emit_expr(a, out, next, false);
            let id = *next; *next += 1;
            // `&&` starts false and fills the result only when the left side
            // is true; `||` mirrors it. The right operand's statements land
            // inside the if body, giving real short-circuit semantics.
            let initial = if *op == Kind::And { "false" } else { "true" };
            let guard = if *op == Kind::And { l.text.clone() } else { format!("!{}", l.text) };
            out.push_str(&format!("    bool t{id} = {initial};\n    if ({guard}) {{\n"));
            let r = emit_expr(b, out, next, false);
            out.push_str(&format!("    t{id} = {};\n    }}\n", r.text));
            Emitted { text: format!("t{id}"), ty: Type::Bool, owned: false, moves: Vec::new(), materialized: true }
        }
        Expr::Binary(a, op, b, line, column, ty) if *op == Kind::Plus && *ty == Type::String => {
            let l = emit_expr(a, out, next, true);
            let r = emit_expr(b, out, next, true);
            // ae_str_cat frees both operands, so only a value this
            // expression actually owns may be handed over. Views, vec elements
            // and struct fields are borrows: they are copied instead, and the
            // borrow stays valid and untouched.
            let owned_part = |part: &Emitted| if part.owned {
                part.text.clone()
            } else {
                format!("ae_str_copy_view(AE_VIEW({}), {line}, {column})", part.text)
            };
            let (left, right) = (owned_part(&l), owned_part(&r));
            let id = *next; *next += 1;
            out.push_str(&format!("    ae_string t{id} = ae_str_cat({left}, {right}, {line}, {column});\n"));
            let mut moves = l.moves; moves.extend(r.moves);
            null_moves(out, &moves);
            Emitted { text: format!("t{id}"), ty: Type::String, owned: true, moves: Vec::new(), materialized: true }
        }
        Expr::Binary(a, op, b, line, column, _) => {
            let l = emit_expr(a, out, next, false); let r = emit_expr(b, out, next, false);
            // f64 arithmetic maps to the C operators: no overflow trapping, no
            // fast-math, and division goes through a checked helper so `x / 0.0`
            // is a reported failure instead of a silent infinity (ADR-37).
            let floating = l.ty == Type::F64;
            let text = match op {
                Kind::EqEq => format!("{} == {}", l.text, r.text),
                Kind::NotEq => format!("{} != {}", l.text, r.text),
                Kind::Less => format!("{} < {}", l.text, r.text),
                Kind::LessEq => format!("{} <= {}", l.text, r.text),
                Kind::Greater => format!("{} > {}", l.text, r.text),
                Kind::GreaterEq => format!("{} >= {}", l.text, r.text),
                Kind::Plus if floating => format!("{} + {}", l.text, r.text),
                Kind::Minus if floating => format!("{} - {}", l.text, r.text),
                Kind::Star if floating => format!("{} * {}", l.text, r.text),
                Kind::Slash if floating => format!("ae_f64_div({}, {}, {line}, {column})", l.text, r.text),
                Kind::Plus => format!("ae_add({}, {}, {line}, {column})", l.text, r.text),
                Kind::Minus => format!("ae_sub({}, {}, {line}, {column})", l.text, r.text),
                Kind::Star => format!("ae_mul({}, {}, {line}, {column})", l.text, r.text),
                Kind::Slash => format!("ae_div({}, {}, {line}, {column})", l.text, r.text),
                Kind::Percent => format!("ae_mod({}, {}, {line}, {column})", l.text, r.text),
                _ => unreachable!(),
            };
            let ty = if matches!(op, Kind::EqEq | Kind::NotEq | Kind::Less | Kind::LessEq | Kind::Greater | Kind::GreaterEq) {
                Type::Bool
            } else if floating {
                Type::F64
            } else {
                Type::Int
            };
            Emitted { text, ty, owned: false, moves: Vec::new(), materialized: false }
        }
    };
    if emitted.materialized { return emitted; }
    let id = *next; *next += 1;
    out.push_str(&format!("    {}t{id} = {};\n", emitted.ty.c_temp(), emitted.text));
    Emitted { text: format!("t{id}"), owned: emitted.ty.owned(), ..emitted }
}
fn release_var(out: &mut String, id: usize, ty: Type) {
    match ty {
        Type::Vec => out.push_str(&format!("    ae_vec_release(&v{id});\n")),
        Type::StrVec => out.push_str(&format!("    ae_strvec_release(&v{id});\n")),
        Type::Map => out.push_str(&format!("    ae_map_release(&v{id});\n")),
        Type::Struct(k) => out.push_str(&format!("    ae_t{k}_release(&v{id});\n")),
        Type::Enum(_, k) => out.push_str(&format!("    ae_e{k}_release(&v{id});\n")),
        _ => out.push_str(&format!("    ae_str_release(&v{id});\n")),
    }
}
// Release one enum payload or struct field in place, addressed by an arbitrary
// C lvalue. Propagation discards Ok/Some payloads it does not bind, and those
// payloads may be any owned type, not just strings (ADR-34).
fn release_member(out: &mut String, member: &str, ty: Type) {
    match ty {
        Type::Vec => out.push_str(&format!("    ae_vec_release(&{member});\n")),
        Type::StrVec => out.push_str(&format!("    ae_strvec_release(&{member});\n")),
        Type::Map => out.push_str(&format!("    ae_map_release(&{member});\n")),
        Type::Struct(k) => out.push_str(&format!("    ae_t{k}_release(&{member});\n")),
        Type::Enum(_, k) => out.push_str(&format!("    ae_e{k}_release(&{member});\n")),
        _ => out.push_str(&format!("    ae_str_release(&{member});\n")),
    }
}
// An owned temporary (not a named slot) that a statement has finished using.
fn drop_owned_temp(out: &mut String, emitted: &Emitted) {
    match emitted.ty {
        Type::Vec => out.push_str(&format!("    ae_vec_release(&{});\n", emitted.text)),
        Type::String => out.push_str(&format!("    ae_str_release(&{});\n", emitted.text)),
        Type::StrVec => out.push_str(&format!("    ae_strvec_release(&{});\n", emitted.text)),
        Type::Map => out.push_str(&format!("    ae_map_release(&{});\n", emitted.text)),
        Type::Struct(k) => out.push_str(&format!("    ae_t{k}_release(&{});\n", emitted.text)),
        Type::Enum(_, k) => out.push_str(&format!("    ae_e{k}_release(&{});\n", emitted.text)),
        _ => out.push_str(&format!("    free({});\n", emitted.text)),
    }
}
fn null_moves(out: &mut String, moves: &[(usize, Type)]) {
    for &(id, ty) in moves {
        match ty {
            Type::Vec => out.push_str(&format!("    ae_vec_forfeit(&v{id});\n")),
            Type::String => out.push_str(&format!("    ae_str_forfeit(&v{id});\n")),
            Type::StrVec => out.push_str(&format!("    ae_strvec_forfeit(&v{id});\n")),
            Type::Map => out.push_str(&format!("    ae_map_forfeit(&v{id});\n")),
            Type::Struct(k) => out.push_str(&format!("    ae_t{k}_forfeit(&v{id});\n")),
            Type::Enum(_, k) => out.push_str(&format!("    ae_e{k}_forfeit(&v{id});\n")),
            _ => out.push_str(&format!("    v{id} = NULL;\n")),
        }
    }
}
fn emit_print(printed: &Printed, out: &mut String, next: &mut usize) {
    match printed {
        Printed::Expr(e) => {
            let t = emit_expr(e, out, next, false);
            match t.ty {
                Type::String | Type::StrView => {
                    let pv = *next; *next += 1;
                    out.push_str(&format!("    ae_strview t{pv} = AE_VIEW({}); if (fwrite(t{pv}.ptr, 1, (size_t)t{pv}.len, stdout) != (size_t)t{pv}.len || fputc('\\n', stdout) == EOF) ae_fail(\"output error\", 0, 0);\n", t.text));
                    if t.owned { drop_owned_temp(out, &t); }
                }
                Type::Bool => out.push_str(&format!("    if (printf(\"%s\\n\", {} ? \"true\" : \"false\") < 0) ae_fail(\"output error\", 0, 0);\n", t.text)),
                Type::F64 => out.push_str(&format!("    ae_f64_print({});\n", t.text)),
                _ => out.push_str(&format!("    if (printf(\"%\" PRId64 \"\\n\", {}) < 0) ae_fail(\"output error\", 0, 0);\n", t.text)),
            }
        }
        Printed::Text(text) => {
            // The literal body is already a valid C string body (escapes were
            // checked by the lexer); fputs avoids format-specifier surprises.
            out.push_str(&format!("    if (fputs(\"{text}\\n\", stdout) == EOF) ae_fail(\"output error\", 0, 0);\n"));
        }
    }
}
fn emit_statements(stmts: &[Statement], out: &mut String, next: &mut usize) {
    for stmt in stmts {
        match stmt {
            Statement::Return(e, frees) => {
                let t = emit_expr(e, out, next, true);
                // A slot the return expression moved out of no longer owns its
                // buffer; releasing it would free the returned value's memory.
                for &(id, ty) in frees {
                    if t.moves.iter().any(|&(m, _)| m == id) { continue; }
                    release_var(out, id, ty);
                }
                null_moves(out, &t.moves);
                out.push_str(&format!("    return {};\n", t.text));
            }
            Statement::Declare(id, e) => {
                let t = emit_expr(e, out, next, true);
                out.push_str(&format!("    {}v{id} = {};\n", t.ty.c_temp(), t.text));
                null_moves(out, &t.moves);
            }
            Statement::Array(id, values) => {
                let items: Vec<_> = values.iter().map(|e| emit_expr(e, out, next, false).text).collect();
                out.push_str(&format!("    int64_t v{id}[{}] = {{", items.len()));
                for item in items { out.push_str(&format!("{item},")); }
                out.push_str("};\n");
            }
            Statement::Store(id, len, index, value, line, col) => {
                let i = emit_expr(index, out, next, false);
                let checked = *next; *next += 1;
                out.push_str(&format!("    size_t t{checked} = ae_index({}, {len}, {line}, {col});\n", i.text));
                let value = emit_expr(value, out, next, false);
                out.push_str(&format!("    v{id}[t{checked}] = {};\n", value.text));
            }
            Statement::VecPush(id, value, line, col) => {
                out.push_str(&format!("    if (!v{id}.data) ae_fail(\"use of moved vector\", {line}, {col});\n"));
                let value = emit_expr(value, out, next, false);
                out.push_str(&format!("    ae_vec_push(&v{id}, {}, {line}, {col});\n", value.text));
                null_moves(out, &value.moves);
            }
            Statement::Sort(id, strings, line, col) => {
                out.push_str(&format!("    if (!v{id}.data) ae_fail(\"use of moved vector\", {line}, {col});\n"));
                out.push_str(&format!("    ae_{}_sort(&v{id});\n", if *strings { "strvec" } else { "vec" }));
            }
            Statement::Reverse(id, strings, line, col) => {
                out.push_str(&format!("    if (!v{id}.data) ae_fail(\"use of moved vector\", {line}, {col});\n"));
                out.push_str(&format!("    ae_{}_reverse(&v{id});\n", if *strings { "strvec" } else { "vec" }));
            }
            Statement::VecPop(id, line, col) => {
                out.push_str(&format!("    if (!v{id}.data) ae_fail(\"use of moved vector\", {line}, {col});\n"));
                out.push_str(&format!("    ae_vec_pop(&v{id}, {line}, {col});\n"));
            }
            Statement::VecStore(id, index, value, line, col) => {
                out.push_str(&format!("    if (!v{id}.data) ae_fail(\"use of moved vector\", {line}, {col});\n"));
                // The index is computed and checked before the right side, as
                // with fixed arrays.
                let i = emit_expr(index, out, next, false);
                let checked = *next; *next += 1;
                out.push_str(&format!("    int64_t t{checked} = ae_vec_index({}, v{id}.len, {line}, {col});\n", i.text));
                let value = emit_expr(value, out, next, false);
                out.push_str(&format!("    v{id}.data[t{checked}] = {};\n", value.text));
            }
            Statement::StrVecPush(id, value, line, col, borrowed) => {
                out.push_str(&format!("    if (!v{id}.data) ae_fail(\"use of moved vector\", {line}, {col});\n"));
                // A borrowed element is copied into the vector; only an owned
                // string is moved, because moving a view would empty the slot
                // it was read from.
                let value = emit_expr(value, out, next, !*borrowed);
                let text = if *borrowed { format!("ae_str_copy_view(AE_VIEW({}), {line}, {col})", value.text) } else { value.text.clone() };
                out.push_str(&format!("    ae_strvec_push(&v{id}, {text}, {line}, {col});\n"));
                null_moves(out, &value.moves);
            }
            Statement::StrVecPop(id, line, col) => {
                out.push_str(&format!("    if (!v{id}.data) ae_fail(\"use of moved vector\", {line}, {col});\n"));
                out.push_str(&format!("    ae_strvec_pop(&v{id}, {line}, {col});\n"));
            }
            Statement::StrVecStore(id, index, value, line, col, borrowed) => {
                out.push_str(&format!("    if (!v{id}.data) ae_fail(\"use of moved vector\", {line}, {col});\n"));
                let i = emit_expr(index, out, next, false);
                let checked = *next; *next += 1;
                out.push_str(&format!("    int64_t t{checked} = ae_strvec_index({}, v{id}.len, {line}, {col});\n", i.text));
                let value = emit_expr(value, out, next, !*borrowed);
                let text = if *borrowed { format!("ae_str_copy_view(AE_VIEW({}), {line}, {col})", value.text) } else { value.text.clone() };
                out.push_str(&format!("    ae_str_release(&v{id}.data[t{checked}]);\n"));
                out.push_str(&format!("    v{id}.data[t{checked}] = {text};\n"));
                null_moves(out, &value.moves);
            }
            Statement::FieldStore(id, fname, fty, value) => {
                // Overwriting an owned field releases the old value first.
                let release_old = match fty {
                    Type::String => format!("    ae_str_release(&v{id}.{});\n", fname),
                    Type::Vec => format!("    ae_vec_release(&v{id}.{});\n", fname),
                    Type::StrVec => format!("    ae_strvec_release(&v{id}.{});\n", fname),
                    Type::Map => format!("    ae_map_release(&v{id}.{});\n", fname),
                    Type::Struct(k) => format!("    ae_t{k}_release(&v{id}.{});\n", fname),
                    Type::Enum(_, k) => format!("    ae_e{k}_release(&v{id}.{});\n", fname),
                    _ => String::new(),
                };
                out.push_str(&release_old);
                let value = emit_expr(value, out, next, false);
                out.push_str(&format!("    v{id}.{} = {};\n", fname, value.text));
                null_moves(out, &value.moves);
            }
            Statement::Assign(id, e, release_old) => {
                let t = emit_expr(e, out, next, true);
                if *release_old { release_var(out, *id, t.ty); }
                out.push_str(&format!("    v{id} = {};\n", t.text));
                null_moves(out, &t.moves);
            }
            Statement::Print(printed) => emit_print(printed, out, next),
            Statement::Exit(code, frees) => {
                let value = emit_expr(code, out, next, false);
                for &(id, ty) in frees { release_var(out, id, ty); }
                out.push_str(&format!("    ae_exit({});\n", value.text));
            }
            Statement::Assert(condition, line, col) => {
                let t = emit_expr(condition, out, next, false);
                out.push_str(&format!("    if (!({})) ae_fail(\"assertion failed\", {line}, {col});\n", t.text));
            }
            Statement::If(condition, yes, no) => {
                let cond = emit_expr(condition, out, next, false);
                out.push_str(&format!("    if ({}) {{\n", cond.text)); emit_statements(yes, out, next);
                out.push_str("    } else {\n"); emit_statements(no, out, next); out.push_str("    }\n");
            }
            Statement::While(condition, body) => {
                out.push_str("    while (1) {\n");
                let cond = emit_expr(condition, out, next, false);
                out.push_str(&format!("    if (!({})) break;\n", cond.text));
                emit_statements(body, out, next); out.push_str("    }\n");
            }
            // The while(1) form re-evaluates the condition after `continue`.
            Statement::Break => out.push_str("    break;\n"),
            Statement::Continue(label) => match label {
                // In a for loop, continue must still run the increment.
                Some(label) => out.push_str(&format!("    goto ae_next_{label};\n")),
                None => out.push_str("    continue;\n"),
            },
            // `m[key] = value;` copies the key into the table, so the key
            // expression may be a temporary or a view.
            Statement::MapPut(place, key, value, line, col) => {
                let k = emit_expr(key, out, next, false);
                let v = emit_expr(value, out, next, false);
                out.push_str(&format!("    ae_map_put(&{}, AE_VIEW({}), {}, {line}, {col});\n", place.text(), k.text, v.text));
                null_moves(out, &k.moves);
                null_moves(out, &v.moves);
                if k.owned { drop_owned_temp(out, &k); }
            }
            // Propagating a result returns the error value itself after every
            // live owned value is released, so the failure path owns nothing
            // the caller could reach. The Ok payload moves out of the enum and
            // the enum is then forfeited, which is what keeps it from being
            // freed twice.
            Statement::Propagate(value, ty, fields, binding, frees) => {
                let t = emit_expr(value, out, next, true);
                let eid = match ty { Type::Enum(_, k) => *k, _ => unreachable!("propagation is enum-typed") };
                null_moves(out, &t.moves);
                out.push_str(&format!("    if ({}.tag == AE_E{eid}_1) {{\n", t.text));
                for &(id, vty) in frees {
                    if t.moves.iter().any(|&(m, _)| m == id) { continue; }
                    release_var(out, id, vty);
                }
                out.push_str(&format!("        return {};\n    }}\n", t.text));
                match binding {
                    Some((id, pty)) => out.push_str(&format!("    {}v{id} = {}.u.v0.p0;\n", pty.c_temp(), t.text)),
                    None => {
                        for (index, field) in fields.iter().enumerate() {
                            if !field.owned() { continue; }
                            release_member(out, &format!("{}.u.v0.p{index}", t.text), *field);
                        }
                    }
                }
                out.push_str(&format!("    ae_e{eid}_forfeit(&{});\n", t.text));
            }
            // Arms become an if/else chain rather than a C switch: a `break`
            // in an arm body must leave the enclosing loop, which a switch
            // would swallow. Coverage is exhaustive, so no fall-through case
            // is needed.
            Statement::Match(subject, subject_ty, arms) => {
                let eid = match subject_ty { Type::Enum(_, k) => *k, _ => unreachable!("match subjects are enum-typed") };
                let mut first = true;
                for arm in arms {
                    let test = format!("v{subject}.tag == AE_E{eid}_{}", arm.variant);
                    if first { out.push_str(&format!("    if ({test}) {{\n")); } else { out.push_str(&format!("    }} else if ({test}) {{\n")); }
                    first = false;
                    for &(bid, pty, index) in &arm.bindings {
                        let member = format!("v{subject}.u.v{}.p{index}", arm.variant);
                        match pty {
                            Type::String => out.push_str(&format!("    ae_strview v{bid} = AE_VIEW({member});\n")),
                            Type::Bool => out.push_str(&format!("    bool v{bid} = {member};\n")),
                            Type::F64 => out.push_str(&format!("    double v{bid} = {member};\n")),
                            // Vector payloads bind as read-only borrows of the slot.
                            Type::Vec => out.push_str(&format!("    const ae_vec *v{bid} = &{member};\n")),
                            Type::StrVec => out.push_str(&format!("    const ae_strvec *v{bid} = &{member};\n")),
                            _ => out.push_str(&format!("    int64_t v{bid} = {member};\n")),
                        }
                    }
                    emit_statements(&arm.body, out, next);
                }
                out.push_str("    }\n");
            }
            Statement::For(var, end_var, body, label, line, col) => {
                out.push_str("    while (1) {\n");
                out.push_str(&format!("    if (!(v{var} < v{end_var})) break;\n"));
                emit_statements(body, out, next);
                out.push_str(&format!("    ae_next_{label}: ;\n"));
                out.push_str(&format!("    v{var} = ae_add(v{var}, 1, {line}, {col});\n"));
                out.push_str("    }\n");
            }
            Statement::Frees(ids) => { for &(id, ty) in ids { release_var(out, id, ty); } }
        }
    }
}
#[derive(Debug, Default)]
struct Program { files: Vec<Vec<Function>>, structs: Vec<StructDef>, enums: Vec<EnumDef> }

fn make_parser<'a>(file: &str, source: &'a str, tokens: Vec<Token>, functions: &'a mut SharedFunctions, structs: &'a mut Vec<StructDef>, struct_names: &'a mut HashMap<String, usize>, enums: &'a mut Vec<EnumDef>, enum_names: &'a mut HashMap<String, usize>, generics: &'a mut Generics) -> Parser<'a> {
    Parser { file: file.to_owned(), source, tokens, pos: 0, names: HashMap::new(), next_var: 0, expression_depth: 0, expression_nodes: 0, block_depth: 0, loop_depth: 0, loop_scopes: Vec::new(), loop_labels: Vec::new(), next_label: 0, functions, structs, struct_names, enums, enum_names, generics, id_order: Vec::new(), in_main: false, return_type: Type::Int, moved: std::collections::HashSet::new(), borrows: Vec::new(), scopes: Vec::new(), subst: HashMap::new(), pending: Vec::new() }
}

// Leading `use "path";` headers. Parsed from tokens before the Parser owns
// the shared function table, so imports can be loaded depth-first.
fn scan_uses(file: &str, tokens: &[Token]) -> Result<Vec<String>, String> {
    let mut uses = Vec::new();
    let mut pos = 0;
    while matches!(tokens.get(pos).map(|t| &t.kind), Some(Kind::Use)) {
        pos += 1;
        match tokens.get(pos).map(|t| t.kind.clone()) {
            Some(Kind::Text(text)) => { uses.push(text); pos += 1; }
            _ => {
                let offset = tokens.get(pos).map(|t| t.offset).unwrap_or(0);
                return Err(error(file, "", offset, "expected a string path after 'use'"));
            }
        }
        match tokens.get(pos).map(|t| &t.kind) {
            Some(Kind::Semi) => pos += 1,
            _ => {
                let offset = tokens.get(pos).map(|t| t.offset).unwrap_or(0);
                return Err(error(file, "", offset, "expected ';' after 'use' path"));
            }
        }
    }
    Ok(uses)
}

#[allow(dead_code)] // exercised by in-memory unit tests
fn parse_source(file: &str, source: &str, shared: &mut SharedFunctions, structs: &mut Vec<StructDef>, struct_names: &mut HashMap<String, usize>, enums: &mut Vec<EnumDef>, enum_names: &mut HashMap<String, usize>, generics: &mut Generics, next_id: &mut usize) -> Result<Vec<Function>, String> {
    let tokens = lex(source)?;
    let uses = scan_uses(file, &tokens)?;
    if !uses.is_empty() {
        return Err(error(file, source, 0, "'use' is only supported when compiling a source file"));
    }
    let mut p = make_parser(file, source, tokens, shared, structs, struct_names, enums, enum_names, generics);
    p.collect_functions(next_id)?;
    let functions = p.program()?;
    if functions.iter().filter(|f| f.is_main).count() != 1 {
        return Err(error(file, source, 0, "missing main function"));
    }
    Ok(functions)
}

/// One source file in the import graph. `raw` matches the `file` field stored
/// on declarations; `label` is a platform-independent name for reports.
struct ModuleInfo { raw: String, label: String, uses: Vec<String> }

fn module_label(path: &Path, root: &Path) -> String {
    let rel = path.strip_prefix(root).unwrap_or(path);
    rel.components().map(|part| part.as_os_str().to_string_lossy().into_owned()).collect::<Vec<_>>().join("/")
}

#[allow(clippy::too_many_arguments)]
fn load_file(path: &Path, root: &Path, visited: &mut std::collections::HashSet<std::path::PathBuf>, shared: &mut SharedFunctions, structs: &mut Vec<StructDef>, struct_names: &mut HashMap<String, usize>, enums: &mut Vec<EnumDef>, enum_names: &mut HashMap<String, usize>, generics: &mut Generics, next_id: &mut usize, out: &mut Vec<Vec<Function>>, mains: &mut usize, modules: &mut Vec<ModuleInfo>) -> Result<(), String> {
    let canonical = std::fs::canonicalize(path).map_err(|e| format!("cannot open {}: {e}", path.display()))?;
    if !visited.insert(canonical) { return Err(format!("circular import of {}", path.display())); }
    let source = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let file = path.display().to_string();
    let tokens = lex(&source).map_err(|e| format!("{file}:{e}"))?;
    let uses = scan_uses(&file, &tokens).map_err(|e| format!("{file}:{e}"))?;
    let parent = path.parent().unwrap_or(Path::new(".")).to_owned();
    for import in &uses {
        load_file(&parent.join(import), root, visited, shared, structs, struct_names, enums, enum_names, generics, next_id, out, mains, modules)?;
    }
    let mut p = make_parser(&file, &source, tokens, shared, structs, struct_names, enums, enum_names, generics);
    p.skip_uses();
    p.collect_functions(next_id)?;
    let functions = p.program()?;
    *mains += functions.iter().filter(|f| f.is_main).count();
    out.push(functions);
    modules.push(ModuleInfo {
        raw: file,
        label: module_label(path, root),
        uses: uses.iter().map(|u| u.replace('\\', "/")).collect(),
    });
    Ok(())
}

struct Loaded {
    program: Program,
    shared: SharedFunctions,
    generics: Generics,
    modules: Vec<ModuleInfo>,
}

fn load_program_full(entry: &Path) -> Result<Loaded, String> {
    let root = entry.parent().filter(|p| !p.as_os_str().is_empty()).unwrap_or(Path::new("."));
    let mut visited = std::collections::HashSet::new();
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
    load_file(entry, root, &mut visited, &mut shared, &mut structs, &mut struct_names, &mut enums, &mut enum_names, &mut generics, &mut next_id, &mut files, &mut mains, &mut modules)?;
    if mains != 1 { return Err(format!("program must define exactly one main function (found {mains})")); }
    Ok(Loaded { program: Program { files, structs, enums }, shared, generics, modules })
}

fn load_program(entry: &Path) -> Result<Program, String> {
    load_program_full(entry).map(|loaded| loaded.program)
}

pub fn check_path(path: &Path) -> Result<(), String> {
    load_program(path).map(|_| ())
}

/// A stable, platform-independent description of the import graph and every
/// module's public interface. Function bodies, comments and local variables
/// are deliberately absent: changing one must not change any interface hash.
/// This is a diagnosis and planning tool for package-level incrementality; it
/// does not yet participate in a cache decision, because the backend still
/// emits one translation unit per program.
pub fn describe_path(entry: &Path) -> Result<String, String> {
    let loaded = load_program_full(entry)?;
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
fn c_type_field(ty: Type) -> String {
    match ty {
        Type::Int => "int64_t".to_owned(),
        Type::Bool => "bool".to_owned(),
        Type::F64 => "double".to_owned(),
        Type::String => "ae_string".to_owned(),
        Type::StrView => "ae_strview".to_owned(),
        Type::Vec => "ae_vec".to_owned(),
        Type::StrVec => "ae_strvec".to_owned(),
        Type::Map => "ae_map".to_owned(),
        Type::Struct(k) => format!("ae_t{k}"),
        Type::Enum(_, k) => format!("ae_e{k}"),
        Type::VecRef => "const ae_vec *".to_owned(),
        Type::StrVecRef => "const ae_strvec *".to_owned(),
        Type::Array(n) => format!("int64_t [{n}]"),
    }
}
fn c_type(ty: Type, id: usize, declaration: bool) -> String {
    match ty {
        Type::Int => format!("int64_t v{id}"),
        Type::Bool => format!("bool v{id}"),
        Type::F64 => format!("double v{id}"),
        Type::String => format!("ae_string v{id}"),
        Type::StrView => format!("ae_strview v{id}"),
        Type::Vec => format!("ae_vec v{id}"),
        Type::StrVec => format!("ae_strvec v{id}"),
        Type::Map => format!("ae_map v{id}"),
        Type::Struct(k) => format!("ae_t{k} v{id}"),
        Type::Enum(_, k) => format!("ae_e{k} v{id}"),
        Type::VecRef if declaration => format!("const ae_vec *v{id}"),
        Type::VecRef => "const ae_vec *".to_owned(),
        Type::StrVecRef if declaration => format!("const ae_strvec *v{id}"),
        Type::StrVecRef => "const ae_strvec *".to_owned(),
        Type::Array(_) if declaration => format!("const int64_t *v{id}"),
        Type::Array(_) => "const int64_t *".to_owned(),
    }
}
// Return type of a generated C function: the same spelling the expression
// temporaries use, without the variable name.
fn c_return(ty: Type) -> String {
    match ty {
        Type::Bool => "bool".to_owned(),
        Type::F64 => "double".to_owned(),
        Type::String => "ae_string".to_owned(),
        Type::StrView => "ae_strview".to_owned(),
        Type::Vec => "ae_vec".to_owned(),
        Type::StrVec => "ae_strvec".to_owned(),
        Type::Map => "ae_map".to_owned(),
        Type::Struct(k) => format!("ae_t{k}"),
        Type::Enum(_, k) => format!("ae_e{k}"),
        _ => "int64_t".to_owned(),
    }
}
/// `full` is the self-contained C translation unit that `emit-c` prints.
/// `runtime_len` marks where the program-specific suffix begins: the runtime
/// part is the reachable subset of the fixed runtime prefix, the suffix holds
/// user layouts and functions. Both views come from one string so they cannot
/// disagree about what the program means.
struct GeneratedC { full: String, runtime_len: usize }

#[derive(Clone, Copy, PartialEq)]
enum CScan { Code, Str, Char, Block }

/// Count braces outside strings, character literals and comments. Runtime code
/// contains JSON examples and format strings, so a literal '{' must never be
/// mistaken for the start of a function body.
fn c_brace_delta(line: &str, state: &mut CScan) -> i32 {
    let bytes = line.as_bytes();
    let mut i = 0usize;
    let mut delta = 0i32;
    while i < bytes.len() {
        let b = bytes[i];
        let next = bytes.get(i + 1).copied().unwrap_or(0);
        match *state {
            CScan::Block => { if b == b'*' && next == b'/' { *state = CScan::Code; i += 2; } else { i += 1; } }
            CScan::Str => { if b == b'\\' { i += 2; } else { if b == b'"' { *state = CScan::Code; } i += 1; } }
            CScan::Char => { if b == b'\\' { i += 2; } else { if b == b'\'' { *state = CScan::Code; } i += 1; } }
            CScan::Code => {
                if b == b'/' && next == b'*' { *state = CScan::Block; i += 2; }
                else if b == b'/' && next == b'/' { break; }
                else if b == b'"' { *state = CScan::Str; i += 1; }
                else if b == b'\'' { *state = CScan::Char; i += 1; }
                else { if b == b'{' { delta += 1; } else if b == b'}' { delta -= 1; } i += 1; }
            }
        }
    }
    delta
}

/// Recognise the single-line `static <ret> ae_xxx(<args>) {` form used by
/// every runtime function. Returns (specifier prefix, signature, name, tail).
fn runtime_function_start(line: &str) -> Option<(&str, &str, &str, &str)> {
    let at = line.find("static ")?;
    let head = &line[..at];
    if !head.trim().is_empty() && head.trim() != "_Noreturn" { return None; }
    let rest = &line[at + "static ".len()..];
    let open = rest.find('(')?;
    let close = open + rest[open..].find(')')?;
    let brace = close + rest[close..].find('{')?;
    if !rest[close + 1..brace].trim().is_empty() { return None; }
    let name = &rest[..open];
    let name = &name[name.rfind("ae_")?..];
    if !name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_') { return None; }
    Some((head, rest[..brace].trim_end(), name, &rest[brace + 1..]))
}

/// Split the runtime prefix into top-level units. `Some(name)` is a complete
/// function definition; `None` is any other text (includes, macros, typedefs,
/// static data, comments) that must be kept verbatim. Anything not recognised
/// as a function lands in a `None` unit, which is the conservative outcome.
fn runtime_units(prefix: &str) -> Vec<(Option<String>, String)> {
    let mut units: Vec<(Option<String>, String)> = Vec::new();
    let mut pending = String::new();
    let mut depth = 0i32;
    let mut state = CScan::Code;
    let mut in_function = false;
    for line in prefix.split_inclusive('\n') {
        let raw = line.strip_suffix('\n').unwrap_or(line);
        if !in_function {
            if let Some((head, signature, name, tail)) = runtime_function_start(raw) {
                if !pending.is_empty() {
                    units.push((None, std::mem::take(&mut pending)));
                }
                let mut text = String::new();
                text.push_str(head); text.push_str(signature); text.push('{'); text.push_str(tail); text.push('\n');
                units.push((Some(name.to_owned()), text));
                let mut first = String::from("{");
                first.push_str(tail);
                state = CScan::Code;
                depth = c_brace_delta(&first, &mut state);
                in_function = depth > 0;
                continue;
            }
            pending.push_str(line);
            continue;
        }
        units.last_mut().unwrap().1.push_str(line);
        depth += c_brace_delta(raw, &mut state);
        if depth <= 0 { depth = 0; in_function = false; state = CScan::Code; }
    }
    if !pending.is_empty() { units.push((None, pending)); }
    units
}

/// Every `ae_...` identifier in a piece of C text. Used both for roots and
/// for the transitive dependency walk; a symbol that names no function is
/// harmless (it is a type, macro or a name in a comment).
fn ae_symbols(text: &str) -> Vec<String> {
    let bytes = text.as_bytes();
    let mut found = Vec::new();
    let mut i = 0usize;
    while i + 7 <= bytes.len() {
        if &bytes[i..i + 7] == b"ae_" && (i == 0 || !(bytes[i - 1].is_ascii_alphanumeric() || bytes[i - 1] == b'_')) {
            let mut end = i + 7;
            while end < bytes.len() && (bytes[end].is_ascii_alphanumeric() || bytes[end] == b'_') { end += 1; }
            found.push(text[i..end].to_owned());
            i = end;
            continue;
        }
        i += 1;
    }
    found
}

/// Keep only runtime functions reachable from the program. Roots are the
/// symbols in the program suffix plus every symbol in the always-retained
/// non-function text (macro bodies and static initialisers can call functions
/// that the generated code never names directly). The walk then follows calls
/// between retained functions. Dropping an unreachable function cannot change
/// behaviour: it has no caller in this translation unit.
fn prune_runtime(prefix: &str, suffix: &str) -> (String, usize) {
    let units = runtime_units(prefix);
    // A name can have several definitions under #ifdef/_WIN32 branches. The
    // scanner does not run the preprocessor, so every definition must be kept
    // and every body scanned: keeping only the last one would silently drop a
    // helper that the Windows branch calls.
    let mut definitions: HashMap<String, Vec<usize>> = HashMap::new();
    for (index, (name, _)) in units.iter().enumerate() {
        if let Some(name) = name { definitions.entry(name.clone()).or_default().push(index); }
    }
    let mut keep: HashSet<String> = HashSet::new();
    let mut queue: Vec<String> = Vec::new();
    queue.extend(ae_symbols(suffix));
    for (name, text) in &units {
        if name.is_none() { queue.extend(ae_symbols(text)); }
    }
    // Hosts embedding the generated C may substitute the allocator without
    // the generated code naming it; keep that documented entry point.
    queue.push("ae_set_allocator".to_owned());
    while let Some(symbol) = queue.pop() {
        if !keep.insert(symbol.clone()) { continue; }
        if let Some(indices) = definitions.get(&symbol) {
            for &index in indices {
                for next in ae_symbols(&units[index].1) {
                    if !keep.contains(&next) { queue.push(next); }
                }
            }
        }
    }
    let mut out = String::new();
    for (name, text) in &units {
        if name.is_none() || name.as_ref().is_some_and(|n| keep.contains(n)) { out.push_str(text); }
    }
    let runtime_len = out.len();
    out.push_str(suffix);
    (out, runtime_len)
}

fn emit_enum_layouts(enums: &[EnumDef], from: usize, to: usize, out: &mut String) {
    // Enum layouts: a tag plus a union of per-variant payload records. Each
    // variant gets its own struct so a variant can carry several fields at
    // once (a union of individual fields would alias them). Release frees the
    // live variant's owned payloads (idempotently); forfeit only clears them
    // because the value moved elsewhere.
    for eid in from..to {
        let def = &enums[eid];
        let tags = (0..def.variants.len()).map(|v| format!("    AE_E{eid}_{v} = {v}")).collect::<Vec<_>>().join(",\n");
        out.push_str(&format!("\ntypedef enum {{\n{tags}\n}} ae_e{eid}_tag;\n"));
        let mut members = String::new();
        for (vidx, (_, fields)) in def.variants.iter().enumerate() {
            if fields.is_empty() { continue; }
            let mut record = String::new();
            for (index, ty) in fields.iter().enumerate() {
                record.push_str(&format!("    {} p{index};\n", c_type_field(*ty)));
            }
            out.push_str(&format!("typedef struct {{\n{record}}} ae_e{eid}_v{vidx};\n"));
            members.push_str(&format!("        ae_e{eid}_v{vidx} v{vidx};\n"));
        }
        // A union needs at least one member; enums without payloads get a
        // placeholder so the initializer form stays uniform.
        if members.is_empty() { members.push_str("        char ae_empty;\n"); }
        out.push_str(&format!("typedef struct {{\n    ae_e{eid}_tag tag;\n    union {{\n{members}    }} u;\n}} ae_e{eid};\n"));
        let mut release = String::new();
        let mut forfeit = String::new();
        for (vidx, (_, fields)) in def.variants.iter().enumerate() {
            if fields.is_empty() { continue; }
            let mut clears = String::new();
            let mut forfeits = String::new();
            for (index, ty) in fields.iter().enumerate() {
                let member = format!("v->u.v{vidx}.p{index}");
                match ty {
                    Type::String => {
                        clears.push_str(&format!("        ae_str_release(&{member});\n"));
                        forfeits.push_str(&format!("        ae_str_forfeit(&{member});\n"));
                    }
                    Type::Vec => {
                        clears.push_str(&format!("        ae_vec_release(&{member});\n"));
                        forfeits.push_str(&format!("        ae_vec_forfeit(&{member});\n"));
                    }
                    Type::StrVec => {
                        clears.push_str(&format!("        ae_strvec_release(&{member});\n"));
                        forfeits.push_str(&format!("        ae_strvec_forfeit(&{member});\n"));
                    }
                    Type::Map => {
                        clears.push_str(&format!("        ae_map_release(&{member});\n"));
                        forfeits.push_str(&format!("        ae_map_forfeit(&{member});\n"));
                    }
                    Type::Bool => {
                        clears.push_str(&format!("        {member} = false;\n"));
                        forfeits.push_str(&format!("        {member} = false;\n"));
                    }
                    _ => {
                        clears.push_str(&format!("        {member} = 0;\n"));
                        forfeits.push_str(&format!("        {member} = 0;\n"));
                    }
                }
            }
            release.push_str(&format!("    if (v->tag == AE_E{eid}_{vidx}) {{\n{clears}    }}\n"));
            forfeit.push_str(&format!("    if (v->tag == AE_E{eid}_{vidx}) {{\n{forfeits}    }}\n"));
        }
        out.push_str(&format!("static void ae_e{eid}_release(ae_e{eid} *v) {{\n{release}}}\n"));
        out.push_str(&format!("static void ae_e{eid}_forfeit(ae_e{eid} *v) {{\n{forfeit}}}\n"));
    }
}

fn emit_program(program: Program) -> Result<GeneratedC, String> {
    let Program { files, structs, enums } = program;
    let prelude_count = prelude_enums().0.len();
    // Fallible built-ins and platform helpers name prelude enum types, so the
    // prelude layouts come before runtime_ex.c. Their order is checked here
    // rather than trusted; a reordered prelude would silently change payload
    // ids in generated code.
    let names: Vec<&str> = enums.iter().take(6).map(|def| def.name).collect();
    if names != ["ReadResult", "WriteResult", "IntResult", "DirResult", "FileResult", "StatusResult"] {
        return Err(format!("internal: prelude enums must be registered first and in order, found {names:?}"));
    }
    let mut out = String::from(include_str!("runtime.c"));
    emit_enum_layouts(&enums, 0, prelude_count, &mut out);
    out.push_str(include_str!("runtime_ex.c"));
    // Everything above is byte-identical for every program: no user enum,
    // struct, function or literal reaches it. Keeping the cut here lets the
    // build driver reuse one compiled runtime object across programs.
    let runtime_len = out.len();
    // User enums are emitted after the runtime prefix. runtime_ex.c only names
    // prelude ids, so this does not change any type it can reference.
    emit_enum_layouts(&enums, prelude_count, enums.len(), &mut out);
    // Struct layouts and their ownership helpers. Release zeroes every field
    // and frees owned ones; forfeit only zeroes (the value moved elsewhere).
    for (sid, def) in structs.iter().enumerate() {
        let mut layout = String::new();
        for (n, ty) in &def.fields {
            layout.push_str(&format!("    {} {};\n", c_type_field(*ty), n));
        }
        let mut release = String::new();
        let mut forfeit = String::new();
        for (n, ty) in &def.fields {
            match ty {
                Type::String => {
                    release.push_str(&format!("    ae_str_release(&v->{});\n", n));
                    forfeit.push_str(&format!("    ae_str_forfeit(&v->{});\n", n));
                }
                Type::Vec => {
                    release.push_str(&format!("    ae_vec_release(&v->{});\n", n));
                    forfeit.push_str(&format!("    ae_vec_forfeit(&v->{});\n", n));
                }
                Type::Struct(k) => {
                    release.push_str(&format!("    ae_t{k}_release(&v->{});\n", n));
                    forfeit.push_str(&format!("    ae_t{k}_forfeit(&v->{});\n", n));
                }
                Type::StrVec => {
                    release.push_str(&format!("    ae_strvec_release(&v->{});\n", n));
                    forfeit.push_str(&format!("    ae_strvec_forfeit(&v->{});\n", n));
                }
                Type::Map => {
                    release.push_str(&format!("    ae_map_release(&v->{});\n", n));
                    forfeit.push_str(&format!("    ae_map_forfeit(&v->{});\n", n));
                }
                Type::Enum(_, k) => {
                    release.push_str(&format!("    ae_e{k}_release(&v->{});\n", n));
                    forfeit.push_str(&format!("    ae_e{k}_forfeit(&v->{});\n", n));
                }
                other => {
                    let zero = match other { Type::Bool => "false", _ => "0" };
                    release.push_str(&format!("    v->{} = {};\n", n, zero));
                    forfeit.push_str(&format!("    v->{} = {};\n", n, zero));
                }
            }
        }
        out.push_str(&format!("\ntypedef struct {{\n{layout}}} ae_t{sid};\n"));
        out.push_str(&format!("static void ae_t{sid}_release(ae_t{sid} *v) {{\n{release}}}\n"));
        out.push_str(&format!("static void ae_t{sid}_forfeit(ae_t{sid} *v) {{\n{forfeit}}}\n"));
    }
    let all: Vec<&Function> = files.iter().flatten().collect();
    for function in &all {
        if function.is_main { continue; }
        let params = if function.params.is_empty() { "void".to_owned() } else { function.params.iter().map(|&(id, ty)| c_type(ty, id, false)).collect::<Vec<_>>().join(", ") };
        let ret = c_return(function.returns);
        out.push_str(&format!("\nstatic {ret} ae_fn_{}({params});\n", function.id));
    }
    for function in &all {
        if function.is_main { continue; }
        let params = if function.params.is_empty() { "void".to_owned() } else { function.params.iter().map(|&(id, ty)| c_type(ty, id, true)).collect::<Vec<_>>().join(", ") };
        let ret = c_return(function.returns);
        out.push_str(&format!("\nstatic {ret} ae_fn_{}({params}) {{\n", function.id));
        emit_statements(&function.body, &mut out, &mut 0);
        // Defensive backstop: even a future control-flow analysis bug must not
        // produce a C function that falls through without returning a value.
        out.push_str("    ae_fail(\"function exited without return\", 0, 0);\n}\n");
    }
    let main = all.iter().find(|f| f.is_main).expect("load_program checks exactly one main");
    out.push_str("\nint main(int argc, char **argv) {\n    ae_argc_g = (int64_t)argc;\n    ae_argv_g = argv;\n");
    emit_statements(&main.body, &mut out, &mut 0);
    out.push_str("    if (fflush(stdout) == EOF) return 1;\n    return 0;\n}\n");
    // The fixed prefix is program-independent; emit only its reachable part.
    // `split_off` avoids copying the 100 KB runtime text.
    let user = out.split_off(runtime_len);
    let (full, pruned_len) = prune_runtime(&out, &user);
    Ok(GeneratedC { full, runtime_len: pruned_len })
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
}
