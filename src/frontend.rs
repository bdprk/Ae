use std::collections::HashMap;
use std::path::Path;

#[derive(Debug, Clone, PartialEq)]
enum Kind {
    Ident(String), Number(i64), Text(String), Fn, Let, Var, Print, If, Else, While, For, In, Return, Break, Continue, Assert, True, False, Use,
    EqEq, NotEq, Less, LessEq, Greater, GreaterEq, And, Or, Bang,
    LParen, RParen, LBrace, RBrace, LBracket, RBracket, Comma, Colon, Arrow, Amp, Semi, Equal, Plus, Minus, Star, Slash, Percent, Dot, DotDot, Struct,
    Enum, Match, ColonColon, FatArrow, Eof,
}
#[derive(Debug, Clone)]
struct Token { kind: Kind, offset: usize, line: usize, column: usize }

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Type { Int, Bool, String, StrView, Vec, StrVec, VecRef, Array(usize), Struct(usize), Enum(&'static str, usize) }
impl Type {
    fn name(self) -> String {
        match self {
            Type::Int => "i64".to_owned(),
            Type::Bool => "bool".to_owned(),
            Type::String => "string".to_owned(),
            Type::StrView => "string view".to_owned(),
            Type::Vec => "vec".to_owned(),
            Type::StrVec => "vec[string]".to_owned(),
            Type::VecRef => "&vec".to_owned(),
            Type::Array(n) => format!("&[i64; {n}]"),
            Type::Struct(id) => format!("struct #{}", id),
            Type::Enum(name, _) => name.to_owned(),
        }
    }
    fn c_temp(self) -> String {
        match self {
            Type::Bool => "bool ".to_owned(),
            Type::String => "ae_string ".to_owned(),
            Type::StrView => "ae_strview ".to_owned(),
            Type::Vec => "ae_vec ".to_owned(),
            Type::StrVec => "ae_strvec ".to_owned(),
            Type::Struct(k) => format!("ae_t{k} "),
            Type::Enum(_, k) => format!("ae_e{k} "),
            _ => "int64_t ".to_owned(),
        }
    }
    // Owned types carry heap resources: they move, and every live slot is
    // released on scope exit and control-flow jumps. Enums own their payload
    // slots, which may hold strings.
    fn owned(self) -> bool {
        matches!(self, Type::String | Type::Vec | Type::StrVec | Type::Struct(_) | Type::Enum(..))
    }
}
enum MoveKind { Var(usize), Field, StrVecElement }
// Built-in names are reserved so their semantics stay unambiguous.
const RESERVED: [&str; 15] = ["len", "push", "pop", "vec", "read_file", "write_file", "bytes", "contains", "slice", "split", "args", "join", "slice_view", "lines", "count_lines_with"];

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
                        if !matches!(escaped, Some(b'n' | b't' | b'\\' | b'"')) {
                            return Err(lex_error(source, i, "unsupported string escape; allowed: \\n \\t \\\\ \\\""));
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
            Kind::Number(source[start..i].parse().map_err(|_| lex_error(source, start, "integer literal exceeds i64 maximum"))?)
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
                '!' => Kind::Bang, '.' => Kind::Dot,
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
enum Expr { Number(i64), BoolLit(bool), StrLit(String, usize, usize), Variable(usize, Type, usize, usize), Call(usize, Vec<Argument>, Type), Index(usize, usize, Box<Expr>, usize, usize), VecLit(Vec<Expr>, usize, usize), VecIndex(usize, Box<Expr>, usize, usize, bool), Len(Box<Expr>, usize, usize), ReadFile(Box<Expr>, usize, usize), WriteFile(Box<Expr>, Box<Expr>, usize, usize), Bytes(Box<Expr>, usize, usize), Contains(Box<Expr>, Box<Expr>, usize, usize), Slice(Box<Expr>, Box<Expr>, Box<Expr>, usize, usize), Split(Box<Expr>, Box<Expr>, usize, usize), Args(usize, usize), Join(usize, Box<Expr>, usize, usize), StrVecIndex(usize, Box<Expr>, usize, usize), SliceView(usize, Box<Expr>, Box<Expr>, usize, usize), Lines(Box<Expr>, usize, usize), CountLinesWith(Box<Expr>, Box<Expr>, usize, usize), StructLit(usize, Vec<Expr>), EnumLit(&'static str, usize, usize, Vec<Expr>), Field(Box<Expr>, String, Type), StrEq(Box<Expr>, Box<Expr>, bool), Neg(Box<Expr>, usize, usize), Not(Box<Expr>), Binary(Box<Expr>, Kind, Box<Expr>, usize, usize, Type) }
#[derive(Debug)]
enum Argument { Value(Expr), Borrow(usize, bool) } // bool: borrow of a vec struct (pass &slot) vs array (decays)
#[derive(Debug)]
enum Printed { Expr(Expr), Text(String) }
#[derive(Debug)]
enum Statement { Return(Expr, Vec<(usize, Type)>), Declare(usize, Expr), Array(usize, Vec<Expr>), Assign(usize, Expr, bool), Store(usize, usize, Expr, Expr, usize, usize), VecPush(usize, Expr, usize, usize), VecPop(usize, usize, usize), VecStore(usize, Expr, Expr, usize, usize), StrVecPush(usize, Expr, usize, usize), StrVecPop(usize, usize, usize), StrVecStore(usize, Expr, Expr, usize, usize), FieldStore(usize, String, Type, Expr), Print(Printed), Assert(Expr, usize, usize), If(Expr, Vec<Statement>, Vec<Statement>), While(Expr, Vec<Statement>), For(usize, usize, Vec<Statement>, usize, usize, usize), Match(usize, Type, Vec<MatchArm>), Break, Continue(Option<usize>), Frees(Vec<(usize, Type)>) }
#[derive(Debug)]
struct Function { id: usize, params: Vec<(usize, Type)>, body: Vec<Statement>, is_main: bool, returns: Type }
fn returns(stmts: &[Statement]) -> bool {
    stmts.iter().any(|s| match s {
        Statement::Return(..) => true,
        Statement::If(_, yes, no) => returns(yes) && returns(no),
        // Exhaustive coverage means the match always runs one arm, so the
        // match returns whenever every arm does.
        Statement::Match(_, _, arms) => !arms.is_empty() && arms.iter().all(|arm| returns(&arm.body)),
        _ => false,
    })
}
#[derive(Debug, Clone)]
struct StructDef { name: String, fields: Vec<(String, Type)> }
// Enum layouts are closed: variants carry a fixed field list, and the first
// version restricts payload fields to i64/bool/string so no enum can contain
// itself and every payload has a trivial release.
#[derive(Debug, Clone)]
struct EnumDef { name: &'static str, variants: Vec<(String, Vec<Type>)> }
#[derive(Debug)]
struct MatchArm { variant: usize, bindings: Vec<(usize, Type, usize)>, body: Vec<Statement> }

type SharedFunctions = HashMap<String, (usize, Vec<Type>, Type)>;

struct Parser<'a> { file: String, source: &'a str, tokens: Vec<Token>, pos: usize, names: HashMap<String, (usize, bool, Type)>, next_var: usize, expression_depth: usize, expression_nodes: usize, block_depth: usize, loop_depth: usize, loop_scopes: Vec<usize>, loop_labels: Vec<Option<usize>>, next_label: usize, functions: &'a mut SharedFunctions, structs: &'a mut Vec<StructDef>, struct_names: &'a mut HashMap<String, usize>, enums: &'a mut Vec<EnumDef>, enum_names: &'a mut HashMap<String, usize>, id_order: Vec<usize>, in_main: bool, return_type: Type, moved: std::collections::HashSet<usize>, borrows: Vec<(usize, usize)>, scopes: Vec<Vec<(usize, Type)>> }
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
                if ty != Type::Int { return Err(self.fail("unary '-' requires an integer operand")); }
                (Expr::Neg(Box::new(operand), line, column), Type::Int)
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
                    self.pos += 1;
                    let vname = self.identifier()?;
                    let def = self.enums[eid].clone();
                    let vidx = def.variants.iter().position(|(n, _)| *n == vname)
                        .ok_or_else(|| self.fail(&format!("enum '{}' has no variant '{}'", def.name, vname)))?;
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
                    (Expr::EnumLit(def.name, eid, vidx, values), Type::Enum(def.name, eid))
                } else if let Some(&sid) = self.struct_names.get(&name) {
                    if *self.current() == Kind::LBrace {
                        // Construction: Name { field: expr, ... } - fields may
                        // appear in any order, each exactly once.
                        self.pos += 1;
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
                                None => return Err(error(&self.file, self.source, name_offset, &format!("missing field '{}' in '{}' construction", def_fields[i].0, name))),
                            }
                        }
                        // Early return: no postfix on a fresh literal; wrap in
                        // parentheses for field access.
                        return Ok((Expr::StructLit(sid, values), Type::Struct(sid)));
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
                                    Type::Vec | Type::StrVec | Type::VecRef => {
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
                            if !matches!(ty, Type::String | Type::StrView | Type::Vec | Type::StrVec | Type::VecRef) { return Err(self.fail("len expects a string, vec, or array variable")); }
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
                } else if matches!(name.as_str(), "read_file" | "write_file" | "bytes" | "contains" | "slice" | "split") && *self.current() == Kind::LParen {
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
                        if second_ty != Type::String { return Err(self.fail("split expects a string separator")); }
                        self.take(Kind::RParen)?;
                        Expr::Split(Box::new(first), Box::new(second), line, col)
                    } else if name == "contains" {
                        self.take(Kind::Comma)?;
                        let (second, second_ty) = self.expression(0, false)?;
                        if !matches!(second_ty, Type::String | Type::StrView) { return Err(self.fail("contains expects a string or string view pattern")); }
                        self.take(Kind::RParen)?;
                        Expr::Contains(Box::new(first), Box::new(second), line, col)
                    } else {
                        self.take(Kind::Comma)?;
                        let (second, second_ty) = self.expression(0, false)?;
                        if !matches!(second_ty, Type::String | Type::StrView) { return Err(self.fail("write_file expects a string or string view content")); }
                        self.take(Kind::RParen)?;
                        Expr::WriteFile(Box::new(first), Box::new(second), line, col)
                    };
                    let ty = match name.as_str() { "write_file" => Type::Int, "bytes" => Type::Vec, "contains" => Type::Bool, "split" => Type::StrVec, _ => Type::String };
                    (builtin, ty)
                } else if *self.current() == Kind::LParen {
                    let (id, types, ret) = self.functions.get(&name).cloned().ok_or_else(|| error(&self.file, self.source, name_offset, &format!("unknown function '{name}'")))?;
                    self.pos += 1;
                    let mut args = Vec::new();
                    if *self.current() != Kind::RParen {
                        loop {
                            if args.len() >= 64 { return Err(self.fail("function argument limit exceeded (64)")); }
                            let expected = *types.get(args.len()).ok_or_else(|| self.fail(&format!("too many arguments to '{name}'")))?;
                            if expected == Type::VecRef {
                                self.take(Kind::Amp)?;
                                let arg = self.identifier()?;
                                let (var, _, actual) = *self.names.get(&arg).ok_or_else(|| self.fail(&format!("unknown variable '{arg}'")))?;
                                if actual != Type::Vec { return Err(self.fail(&format!("argument must be a vec, found {}", actual.name()))); }
                                if self.moved.contains(&var) { return Err(self.fail(&format!("cannot borrow '{arg}' because it was moved"))); }
                                args.push(Argument::Borrow(var, true));
                            } else if let Type::Array(len) = expected {
                                self.take(Kind::Amp)?;
                                let arg = self.identifier()?;
                                let (var, _, actual) = *self.names.get(&arg).ok_or_else(|| self.fail(&format!("unknown array '{arg}'")))?;
                                if actual != Type::Array(len) { return Err(self.fail(&format!("argument requires array of length {len}"))); }
                                args.push(Argument::Borrow(var, false));
                            } else {
                                // By-value arguments consume string values.
                                let (value, ty) = self.expression(0, moving)?;
                                if ty != expected { return Err(self.fail(&format!("argument {} to '{name}' must be {}", args.len() + 1, expected.name()))); }
                                args.push(Argument::Value(value));
                            }
                            if *self.current() != Kind::Comma { break; }
                            self.pos += 1;
                        }
                    }
                    self.take(Kind::RParen)?;
                    if args.len() != types.len() { return Err(self.fail(&format!("function '{name}' expects {} arguments, got {}", types.len(), args.len()))); }
                    (Expr::Call(id, args, ret), ret)
                } else {
                    let (id, _, ty) = *self.names.get(&name).ok_or_else(|| error(&self.file, self.source, name_offset, &format!("unknown variable '{name}'")))?;
                    if self.moved.contains(&id) { return Err(error(&self.file, self.source, name_offset, &format!("cannot use '{name}' because it was moved"))); }
                    if let Type::Array(len) = ty {
                        let (line, col) = self.position(); self.take(Kind::LBracket)?;
                        let (index, index_ty) = self.expression(0, false)?;
                        if index_ty != Type::Int { return Err(self.fail("array index must be an integer")); }
                        self.take(Kind::RBracket)?;
                        (Expr::Index(id, len, Box::new(index), line, col), Type::Int)
                    } else if ty == Type::StrVec && *self.current() == Kind::LBracket {
                        if self.borrows.iter().any(|&(bid, _)| bid == id) { return Err(self.fail(&format!("cannot modify '{name}' while it is borrowed"))); }
                        let (line, col) = self.position(); self.take(Kind::LBracket)?;
                        let (index, index_ty) = self.expression(0, false)?;
                        if index_ty != Type::Int { return Err(self.fail("vector index must be an integer")); }
                        self.take(Kind::RBracket)?;
                        (Expr::StrVecIndex(id, Box::new(index), line, col), Type::StrView)
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
                (Expr::StrVecIndex(_, _, _, _), _) => Some(MoveKind::StrVecElement),
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
                        (Type::String | Type::StrView, Type::String | Type::StrView) => op == Kind::EqEq || op == Kind::NotEq,
                        _ => false,
                    };
                    if !comparable {
                        return Err(self.fail("comparison requires two integers, or two strings/views with == or !="));
                    }
                    // String/view equality gets a dedicated node so emission
                    // never confuses it with C pointer comparison.
                    if (op == Kind::EqEq || op == Kind::NotEq) && matches!(lhs_ty, Type::String | Type::StrView) {
                        lhs = (Expr::StrEq(Box::new(lhs_expr), Box::new(rhs), op == Kind::NotEq), Type::Bool);
                        continue;
                    }
                    Type::Bool
                }
                Kind::Plus if lhs_ty == Type::String || rhs_ty == Type::String => {
                    if lhs_ty != Type::String || rhs_ty != Type::String { return Err(self.fail("'+' requires two strings or two integers")); }
                    // The left operand was parsed in the caller's mode; using it
                    // here consumes it, so retroactively record the move.
                    match &lhs_expr {
                        Expr::Variable(id, ty, _, _) => { if ty.owned() && !self.moved.contains(id) { self.moved.insert(*id); } }
                        Expr::Field(_, _, fty) if fty.owned() => return Err(self.fail("cannot move a field out of a struct")),
                        Expr::StrVecIndex(_, _, _, _) => return Err(self.fail("cannot move an element out of a vector")),
                        _ => {}
                    }
                    Type::String
                }
                _ => {
                    if lhs_ty != Type::Int || rhs_ty != Type::Int { return Err(self.fail("arithmetic requires integer operands")); }
                    Type::Int
                }
            };
            lhs = (Expr::Binary(Box::new(lhs_expr), op, Box::new(rhs), line, column, ty), ty);
        }
        Ok(lhs)
    }
    fn named_type(&mut self) -> Result<Type, String> {
        let name = self.identifier()?;
        match name.as_str() {
            "i64" => Ok(Type::Int),
            "bool" => Ok(Type::Bool),
            "string" => Ok(Type::String),
            "vec" => Ok(Type::Vec),
            other => match self.struct_names.get(other) {
                Some(&id) => Ok(Type::Struct(id)),
                None => match self.enum_names.get(other) {
                    Some(&id) => Ok(Type::Enum(self.enums[id].name, id)),
                    None => Err(self.fail(&format!("unknown type '{}'", other))),
                },
            },
        }
    }
    fn parse_enum_def(&mut self) -> Result<(), String> {
        self.pos += 1;
        let name = self.identifier()?;
        if RESERVED.contains(&name.as_str()) { return Err(self.fail(&format!("'{}' is a built-in name and cannot be an enum", name))); }
        if self.struct_names.contains_key(&name) { return Err(self.fail(&format!("'{}' is already a struct", name))); }
        if self.enum_names.contains_key(&name) { return Err(self.fail(&format!("duplicate enum '{}'", name))); }
        self.take(Kind::LBrace)?;
        let mut variants: Vec<(String, Vec<Type>)> = Vec::new();
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
                        let fty = self.named_type()?;
                        // Payloads stay flat in the first version: a variant
                        // may not carry a vec, struct, or another enum.
                        if !matches!(fty, Type::Int | Type::Bool | Type::String) {
                            return Err(self.fail("enum variant payloads must be i64, bool, or string"));
                        }
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
        let id = self.enums.len();
        // Type names are interned once per declaration so Type stays Copy and
        // diagnostics can print the declared name.
        let interned: &'static str = Box::leak(name.clone().into_boxed_str());
        self.enum_names.insert(name, id);
        self.enums.push(EnumDef { name: interned, variants });
        Ok(())
    }
    fn parse_struct_def(&mut self) -> Result<(), String> {
        self.pos += 1;
        let name = self.identifier()?;
        if RESERVED.contains(&name.as_str()) { return Err(self.fail(&format!("'{}' is a built-in name and cannot be a struct", name))); }
        if self.struct_names.contains_key(&name) { return Err(self.fail(&format!("duplicate struct '{}'", name))); }
        self.take(Kind::LBrace)?;
        let mut fields: Vec<(String, Type)> = Vec::new();
        let mut seen = std::collections::HashSet::new();
        while *self.current() != Kind::RBrace {
            let fname = self.identifier()?;
            if !seen.insert(fname.clone()) { return Err(self.fail(&format!("duplicate field '{}' in struct '{}'", fname, name))); }
            self.take(Kind::Colon)?;
            let fty = self.named_type()?;
            fields.push((fname, fty));
            if *self.current() == Kind::Comma { self.pos += 1; }
        }
        self.take(Kind::RBrace)?;
        let id = self.structs.len();
        self.struct_names.insert(name.clone(), id);
        self.structs.push(StructDef { name, fields });
        Ok(())
    }
    // Arrays are borrowed read-only; strings and scalars pass by value.
    fn parameter_type(&mut self) -> Result<Type, String> {
        if *self.current() == Kind::Amp {
            self.pos += 1;
            if matches!(self.current(), Kind::Ident(name) if name == "vec") {
                self.pos += 1;
                return Ok(Type::VecRef);
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
    // Second pass skips struct bodies (already collected, must not re-register).
    fn skip_struct_def(&mut self) {
        self.pos += 2; // 'struct' Name
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
    fn skip_enum_def(&mut self) {
        self.pos += 2; // 'enum' Name
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
            if *self.current() == Kind::Struct { self.parse_struct_def()?; continue; }
            if *self.current() == Kind::Enum { self.parse_enum_def()?; continue; }
            self.take(Kind::Fn)?;
            let name = self.identifier()?;
            if !seen.insert(name.clone()) { return Err(self.fail(&format!("duplicate function '{name}'"))); }
            self.take(Kind::LParen)?;
            let mut types = Vec::new();
            if *self.current() != Kind::RParen {
                loop {
                    if types.len() >= 64 { return Err(self.fail("function parameter limit exceeded (64)")); }
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
            // Ids are unique across all files of a program.
            let id = *next_id;
            *next_id += 1;
            self.id_order.push(id);
            if name == "main" {
                if !types.is_empty() { return Err(self.fail("main cannot have parameters")); }
            } else {
                self.take(Kind::Arrow)?;
                let ret = self.return_annotation()?;
                if self.functions.insert(name.clone(), (id, types, ret)).is_some() {
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
            if *self.current() == Kind::Struct { self.skip_struct_def(); continue; }
            if *self.current() == Kind::Enum { self.skip_enum_def(); continue; }
            self.take(Kind::Fn)?;
            let name = self.identifier()?;
            let is_main = name == "main";
            if is_main && main_found { return Err(self.fail("duplicate main function")); }
            self.take(Kind::LParen)?;
            // Variable ids restart per function, so move tracking must too.
            self.names.clear(); self.next_var = 0; self.moved.clear();
            let mut params = Vec::new();
            if *self.current() != Kind::RParen {
                loop {
                    if params.len() >= 64 { return Err(self.fail("function parameter limit exceeded (64)")); }
                    let param = self.identifier()?;
                    if RESERVED.contains(&param.as_str()) { return Err(self.fail(&format!("'{param}' is a built-in name and cannot be a parameter"))); }
                    if self.names.contains_key(&param) { return Err(self.fail(&format!("duplicate parameter '{param}'"))); }
                    self.take(Kind::Colon)?;
                    let ty = self.parameter_type()?;
                    let id = self.next_var; self.next_var += 1;
                    self.names.insert(param, (id, false, ty)); params.push((id, ty));
                    if *self.current() != Kind::Comma { break; }
                    self.pos += 1;
                }
            }
            self.take(Kind::RParen)?;
            let id = self.id_order[result.len()];
            let returns_type;
            if is_main {
                if !params.is_empty() { return Err(self.fail("main cannot have parameters")); }
                main_found = true;
                returns_type = Type::Int;
            } else {
                self.take(Kind::Arrow)?;
                returns_type = self.return_annotation()?;
            }
            self.in_main = is_main;
            self.return_type = returns_type;
            // Parameter scope: lives for the whole body so return paths can
            // release owned parameters.
            self.scopes.push(params.iter().map(|&(id, ty)| (id, ty)).collect());
            let body = self.block();
            let params_scope = self.scopes.pop().unwrap();
            let mut body = body?;
            let param_owned: Vec<(usize, Type)> = params_scope.iter().filter(|(_, ty)| ty.owned()).copied().collect();
            if !param_owned.is_empty() { body.push(Statement::Frees(param_owned)); }
            if !is_main && !returns(&body) { return Err(self.fail("function may exit without returning a value")); }
            result.push(Function { id, params, body, is_main, returns: returns_type });
        }
        // main presence is checked once per program, not per file.
        Ok(result)
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
            Expr::StrVecIndex(id, idx, _, _) => {
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
                        Argument::Borrow(_, _) => {}
                    }
                }
            }
            Expr::Contains(a, b, _, _) | Expr::Split(a, b, _, _) | Expr::StrEq(a, b, _) => { self.record_borrow_bases(a); self.record_borrow_bases(b); }
            Expr::WriteFile(a, b, _, _) | Expr::Slice(_, a, b, _, _) => { self.record_borrow_bases(a); self.record_borrow_bases(b); }
            Expr::Join(_, sep, _, _) => self.record_borrow_bases(sep),
            Expr::Field(base, _, _) => self.record_borrow_bases(base),
            _ => {}
        }
    }
    fn live_owned(&self) -> Vec<(usize, Type)> {
        self.scopes.iter().flatten().filter(|(_, ty)| ty.owned()).map(|&(id, ty)| (id, ty)).collect()
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
                        let (value, ty) = self.expression(0, true)?; self.take(Kind::Semi)?;
                        (Statement::Declare(id, value), ty)
                    };
                    self.names.insert(name.clone(), (id, mutable, ty));
                    self.scopes.last_mut().unwrap().push((id, ty));
                    declared.push(name);
                    if let Statement::Declare(_, init) = &stmt { self.record_borrow_bases(init); }
                    result.push(stmt);
                }
                Kind::Ident(name) => {
                    // Built-in mutation statements come before assignment so
                    // they stay unambiguous: push(v, x); pop(v);
                    if (name == "push" || name == "pop") && self.tokens.get(self.pos + 1).map(|t| t.kind.clone()) == Some(Kind::LParen) {
                        let (line, col) = self.position();
                        self.pos += 2;
                        let target = self.identifier()?;
                        let (id, mutable, ty) = *self.names.get(&target).ok_or_else(|| self.fail(&format!("unknown variable '{target}'")))?;
                        if self.moved.contains(&id) { return Err(self.fail(&format!("cannot use '{target}' because it was moved"))); }
                        if !matches!(ty, Type::Vec | Type::StrVec) { return Err(self.fail(&format!("{name} expects a vec, found {}", ty.name()))); }
                        if !mutable { return Err(self.fail(&format!("cannot modify immutable variable '{target}'"))); }
                        if self.borrows.iter().any(|&(bid, _)| bid == id) { return Err(self.fail(&format!("cannot modify '{target}' while it is borrowed"))); }
                        if name == "push" {
                            self.take(Kind::Comma)?;
                            let (value, value_ty) = self.expression(0, ty == Type::StrVec)?;
                            let ok = match ty { Type::Vec => value_ty == Type::Int, _ => value_ty == Type::String };
                            if !ok { return Err(self.fail(&format!("vector elements must be {}", if ty == Type::Vec { "i64" } else { "string" }))); }
                            self.take(Kind::RParen)?; self.take(Kind::Semi)?;
                            if ty == Type::StrVec {
                                result.push(Statement::StrVecPush(id, value, line, col));
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
                    let (id, mutable, ty) = *self.names.get(&name).ok_or_else(|| self.fail(&format!("unknown variable '{name}'")))?;
                    if !mutable { return Err(self.fail(&format!("cannot assign to immutable variable '{name}'"))); }
                    self.pos += 1;
                    if let Type::Array(len) = ty {
                        let (line, col) = self.position(); self.take(Kind::LBracket)?;
                        let (index, index_ty) = self.expression(0, false)?;
                        if index_ty != Type::Int { return Err(self.fail("array index must be an integer")); }
                        self.take(Kind::RBracket)?; self.take(Kind::Equal)?;
                        let (value, value_ty) = self.expression(0, false)?; self.take(Kind::Semi)?;
                        if value_ty != Type::Int { return Err(self.fail("array elements must be i64")); }
                        result.push(Statement::Store(id, len, index, value, line, col));
                    } else if ty == Type::StrVec && *self.current() == Kind::LBracket {
                        let (line, col) = self.position(); self.take(Kind::LBracket)?;
                        let (index, index_ty) = self.expression(0, false)?;
                        if index_ty != Type::Int { return Err(self.fail("vector index must be an integer")); }
                        self.take(Kind::RBracket)?; self.take(Kind::Equal)?;
                        let (value, value_ty) = self.expression(0, true)?; self.take(Kind::Semi)?;
                        if value_ty != Type::String { return Err(self.fail("vector elements must be string")); }
                        result.push(Statement::StrVecStore(id, index, value, line, col));
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
                        let release_old = ty == Type::String;
                        if release_old {
                            // The right-hand side may not consume the target
                            // itself; assigning to a previously moved variable
                            // is a legal re-initialization.
                            if self.moved.contains(&id) && !moved_before {
                                return Err(self.fail(&format!("cannot assign to '{name}' because it was moved in the same statement")));
                            }
                            self.moved.remove(&id);
                        }
                        result.push(Statement::Assign(id, value, release_old));
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
                    self.pos += 1;
                    let (condition, ty) = self.expression(0, false)?;
                    if ty != Type::Bool { return Err(self.fail("condition must be a boolean expression")); }
                    if self.loop_depth >= 64 { return Err(self.fail("loop nesting limit exceeded (64)")); }
                    self.loop_depth += 1;
                    self.loop_scopes.push(self.scopes.len());
                    self.loop_labels.push(None);
                    let body = self.block();
                    self.loop_labels.pop();
                    self.loop_scopes.pop();
                    self.loop_depth -= 1;
                    // Moves inside the body poison uses after the loop; uses
                    // before the move inside the body are caught at runtime by
                    // the null check on every string read.
                    result.push(Statement::While(condition, body?));
                }
                Kind::For => {
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
                        let (v_id, elem_ty) = match (&first, first_ty) {
                            (Expr::Variable(id, Type::Vec, _, _), Type::Vec) => (*id, Type::Int),
                            (Expr::Variable(id, Type::StrVec, _, _), Type::StrVec) => (*id, Type::StrView),
                            _ => return Err(self.fail("for-in needs a bare vec variable or an int range (start..end)")),
                        };
                        if self.loop_depth >= 64 { return Err(self.fail("loop nesting limit exceeded (64)")); }
                        let (line, col) = self.position();
                        let end_var = self.next_var; self.next_var += 1;
                        let counter = self.next_var; self.next_var += 1;
                        let var = self.next_var; self.next_var += 1;
                        let label = self.next_label; self.next_label += 1;
                        self.loop_depth += 1;
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
                        // Per-iteration element binding, then the user's body.
                        let element = match elem_ty {
                            Type::StrView => Expr::StrVecIndex(v_id, Box::new(Expr::Variable(counter, Type::Int, line, col)), line, col),
                            _ => Expr::VecIndex(v_id, Box::new(Expr::Variable(counter, Type::Int, line, col)), line, col, false),
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
                        if !matches!(ty, Type::Int | Type::Bool | Type::String | Type::StrView) {
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
            if pattern != def.name {
                return Err(self.fail(&format!("pattern must name the matched enum ('{}::Variant')", def.name)));
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
                    // String payloads bind as read-only views: nothing can be
                    // moved out of the matched enum.
                    let bty = if field == Type::String { Type::StrView } else { field };
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

fn emit_expr(expr: &Expr, out: &mut String, next: &mut usize, moving: bool) -> Emitted {
    let emitted = match expr {
        Expr::Number(n) => Emitted::value(format!("INT64_C({n})"), Type::Int),
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
            } else if ty.owned() {
                // Runtime backstop: a moved value's slot is empty.
                match ty {
                    // Structs and enums have no runtime sentinel: the compile-time
                    // move set is their only guard (documented limitation).
                    Type::Struct(_) | Type::Enum(_, _) => {}
                    _ => {
                        let probe = match ty { Type::Vec | Type::StrVec => format!("!v{id}.data"), _ => format!("!v{id}.ptr") };
                        let message = match ty { Type::Vec | Type::StrVec => "use of moved vector", _ => "use of moved string" };
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
            let arguments = args.iter().map(|arg| match arg {
                Argument::Value(e) => { let t = emit_expr(e, out, next, true); moves.extend(t.moves); t.text }
                Argument::Borrow(id, is_vec) => if *is_vec { format!("&v{id}") } else { format!("v{id}") },
            }).collect::<Vec<_>>().join(", ");
            let t = *next; *next += 1;
            out.push_str(&format!("    {}t{t} = ae_fn_{id}({arguments});\n", ret.c_temp()));
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
                _ => format!("ae_vec_len({}, {line}, {col})", t.text),
            };
            out.push_str(&format!("    int64_t t{id} = {call};\n"));
            if t.owned { drop_owned_temp(out, &t); }
            Emitted { text: format!("t{id}"), ty: Type::Int, owned: false, moves: Vec::new(), materialized: true }
        }
        Expr::Neg(a, line, column) => {
            let t = emit_expr(a, out, next, false);
            Emitted { text: format!("ae_neg({}, {line}, {column})", t.text), ty: Type::Int, owned: false, moves: t.moves, materialized: false }
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
            Emitted { text: format!("t{id}"), ty: Type::String, owned: true, moves: Vec::new(), materialized: true }
        }
        Expr::WriteFile(path, content, line, col) => {
            let p = emit_expr(path, out, next, false);
            let c = emit_expr(content, out, next, false);
            let id = *next; *next += 1;
            out.push_str(&format!("    int64_t t{id} = ae_write_file({}, AE_VIEW({}), {line}, {col});
", p.text, c.text));
            Emitted { text: format!("t{id}"), ty: Type::Int, owned: false, moves: Vec::new(), materialized: true }
        }
        Expr::Bytes(e, line, col) => {
            let t = emit_expr(e, out, next, false);
            let id = *next; *next += 1;
            out.push_str(&format!("    ae_vec t{id} = ae_bytes(AE_VIEW({}), {line}, {col});
", t.text));
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
        Expr::Split(s, sep, line, col) => {
            let h = emit_expr(s, out, next, false);
            let n = emit_expr(sep, out, next, false);
            let id = *next; *next += 1;
            out.push_str(&format!("    ae_strvec t{id} = ae_split({}, {}, {line}, {col});\n", h.text, n.text));
            if h.owned { drop_owned_temp(out, &h); }
            if n.owned { drop_owned_temp(out, &n); }
            Emitted { text: format!("t{id}"), ty: Type::StrVec, owned: true, moves: Vec::new(), materialized: true }
        }
        Expr::Args(line, col) => {
            let id = *next; *next += 1;
            out.push_str(&format!("    ae_strvec t{id} = ae_args({line}, {col});\n"));
            Emitted { text: format!("t{id}"), ty: Type::StrVec, owned: true, moves: Vec::new(), materialized: true }
        }
        Expr::StrVecIndex(id, index, line, col) => {
            out.push_str(&format!("    if (!v{id}.data) ae_fail(\"use of moved vector\", {line}, {col});\n"));
            let i = emit_expr(index, out, next, false);
            // Elements are borrow-only: the pointer view never owns.
            Emitted { text: format!("(*ae_strvec_get(&v{id}, {}, {line}, {col}))", i.text), ty: Type::String, owned: false, moves: i.moves, materialized: true }
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
            out.push_str(&format!("    ae_string t{id} = ae_join(&v{vid}, {}, {line}, {col});\n", n.text));
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
            let id = *next; *next += 1;
            out.push_str(&format!("    ae_string t{id} = ae_str_cat({}, {}, {line}, {column});\n", l.text, r.text));
            let mut moves = l.moves; moves.extend(r.moves);
            null_moves(out, &moves);
            Emitted { text: format!("t{id}"), ty: Type::String, owned: true, moves: Vec::new(), materialized: true }
        }
        Expr::Binary(a, op, b, line, column, _) => {
            let l = emit_expr(a, out, next, false); let r = emit_expr(b, out, next, false);
            let text = match op {
                Kind::EqEq => format!("{} == {}", l.text, r.text),
                Kind::NotEq => format!("{} != {}", l.text, r.text),
                Kind::Less => format!("{} < {}", l.text, r.text),
                Kind::LessEq => format!("{} <= {}", l.text, r.text),
                Kind::Greater => format!("{} > {}", l.text, r.text),
                Kind::GreaterEq => format!("{} >= {}", l.text, r.text),
                Kind::Plus => format!("ae_add({}, {}, {line}, {column})", l.text, r.text),
                Kind::Minus => format!("ae_sub({}, {}, {line}, {column})", l.text, r.text),
                Kind::Star => format!("ae_mul({}, {}, {line}, {column})", l.text, r.text),
                Kind::Slash => format!("ae_div({}, {}, {line}, {column})", l.text, r.text),
                Kind::Percent => format!("ae_mod({}, {}, {line}, {column})", l.text, r.text),
                _ => unreachable!(),
            };
            let ty = if matches!(op, Kind::EqEq | Kind::NotEq | Kind::Less | Kind::LessEq | Kind::Greater | Kind::GreaterEq) { Type::Bool } else { Type::Int };
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
        Type::Struct(k) => out.push_str(&format!("    ae_t{k}_release(&v{id});\n")),
        Type::Enum(_, k) => out.push_str(&format!("    ae_e{k}_release(&v{id});\n")),
        _ => out.push_str(&format!("    ae_str_release(&v{id});\n")),
    }
}
// An owned temporary (not a named slot) that a statement has finished using.
fn drop_owned_temp(out: &mut String, emitted: &Emitted) {
    match emitted.ty {
        Type::Vec => out.push_str(&format!("    ae_vec_release(&{});\n", emitted.text)),
        Type::String => out.push_str(&format!("    ae_str_release(&{});\n", emitted.text)),
        Type::StrVec => out.push_str(&format!("    ae_strvec_release(&{});\n", emitted.text)),
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
            Statement::StrVecPush(id, value, line, col) => {
                out.push_str(&format!("    if (!v{id}.data) ae_fail(\"use of moved vector\", {line}, {col});\n"));
                let value = emit_expr(value, out, next, true);
                out.push_str(&format!("    ae_strvec_push(&v{id}, {}, {line}, {col});\n", value.text));
                null_moves(out, &value.moves);
            }
            Statement::StrVecPop(id, line, col) => {
                out.push_str(&format!("    if (!v{id}.data) ae_fail(\"use of moved vector\", {line}, {col});\n"));
                out.push_str(&format!("    ae_strvec_pop(&v{id}, {line}, {col});\n"));
            }
            Statement::StrVecStore(id, index, value, line, col) => {
                out.push_str(&format!("    if (!v{id}.data) ae_fail(\"use of moved vector\", {line}, {col});\n"));
                let i = emit_expr(index, out, next, false);
                let checked = *next; *next += 1;
                out.push_str(&format!("    int64_t t{checked} = ae_strvec_index({}, v{id}.len, {line}, {col});\n", i.text));
                let value = emit_expr(value, out, next, true);
                out.push_str(&format!("    ae_str_release(&v{id}.data[t{checked}]);\n"));
                out.push_str(&format!("    v{id}.data[t{checked}] = {};\n", value.text));
                null_moves(out, &value.moves);
            }
            Statement::FieldStore(id, fname, fty, value) => {
                // Overwriting an owned field releases the old value first.
                let release_old = match fty {
                    Type::String => format!("    ae_str_release(&v{id}.{});\n", fname),
                    Type::Vec => format!("    ae_vec_release(&v{id}.{});\n", fname),
                    Type::StrVec => format!("    ae_strvec_release(&v{id}.{});\n", fname),
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

fn make_parser<'a>(file: &str, source: &'a str, tokens: Vec<Token>, functions: &'a mut SharedFunctions, structs: &'a mut Vec<StructDef>, struct_names: &'a mut HashMap<String, usize>, enums: &'a mut Vec<EnumDef>, enum_names: &'a mut HashMap<String, usize>) -> Parser<'a> {
    Parser { file: file.to_owned(), source, tokens, pos: 0, names: HashMap::new(), next_var: 0, expression_depth: 0, expression_nodes: 0, block_depth: 0, loop_depth: 0, loop_scopes: Vec::new(), loop_labels: Vec::new(), next_label: 0, functions, structs, struct_names, enums, enum_names, id_order: Vec::new(), in_main: false, return_type: Type::Int, moved: std::collections::HashSet::new(), borrows: Vec::new(), scopes: Vec::new() }
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
fn parse_source(file: &str, source: &str, shared: &mut SharedFunctions, structs: &mut Vec<StructDef>, struct_names: &mut HashMap<String, usize>, enums: &mut Vec<EnumDef>, enum_names: &mut HashMap<String, usize>, next_id: &mut usize) -> Result<Vec<Function>, String> {
    let tokens = lex(source)?;
    let uses = scan_uses(file, &tokens)?;
    if !uses.is_empty() {
        return Err(error(file, source, 0, "'use' is only supported when compiling a source file"));
    }
    let mut p = make_parser(file, source, tokens, shared, structs, struct_names, enums, enum_names);
    p.collect_functions(next_id)?;
    let functions = p.program()?;
    if functions.iter().filter(|f| f.is_main).count() != 1 {
        return Err(error(file, source, 0, "missing main function"));
    }
    Ok(functions)
}

fn load_file(path: &Path, visited: &mut std::collections::HashSet<std::path::PathBuf>, shared: &mut SharedFunctions, structs: &mut Vec<StructDef>, struct_names: &mut HashMap<String, usize>, enums: &mut Vec<EnumDef>, enum_names: &mut HashMap<String, usize>, next_id: &mut usize, out: &mut Vec<Vec<Function>>, mains: &mut usize) -> Result<(), String> {
    let canonical = std::fs::canonicalize(path).map_err(|e| format!("cannot open {}: {e}", path.display()))?;
    if !visited.insert(canonical) { return Err(format!("circular import of {}", path.display())); }
    let source = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let file = path.display().to_string();
    let tokens = lex(&source).map_err(|e| format!("{file}:{e}"))?;
    let uses = scan_uses(&file, &tokens).map_err(|e| format!("{file}:{e}"))?;
    let parent = path.parent().unwrap_or(Path::new(".")).to_owned();
    for import in uses {
        load_file(&parent.join(&import), visited, shared, structs, struct_names, enums, enum_names, next_id, out, mains)?;
    }
    let mut p = make_parser(&file, &source, tokens, shared, structs, struct_names, enums, enum_names);
    p.skip_uses();
    p.collect_functions(next_id)?;
    let functions = p.program()?;
    *mains += functions.iter().filter(|f| f.is_main).count();
    out.push(functions);
    Ok(())
}

fn load_program(entry: &Path) -> Result<Program, String> {
    let mut visited = std::collections::HashSet::new();
    let mut shared: SharedFunctions = HashMap::new();
    let mut structs: Vec<StructDef> = Vec::new();
    let mut struct_names: HashMap<String, usize> = HashMap::new();
    let mut enums: Vec<EnumDef> = Vec::new();
    let mut enum_names: HashMap<String, usize> = HashMap::new();
    let mut next_id = 0usize;
    let mut files = Vec::new();
    let mut mains = 0usize;
    load_file(entry, &mut visited, &mut shared, &mut structs, &mut struct_names, &mut enums, &mut enum_names, &mut next_id, &mut files, &mut mains)?;
    if mains != 1 { return Err(format!("program must define exactly one main function (found {mains})")); }
    Ok(Program { files, structs, enums })
}

pub fn check_path(path: &Path) -> Result<(), String> {
    load_program(path).map(|_| ())
}
#[allow(dead_code)] // exercised by in-memory unit tests
pub fn check(source: &str) -> Result<(), String> {
    let mut shared: SharedFunctions = HashMap::new();
    let mut structs: Vec<StructDef> = Vec::new();
    let mut struct_names: HashMap<String, usize> = HashMap::new();
    let mut enums: Vec<EnumDef> = Vec::new();
    let mut enum_names: HashMap<String, usize> = HashMap::new();
    let mut next_id = 0usize;
    parse_source("", source, &mut shared, &mut structs, &mut struct_names, &mut enums, &mut enum_names, &mut next_id).map(|_| ())
}
fn c_type_field(ty: Type) -> String {
    match ty {
        Type::Int => "int64_t".to_owned(),
        Type::Bool => "bool".to_owned(),
        Type::String => "ae_string".to_owned(),
        Type::StrView => "ae_strview".to_owned(),
        Type::Vec => "ae_vec".to_owned(),
        Type::StrVec => "ae_strvec".to_owned(),
        Type::Struct(k) => format!("ae_t{k}"),
        Type::Enum(_, k) => format!("ae_e{k}"),
        Type::VecRef => "const ae_vec *".to_owned(),
        Type::Array(n) => format!("int64_t [{n}]"),
    }
}
fn c_type(ty: Type, id: usize, declaration: bool) -> String {
    match ty {
        Type::Int => format!("int64_t v{id}"),
        Type::Bool => format!("bool v{id}"),
        Type::String => format!("ae_string v{id}"),
        Type::StrView => format!("ae_strview v{id}"),
        Type::Vec => format!("ae_vec v{id}"),
        Type::StrVec => format!("ae_strvec v{id}"),
        Type::Struct(k) => format!("ae_t{k} v{id}"),
        Type::Enum(_, k) => format!("ae_e{k} v{id}"),
        Type::VecRef if declaration => format!("const ae_vec *v{id}"),
        Type::VecRef => "const ae_vec *".to_owned(),
        Type::Array(_) if declaration => format!("const int64_t *v{id}"),
        Type::Array(_) => "const int64_t *".to_owned(),
    }
}
// Return type of a generated C function: the same spelling the expression
// temporaries use, without the variable name.
fn c_return(ty: Type) -> String {
    match ty {
        Type::Bool => "bool".to_owned(),
        Type::String => "ae_string".to_owned(),
        Type::StrView => "ae_strview".to_owned(),
        Type::Vec => "ae_vec".to_owned(),
        Type::StrVec => "ae_strvec".to_owned(),
        Type::Struct(k) => format!("ae_t{k}"),
        Type::Enum(_, k) => format!("ae_e{k}"),
        _ => "int64_t".to_owned(),
    }
}
fn emit_program(program: Program) -> Result<String, String> {
    let Program { files, structs, enums } = program;
    let mut out = String::from(include_str!("runtime.c"));
    // Enum layouts: a tag plus a union of per-variant payload records. Each
    // variant gets its own struct so a variant can carry several fields at
    // once (a union of individual fields would alias them). Release frees the
    // live variant's owned payloads (idempotently); forfeit only clears them
    // because the value moved elsewhere.
    for (eid, def) in enums.iter().enumerate() {
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
    Ok(out)
}
#[allow(dead_code)] // exercised by in-memory unit tests
pub fn compile(source: &str) -> Result<String, String> {
    let mut shared: SharedFunctions = HashMap::new();
    let mut structs: Vec<StructDef> = Vec::new();
    let mut struct_names: HashMap<String, usize> = HashMap::new();
    let mut enums: Vec<EnumDef> = Vec::new();
    let mut enum_names: HashMap<String, usize> = HashMap::new();
    let mut next_id = 0usize;
    let functions = parse_source("", source, &mut shared, &mut structs, &mut struct_names, &mut enums, &mut enum_names, &mut next_id)?;
    emit_program(Program { files: vec![functions], structs, enums })
}
pub fn compile_path(path: &Path) -> Result<String, String> {
    emit_program(load_program(path)?)
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
            "fn main() { if true == false { print(1); } }",
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
