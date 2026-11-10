use std::collections::HashMap;
use std::path::Path;

#[derive(Debug, Clone, PartialEq)]
enum Kind {
    Ident(String), Number(i64), Text(String), Fn, Let, Var, Print, If, Else, While, For, In, Return, Break, Continue, Assert, True, False, Use,
    EqEq, NotEq, Less, LessEq, Greater, GreaterEq, And, Or, Bang,
    LParen, RParen, LBrace, RBrace, LBracket, RBracket, Comma, Colon, Arrow, Amp, Semi, Equal, Plus, Minus, Star, Slash, Percent, DotDot, Eof,
}
#[derive(Debug, Clone)]
struct Token { kind: Kind, offset: usize, line: usize, column: usize }

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Type { Int, Bool, String, Vec, VecRef, Array(usize) }
impl Type {
    fn name(self) -> &'static str {
        match self { Type::Int => "i64", Type::Bool => "bool", Type::String => "string", Type::Vec => "vec", Type::VecRef => "&vec", Type::Array(_) => "array" }
    }
    fn c_temp(self) -> &'static str {
        match self { Type::Bool => "bool ", Type::String => "ae_string ", Type::Vec => "ae_vec ", _ => "int64_t " }
    }
    // Owned types carry heap resources: they move, and every live slot is
    // released on scope exit and control-flow jumps.
    fn owned(self) -> bool {
        matches!(self, Type::String | Type::Vec)
    }
}
// Built-in names are reserved so their semantics stay unambiguous.
const RESERVED: [&str; 9] = ["len", "push", "pop", "vec", "read_file", "write_file", "bytes", "contains", "slice"];

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
                "true" => Kind::True, "false" => Kind::False, "use" => Kind::Use,
                s => Kind::Ident(s.to_owned()),
            }
        } else if c.is_ascii_digit() {
            i += 1;
            while i < bytes.len() && bytes[i].is_ascii_digit() { i += 1; }
            Kind::Number(source[start..i].parse().map_err(|_| lex_error(source, start, "integer literal exceeds i64 maximum"))?)
        } else if ["==", "!=", "<=", ">=", "->", "&&", "||", ".."].iter().any(|op| source[i..].starts_with(op)) {
            i += 2;
            match &source[start..i] { "==" => Kind::EqEq, "!=" => Kind::NotEq, "<=" => Kind::LessEq, ">=" => Kind::GreaterEq, "->" => Kind::Arrow, "&&" => Kind::And, "||" => Kind::Or, ".." => Kind::DotDot, _ => unreachable!() }
        } else {
            i += c.len_utf8();
            match c {
                '(' => Kind::LParen, ')' => Kind::RParen, '{' => Kind::LBrace, '}' => Kind::RBrace,
                '[' => Kind::LBracket, ']' => Kind::RBracket, ',' => Kind::Comma, ':' => Kind::Colon, '&' => Kind::Amp,
                ';' => Kind::Semi, '=' => Kind::Equal, '+' => Kind::Plus, '-' => Kind::Minus,
                '*' => Kind::Star, '/' => Kind::Slash, '%' => Kind::Percent, '<' => Kind::Less, '>' => Kind::Greater,
                '!' => Kind::Bang,
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
enum Expr { Number(i64), BoolLit(bool), StrLit(String, usize, usize), Variable(usize, Type, usize, usize), Call(usize, Vec<Argument>, Type), Index(usize, usize, Box<Expr>, usize, usize), VecLit(Vec<Expr>, usize, usize), VecIndex(usize, Box<Expr>, usize, usize, bool), Len(Box<Expr>, usize, usize), ReadFile(Box<Expr>, usize, usize), WriteFile(Box<Expr>, Box<Expr>, usize, usize), Bytes(Box<Expr>, usize, usize), Contains(Box<Expr>, Box<Expr>, usize, usize), Slice(Box<Expr>, Box<Expr>, Box<Expr>, usize, usize), StrEq(Box<Expr>, Box<Expr>, bool), Neg(Box<Expr>, usize, usize), Not(Box<Expr>), Binary(Box<Expr>, Kind, Box<Expr>, usize, usize, Type) }
#[derive(Debug)]
enum Argument { Value(Expr), Borrow(usize, bool) } // bool: borrow of a vec struct (pass &slot) vs array (decays)
#[derive(Debug)]
enum Printed { Expr(Expr), Text(String) }
#[derive(Debug)]
enum Statement { Return(Expr, Vec<(usize, Type)>), Declare(usize, Expr), Array(usize, Vec<Expr>), Assign(usize, Expr, bool), Store(usize, usize, Expr, Expr, usize, usize), VecPush(usize, Expr, usize, usize), VecPop(usize, usize, usize), VecStore(usize, Expr, Expr, usize, usize), Print(Printed), Assert(Expr, usize, usize), If(Expr, Vec<Statement>, Vec<Statement>), While(Expr, Vec<Statement>), For(usize, usize, Vec<Statement>, usize, usize, usize), Break, Continue(Option<usize>), Frees(Vec<(usize, Type)>) }
#[derive(Debug)]
struct Function { id: usize, params: Vec<(usize, Type)>, body: Vec<Statement>, is_main: bool, returns: Type }
fn returns(stmts: &[Statement]) -> bool {
    stmts.iter().any(|s| match s {
        Statement::Return(..) => true,
        Statement::If(_, yes, no) => returns(yes) && returns(no),
        _ => false,
    })
}
struct Parser<'a> { file: String, source: &'a str, tokens: Vec<Token>, pos: usize, names: HashMap<String, (usize, bool, Type)>, next_var: usize, expression_depth: usize, expression_nodes: usize, block_depth: usize, loop_depth: usize, loop_scopes: Vec<usize>, loop_labels: Vec<Option<usize>>, next_label: usize, functions: &'a mut HashMap<String, (usize, Vec<Type>, Type)>, id_order: Vec<usize>, in_main: bool, return_type: Type, moved: std::collections::HashSet<usize>, scopes: Vec<Vec<(usize, Type)>> }
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
                                    Type::Vec | Type::VecRef => {
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
                            if !matches!(ty, Type::String | Type::Vec | Type::VecRef) { return Err(self.fail("len expects a string, vec, or array variable")); }
                            (Expr::Len(Box::new(operand), line, col), Type::Int)
                        }
                    }
                } else if matches!(name.as_str(), "read_file" | "write_file" | "bytes" | "contains" | "slice") && *self.current() == Kind::LParen {
                    let (line, col) = (vline, vcol);
                    self.pos += 1;
                    let (first, first_ty) = self.expression(0, false)?;
                    if first_ty != Type::String { return Err(self.fail(&format!("{name} expects a string argument"))); }
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
                    } else if name == "contains" {
                        self.take(Kind::Comma)?;
                        let (second, second_ty) = self.expression(0, false)?;
                        if second_ty != Type::String { return Err(self.fail("contains expects a string pattern")); }
                        self.take(Kind::RParen)?;
                        Expr::Contains(Box::new(first), Box::new(second), line, col)
                    } else {
                        self.take(Kind::Comma)?;
                        let (second, second_ty) = self.expression(0, false)?;
                        if second_ty != Type::String { return Err(self.fail("write_file expects a string content")); }
                        self.take(Kind::RParen)?;
                        Expr::WriteFile(Box::new(first), Box::new(second), line, col)
                    };
                    let ty = match name.as_str() { "write_file" => Type::Int, "bytes" => Type::Vec, "contains" => Type::Bool, _ => Type::String };
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
                    } else if (ty == Type::Vec || ty == Type::VecRef) && *self.current() == Kind::LBracket {
                        // Vec variables may also be used bare (moves, len);
                        // indexing is optional. Borrowed vectors are read-only.
                        let (line, col) = self.position(); self.take(Kind::LBracket)?;
                        let (index, index_ty) = self.expression(0, false)?;
                        if index_ty != Type::Int { return Err(self.fail("vector index must be an integer")); }
                        self.take(Kind::RBracket)?;
                        (Expr::VecIndex(id, Box::new(index), line, col, ty == Type::VecRef), Type::Int)
                    } else {
                        if moving && ty.owned() { self.moved.insert(id); }
                        (Expr::Variable(id, ty, vline, vcol), ty)
                    }
                }
            }
            Kind::LParen => { self.pos += 1; let inner = self.expression(0, moving)?; self.take(Kind::RParen)?; inner }
            _ => return Err(self.fail("expected expression")),
        };
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
                    if lhs_ty != rhs_ty || !matches!(lhs_ty, Type::Int | Type::String) {
                        return Err(self.fail("comparison requires two integers or two strings"));
                    }
                    // String equality gets a dedicated node so emission never
                    // confuses it with C pointer comparison.
                    if (op == Kind::EqEq || op == Kind::NotEq) && lhs_ty == Type::String {
                        lhs = (Expr::StrEq(Box::new(lhs_expr), Box::new(rhs), op == Kind::NotEq), Type::Bool);
                        continue;
                    }
                    Type::Bool
                }
                Kind::Plus if lhs_ty == Type::String || rhs_ty == Type::String => {
                    if lhs_ty != Type::String || rhs_ty != Type::String { return Err(self.fail("'+' requires two strings or two integers")); }
                    // The left operand was parsed in the caller's mode; using it
                    // here consumes it, so retroactively record the move.
                    if let Expr::Variable(id, ty, _, _) = &lhs_expr { if ty.owned() && !self.moved.contains(id) { self.moved.insert(*id); } }
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
            let name = self.identifier()?;
            match name.as_str() {
                "i64" => Ok(Type::Int),
                "bool" => Ok(Type::Bool),
                "string" => Ok(Type::String),
                "vec" => Ok(Type::Vec),
                _ => Err(self.fail("parameter type must be i64, bool, string, vec, or &[i64; N]")),
            }
        }
    }
    fn return_annotation(&mut self) -> Result<Type, String> {
        let name = self.identifier()?;
        match name.as_str() {
            "i64" => Ok(Type::Int),
            "bool" => Ok(Type::Bool),
            "string" => Ok(Type::String),
            "vec" => Ok(Type::Vec),
            _ => Err(self.fail("return type must be i64, bool, string, or vec")),
        }
    }
    // First pass collects only signatures. Bodies are checked in the second pass.
    // scan_uses validated the headers; skip them before collecting.
    fn skip_uses(&mut self) {
        while *self.current() == Kind::Use { self.pos += 3; }
    }
    fn collect_functions(&mut self, next_id: &mut usize) -> Result<(), String> {
        let mut seen = std::collections::HashSet::new();
        while *self.current() != Kind::Eof {
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
        // Moved markers for names that left scope are no longer meaningful.
        let live: std::collections::HashSet<usize> = self.names.values().map(|&(id, _, _)| id).collect();
        self.moved.retain(|id| live.contains(id));
        Ok(result)
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
                        if ty != Type::Vec { return Err(self.fail(&format!("{name} expects a vec, found {}", ty.name()))); }
                        if !mutable { return Err(self.fail(&format!("cannot modify immutable variable '{target}'"))); }
                        if name == "push" {
                            self.take(Kind::Comma)?;
                            let (value, value_ty) = self.expression(0, false)?;
                            if value_ty != Type::Int { return Err(self.fail("vector elements must be i64")); }
                            self.take(Kind::RParen)?; self.take(Kind::Semi)?;
                            result.push(Statement::VecPush(id, value, line, col));
                        } else {
                            self.take(Kind::RParen)?; self.take(Kind::Semi)?;
                            result.push(Statement::VecPop(id, line, col));
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
                    } else if ty == Type::Vec && *self.current() == Kind::LBracket {
                        let (line, col) = self.position(); self.take(Kind::LBracket)?;
                        let (index, index_ty) = self.expression(0, false)?;
                        if index_ty != Type::Int { return Err(self.fail("vector index must be an integer")); }
                        self.take(Kind::RBracket)?; self.take(Kind::Equal)?;
                        let (value, value_ty) = self.expression(0, false)?; self.take(Kind::Semi)?;
                        if value_ty != Type::Int { return Err(self.fail("vector elements must be i64")); }
                        result.push(Statement::VecStore(id, index, value, line, col));
                    } else {
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
                    if self.names.contains_key(&var_name) { return Err(self.fail(&format!("duplicate variable '{var_name}'"))); }
                    self.take(Kind::In)?;
                    let (start, start_ty) = self.expression(0, false)?;
                    if start_ty != Type::Int { return Err(self.fail("range bounds must be i64")); }
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
                    result.push(Statement::Declare(var, start));
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
                        if !matches!(ty, Type::Int | Type::Bool | Type::String) {
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
                let probe = match ty { Type::Vec => format!("!v{id}.data"), _ => format!("!v{id}.ptr") };
                let message = match ty { Type::Vec => "use of moved vector", _ => "use of moved string" };
                out.push_str(&format!("    if ({probe}) ae_fail(\"{message}\", {line}, {column});\n"));
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
                Type::String => format!("ae_str_len({}, {line}, {col})", t.text),
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
            out.push_str(&format!("    int64_t t{id} = ae_write_file({}, {}, {line}, {col});
", p.text, c.text));
            Emitted { text: format!("t{id}"), ty: Type::Int, owned: false, moves: Vec::new(), materialized: true }
        }
        Expr::Bytes(e, line, col) => {
            let t = emit_expr(e, out, next, false);
            let id = *next; *next += 1;
            out.push_str(&format!("    ae_vec t{id} = ae_bytes({}, {line}, {col});
", t.text));
            Emitted { text: format!("t{id}"), ty: Type::Vec, owned: true, moves: Vec::new(), materialized: true }
        }
        Expr::Slice(e, start, end, line, col) => {
            let t = emit_expr(e, out, next, false);
            let st = emit_expr(start, out, next, false);
            let en = emit_expr(end, out, next, false);
            let id = *next; *next += 1;
            out.push_str(&format!("    ae_string t{id} = ae_slice({}, {}, {}, {line}, {col});
", t.text, st.text, en.text));
            if t.owned { drop_owned_temp(out, &t); }
            Emitted { text: format!("t{id}"), ty: Type::String, owned: true, moves: Vec::new(), materialized: true }
        }
        Expr::Contains(haystack, needle, line, col) => {
            let h = emit_expr(haystack, out, next, false);
            let n = emit_expr(needle, out, next, false);
            let id = *next; *next += 1;
            out.push_str(&format!("    bool t{id} = ae_contains({}, {}, {line}, {col});
", h.text, n.text));
            if h.owned { drop_owned_temp(out, &h); }
            if n.owned { drop_owned_temp(out, &n); }
            Emitted { text: format!("t{id}"), ty: Type::Bool, owned: false, moves: Vec::new(), materialized: true }
        }
        Expr::StrEq(a, b, negated) => {
            let l = emit_expr(a, out, next, false);
            let r = emit_expr(b, out, next, false);
            let id = *next; *next += 1;
            let cmp = format!("ae_str_eq({}, {})", l.text, r.text);
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
        _ => out.push_str(&format!("    ae_str_release(&v{id});\n")),
    }
}
// An owned temporary (not a named slot) that a statement has finished using.
fn drop_owned_temp(out: &mut String, emitted: &Emitted) {
    match emitted.ty {
        Type::Vec => out.push_str(&format!("    ae_vec_release(&{});\n", emitted.text)),
        Type::String => out.push_str(&format!("    ae_str_release(&{});\n", emitted.text)),
        _ => out.push_str(&format!("    free({});\n", emitted.text)),
    }
}
fn null_moves(out: &mut String, moves: &[(usize, Type)]) {
    for &(id, ty) in moves {
        match ty {
            Type::Vec => out.push_str(&format!("    ae_vec_forfeit(&v{id});\n")),
            Type::String => out.push_str(&format!("    ae_str_forfeit(&v{id});\n")),
            _ => out.push_str(&format!("    v{id} = NULL;\n")),
        }
    }
}
fn emit_print(printed: &Printed, out: &mut String, next: &mut usize) {
    match printed {
        Printed::Expr(e) => {
            let t = emit_expr(e, out, next, false);
            match t.ty {
                Type::String => {
                    out.push_str(&format!("    if (fwrite({}.ptr, 1, (size_t){}.len, stdout) != (size_t){}.len || fputc('\\n', stdout) == EOF) ae_fail(\"output error\", 0, 0);\n", t.text, t.text, t.text));
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
fn make_parser<'a>(file: &str, source: &'a str, tokens: Vec<Token>, functions: &'a mut HashMap<String, (usize, Vec<Type>, Type)>) -> Parser<'a> {
    Parser { file: file.to_owned(), source, tokens, pos: 0, names: HashMap::new(), next_var: 0, expression_depth: 0, expression_nodes: 0, block_depth: 0, loop_depth: 0, loop_scopes: Vec::new(), loop_labels: Vec::new(), next_label: 0, functions, id_order: Vec::new(), in_main: false, return_type: Type::Int, moved: std::collections::HashSet::new(), scopes: Vec::new() }
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
fn parse_source(file: &str, source: &str, shared: &mut HashMap<String, (usize, Vec<Type>, Type)>, next_id: &mut usize) -> Result<Vec<Function>, String> {
    let tokens = lex(source)?;
    let uses = scan_uses(file, &tokens)?;
    if !uses.is_empty() {
        return Err(error(file, source, 0, "'use' is only supported when compiling a source file"));
    }
    let mut p = make_parser(file, source, tokens, shared);
    p.collect_functions(next_id)?;
    let functions = p.program()?;
    if functions.iter().filter(|f| f.is_main).count() != 1 {
        return Err(error(file, source, 0, "missing main function"));
    }
    Ok(functions)
}

fn load_file(path: &Path, visited: &mut std::collections::HashSet<std::path::PathBuf>, shared: &mut HashMap<String, (usize, Vec<Type>, Type)>, next_id: &mut usize, out: &mut Vec<Vec<Function>>, mains: &mut usize) -> Result<(), String> {
    let canonical = std::fs::canonicalize(path).map_err(|e| format!("cannot open {}: {e}", path.display()))?;
    if !visited.insert(canonical) { return Err(format!("circular import of {}", path.display())); }
    let source = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let file = path.display().to_string();
    let tokens = lex(&source).map_err(|e| format!("{file}:{e}"))?;
    let uses = scan_uses(&file, &tokens).map_err(|e| format!("{file}:{e}"))?;
    let parent = path.parent().unwrap_or(Path::new(".")).to_owned();
    for import in uses {
        load_file(&parent.join(&import), visited, shared, next_id, out, mains)?;
    }
    let mut p = make_parser(&file, &source, tokens, shared);
    p.skip_uses();
    p.collect_functions(next_id)?;
    let functions = p.program()?;
    *mains += functions.iter().filter(|f| f.is_main).count();
    out.push(functions);
    Ok(())
}

fn load_program(entry: &Path) -> Result<Vec<Vec<Function>>, String> {
    let mut visited = std::collections::HashSet::new();
    let mut shared: HashMap<String, (usize, Vec<Type>, Type)> = HashMap::new();
    let mut next_id = 0usize;
    let mut files = Vec::new();
    let mut mains = 0usize;
    load_file(entry, &mut visited, &mut shared, &mut next_id, &mut files, &mut mains)?;
    if mains != 1 { return Err(format!("program must define exactly one main function (found {mains})")); }
    Ok(files)
}

pub fn check_path(path: &Path) -> Result<(), String> {
    load_program(path).map(|_| ())
}
#[allow(dead_code)] // exercised by in-memory unit tests
pub fn check(source: &str) -> Result<(), String> {
    let mut shared: HashMap<String, (usize, Vec<Type>, Type)> = HashMap::new();
    let mut next_id = 0usize;
    parse_source("", source, &mut shared, &mut next_id).map(|_| ())
}
fn c_type(ty: Type, id: usize, declaration: bool) -> String {
    match ty {
        Type::Int => format!("int64_t v{id}"),
        Type::Bool => format!("bool v{id}"),
        Type::String => format!("ae_string v{id}"),
        Type::Vec => format!("ae_vec v{id}"),
        Type::VecRef if declaration => format!("const ae_vec *v{id}"),
        Type::VecRef => "const ae_vec *".to_owned(),
        Type::Array(_) if declaration => format!("const int64_t *v{id}"),
        Type::Array(_) => "const int64_t *".to_owned(),
    }
}
fn emit_program(files: Vec<Vec<Function>>) -> Result<String, String> {
    let mut out = String::from(include_str!("runtime.c"));
    let all: Vec<&Function> = files.iter().flatten().collect();
    for function in &all {
        if function.is_main { continue; }
        let params = if function.params.is_empty() { "void".to_owned() } else { function.params.iter().map(|&(id, ty)| c_type(ty, id, false)).collect::<Vec<_>>().join(", ") };
        let ret = match function.returns { Type::Bool => "bool", Type::String => "ae_string", Type::Vec => "ae_vec", _ => "int64_t" };
        out.push_str(&format!("\nstatic {ret} ae_fn_{}({params});\n", function.id));
    }
    for function in &all {
        if function.is_main { continue; }
        let params = if function.params.is_empty() { "void".to_owned() } else { function.params.iter().map(|&(id, ty)| c_type(ty, id, true)).collect::<Vec<_>>().join(", ") };
        let ret = match function.returns { Type::Bool => "bool", Type::String => "ae_string", Type::Vec => "ae_vec", _ => "int64_t" };
        out.push_str(&format!("\nstatic {ret} ae_fn_{}({params}) {{\n", function.id));
        emit_statements(&function.body, &mut out, &mut 0);
        // Defensive backstop: even a future control-flow analysis bug must not
        // produce a C function that falls through without returning a value.
        out.push_str("    ae_fail(\"function exited without return\", 0, 0);\n}\n");
    }
    let main = all.iter().find(|f| f.is_main).expect("load_program checks exactly one main");
    out.push_str("\nint main(void) {\n");
    emit_statements(&main.body, &mut out, &mut 0);
    out.push_str("    if (fflush(stdout) == EOF) return 1;\n    return 0;\n}\n");
    Ok(out)
}
#[allow(dead_code)] // exercised by in-memory unit tests
pub fn compile(source: &str) -> Result<String, String> {
    let mut shared: HashMap<String, (usize, Vec<Type>, Type)> = HashMap::new();
    let mut next_id = 0usize;
    let functions = parse_source("", source, &mut shared, &mut next_id)?;
    emit_program(vec![functions])
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
