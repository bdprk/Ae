use std::collections::HashMap;

#[derive(Debug, Clone, PartialEq)]
enum Kind {
    Ident(String), Number(i64), Text(String), Fn, Let, Var, Print, If, Else, While, Return, Break, Continue, Assert, True, False,
    EqEq, NotEq, Less, LessEq, Greater, GreaterEq, And, Or, Bang,
    LParen, RParen, LBrace, RBrace, LBracket, RBracket, Comma, Colon, Arrow, Amp, Semi, Equal, Plus, Minus, Star, Slash, Percent, Eof,
}
#[derive(Debug, Clone)]
struct Token { kind: Kind, offset: usize, line: usize, column: usize }

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Type { Int, Bool, Array(usize) }
impl Type {
    fn name(self) -> &'static str {
        match self { Type::Int => "i64", Type::Bool => "bool", Type::Array(_) => "array" }
    }
}

fn location(source: &str, offset: usize) -> (usize, usize) {
    let prefix = &source[..offset];
    (prefix.bytes().filter(|b| *b == b'\n').count() + 1,
     prefix.rsplit('\n').next().unwrap_or("").chars().count() + 1)
}
fn error(source: &str, offset: usize, message: &str) -> String {
    let (line, column) = location(source, offset);
    format!("{line}:{column}: {message}")
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
                    b'\n' | b'\0' => return Err(error(source, i, "string literals must not contain raw newlines or NUL; use escapes")),
                    b'\\' => {
                        let escaped = bytes.get(i + 1).copied();
                        if !matches!(escaped, Some(b'n' | b't' | b'\\' | b'"')) {
                            return Err(error(source, i, "unsupported string escape; allowed: \\n \\t \\\\ \\\""));
                        }
                        i += 2;
                    }
                    _ => i += 1,
                }
            }
            if i >= bytes.len() { return Err(error(source, start, "unterminated string literal")); }
            i += 1;
            Kind::Text(source[start + 1..i - 1].to_owned())
        } else if c.is_ascii_alphabetic() || c == '_' {
            i += 1;
            while i < bytes.len() && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_') { i += 1; }
            match &source[start..i] {
                "fn" => Kind::Fn, "let" => Kind::Let, "var" => Kind::Var, "print" => Kind::Print,
                "if" => Kind::If, "else" => Kind::Else, "while" => Kind::While, "return" => Kind::Return,
                "break" => Kind::Break, "continue" => Kind::Continue, "assert" => Kind::Assert,
                "true" => Kind::True, "false" => Kind::False,
                s => Kind::Ident(s.to_owned()),
            }
        } else if c.is_ascii_digit() {
            i += 1;
            while i < bytes.len() && bytes[i].is_ascii_digit() { i += 1; }
            Kind::Number(source[start..i].parse().map_err(|_| error(source, start, "integer literal exceeds i64 maximum"))?)
        } else if ["==", "!=", "<=", ">=", "->", "&&", "||"].iter().any(|op| source[i..].starts_with(op)) {
            i += 2;
            match &source[start..i] { "==" => Kind::EqEq, "!=" => Kind::NotEq, "<=" => Kind::LessEq, ">=" => Kind::GreaterEq, "->" => Kind::Arrow, "&&" => Kind::And, "||" => Kind::Or, _ => unreachable!() }
        } else {
            i += c.len_utf8();
            match c {
                '(' => Kind::LParen, ')' => Kind::RParen, '{' => Kind::LBrace, '}' => Kind::RBrace,
                '[' => Kind::LBracket, ']' => Kind::RBracket, ',' => Kind::Comma, ':' => Kind::Colon, '&' => Kind::Amp,
                ';' => Kind::Semi, '=' => Kind::Equal, '+' => Kind::Plus, '-' => Kind::Minus,
                '*' => Kind::Star, '/' => Kind::Slash, '%' => Kind::Percent, '<' => Kind::Less, '>' => Kind::Greater,
                '!' => Kind::Bang,
                _ => return Err(error(source, start, &format!("unexpected character {c:?}"))),
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
enum Expr { Number(i64), BoolLit(bool), Variable(usize, Type), Call(usize, Vec<Argument>, Type), Index(usize, usize, Box<Expr>, usize, usize), Neg(Box<Expr>, usize, usize), Not(Box<Expr>), Binary(Box<Expr>, Kind, Box<Expr>, usize, usize, Type) }
#[derive(Debug)]
enum Argument { Value(Expr), Borrow(usize) }
#[derive(Debug)]
enum Printed { Expr(Expr), Text(String) }
#[derive(Debug)]
enum Statement { Return(Expr), Declare(usize, Expr), Array(usize, Vec<Expr>), Assign(usize, Expr), Store(usize, usize, Expr, Expr, usize, usize), Print(Printed), Assert(Expr, usize, usize), If(Expr, Vec<Statement>, Vec<Statement>), While(Expr, Vec<Statement>), Break, Continue }
#[derive(Debug)]
struct Function { id: usize, params: Vec<(usize, Type)>, body: Vec<Statement>, is_main: bool, returns: Type }
fn returns(stmts: &[Statement]) -> bool {
    stmts.iter().any(|s| match s {
        Statement::Return(_) => true,
        Statement::If(_, yes, no) => returns(yes) && returns(no),
        _ => false,
    })
}
struct Parser<'a> { source: &'a str, tokens: Vec<Token>, pos: usize, names: HashMap<String, (usize, bool, Type)>, next_var: usize, expression_depth: usize, expression_nodes: usize, block_depth: usize, loop_depth: usize, functions: HashMap<String, (usize, Vec<Type>, Type)>, in_main: bool, return_type: Type }
impl<'a> Parser<'a> {
    fn current(&self) -> &Kind { &self.tokens[self.pos].kind }
    fn fail(&self, message: &str) -> String { error(self.source, self.tokens[self.pos].offset, message) }
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
    fn expression(&mut self, min: u8) -> Result<(Expr, Type), String> {
        if self.expression_depth == 0 { self.expression_nodes = 0; }
        if self.expression_depth >= 128 { return Err(self.fail("expression nesting limit exceeded (128)")); }
        self.expression_depth += 1;
        let result = self.expression_inner(min);
        self.expression_depth -= 1;
        result
    }
    fn expression_inner(&mut self, min: u8) -> Result<(Expr, Type), String> {
        self.expression_nodes += 1;
        if self.expression_nodes > 128 { return Err(self.fail("expression complexity limit exceeded (128)")); }
        let mut lhs = match self.current().clone() {
            Kind::Number(n) => { self.pos += 1; (Expr::Number(n), Type::Int) }
            Kind::True => { self.pos += 1; (Expr::BoolLit(true), Type::Bool) }
            Kind::False => { self.pos += 1; (Expr::BoolLit(false), Type::Bool) }
            Kind::Minus => {
                let (line, column) = self.position();
                self.pos += 1;
                // Operands bind tighter than any binary operator: -a * b is (-a) * b.
                let (operand, ty) = self.expression(6)?;
                if ty != Type::Int { return Err(self.fail("unary '-' requires an integer operand")); }
                (Expr::Neg(Box::new(operand), line, column), Type::Int)
            }
            Kind::Bang => {
                self.pos += 1;
                let (operand, ty) = self.expression(6)?;
                if ty != Type::Bool { return Err(self.fail("'!' requires a boolean operand")); }
                (Expr::Not(Box::new(operand)), Type::Bool)
            }
            Kind::Ident(name) => {
                let name_offset = self.tokens[self.pos].offset;
                self.pos += 1;
                if *self.current() == Kind::LParen {
                    let (id, types, ret) = self.functions.get(&name).cloned().ok_or_else(|| error(self.source, name_offset, &format!("unknown function '{name}'")))?;
                    self.pos += 1;
                    let mut args = Vec::new();
                    if *self.current() != Kind::RParen {
                        loop {
                            if args.len() >= 64 { return Err(self.fail("function argument limit exceeded (64)")); }
                            let expected = *types.get(args.len()).ok_or_else(|| self.fail(&format!("too many arguments to '{name}'")))?;
                            if let Type::Array(len) = expected {
                                self.take(Kind::Amp)?;
                                let arg = self.identifier()?;
                                let (var, _, actual) = *self.names.get(&arg).ok_or_else(|| self.fail(&format!("unknown array '{arg}'")))?;
                                if actual != Type::Array(len) { return Err(self.fail(&format!("argument requires array of length {len}"))); }
                                args.push(Argument::Borrow(var));
                            } else {
                                let (value, ty) = self.expression(0)?;
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
                    let (id, _, ty) = *self.names.get(&name).ok_or_else(|| error(self.source, name_offset, &format!("unknown variable '{name}'")))?;
                    if let Type::Array(len) = ty {
                        let (line, col) = self.position(); self.take(Kind::LBracket)?;
                        let (index, index_ty) = self.expression(0)?;
                        if index_ty != Type::Int { return Err(self.fail("array index must be an integer")); }
                        self.take(Kind::RBracket)?;
                        (Expr::Index(id, len, Box::new(index), line, col), Type::Int)
                    } else { (Expr::Variable(id, ty), ty) }
                }
            }
            Kind::LParen => { self.pos += 1; let inner = self.expression(0)?; self.take(Kind::RParen)?; inner }
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
            let (rhs, rhs_ty) = self.expression(prec + 1)?;
            let (lhs_expr, lhs_ty) = lhs;
            let ty = match op {
                Kind::And | Kind::Or => {
                    if lhs_ty != Type::Bool || rhs_ty != Type::Bool { return Err(self.fail(&format!("'{}' requires boolean operands", if op == Kind::And { "&&" } else { "||" }))); }
                    Type::Bool
                }
                Kind::EqEq | Kind::NotEq | Kind::Less | Kind::LessEq | Kind::Greater | Kind::GreaterEq => {
                    if lhs_ty != Type::Int || rhs_ty != Type::Int { return Err(self.fail("comparison requires integer operands")); }
                    Type::Bool
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
    // Arrays are borrowed read-only; scalars pass by value.
    fn parameter_type(&mut self) -> Result<Type, String> {
        if *self.current() == Kind::Amp {
            self.pos += 1; self.take(Kind::LBracket)?;
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
                _ => Err(self.fail("parameter type must be i64, bool, or &[i64; N]")),
            }
        }
    }
    fn return_annotation(&mut self) -> Result<Type, String> {
        let name = self.identifier()?;
        match name.as_str() {
            "i64" => Ok(Type::Int),
            "bool" => Ok(Type::Bool),
            _ => Err(self.fail("return type must be i64 or bool")),
        }
    }
    // First pass collects only signatures. Bodies are checked in the second pass.
    fn collect_functions(&mut self) -> Result<(), String> {
        let mut id = 0;
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
                    self.identifier()?; self.take(Kind::Colon)?;
                    types.push(self.parameter_type()?);
                    if *self.current() != Kind::Comma { break; }
                    self.pos += 1;
                }
            }
            self.take(Kind::RParen)?;
            if name == "main" {
                if !types.is_empty() { return Err(self.fail("main cannot have parameters")); }
            } else {
                self.take(Kind::Arrow)?;
                let ret = self.return_annotation()?;
                self.functions.insert(name, (id, types, ret));
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
            id += 1;
        }
        if !seen.contains("main") { return Err(self.fail("missing main function")); }
        self.pos = 0;
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
            self.names.clear(); self.next_var = 0;
            let mut params = Vec::new();
            if *self.current() != Kind::RParen {
                loop {
                    if params.len() >= 64 { return Err(self.fail("function parameter limit exceeded (64)")); }
                    let param = self.identifier()?;
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
            let id = result.len();
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
            let body = self.block()?;
            if !is_main && !returns(&body) { return Err(self.fail("function may exit without returning a value")); }
            result.push(Function { id, params, body, is_main, returns: returns_type });
        }
        if !main_found { return Err(self.fail("missing main function")); }
        Ok(result)
    }
    fn block(&mut self) -> Result<Vec<Statement>, String> {
        if self.block_depth >= 64 { return Err(self.fail("block nesting limit exceeded (64)")); }
        self.block_depth += 1;
        let result = self.block_inner();
        self.block_depth -= 1;
        result
    }
    fn block_inner(&mut self) -> Result<Vec<Statement>, String> {
        self.take(Kind::LBrace)?;
        let mut declared = Vec::new();
        let mut result = Vec::new();
        while *self.current() != Kind::RBrace {
            let stmt = match self.current().clone() {
                Kind::Let | Kind::Var => {
                    let mutable = *self.current() == Kind::Var; self.pos += 1;
                    let name = self.identifier()?;
                    if self.names.contains_key(&name) { return Err(self.fail(&format!("duplicate variable '{name}'"))); }
                    self.take(Kind::Equal)?;
                    let id = self.next_var; self.next_var += 1;
                    let (stmt, ty) = if *self.current() == Kind::LBracket {
                        self.pos += 1;
                        let mut values = Vec::new();
                        if *self.current() == Kind::RBracket { return Err(self.fail("array must contain at least one element")); }
                        loop {
                            if values.len() == 4096 { return Err(self.fail("array element limit exceeded (4096)")); }
                            let (element, element_ty) = self.expression(0)?;
                            if element_ty != Type::Int { return Err(self.fail("array elements must be i64")); }
                            values.push(element);
                            if *self.current() != Kind::Comma { break; }
                            self.pos += 1;
                        }
                        self.take(Kind::RBracket)?; self.take(Kind::Semi)?;
                        let len = values.len(); (Statement::Array(id, values), Type::Array(len))
                    } else {
                        let (value, ty) = self.expression(0)?; self.take(Kind::Semi)?;
                        (Statement::Declare(id, value), ty)
                    };
                    self.names.insert(name.clone(), (id, mutable, ty)); declared.push(name);
                    stmt
                }
                Kind::Ident(name) => {
                    let (id, mutable, ty) = *self.names.get(&name).ok_or_else(|| self.fail(&format!("unknown variable '{name}'")))?;
                    if !mutable { return Err(self.fail(&format!("cannot assign to immutable variable '{name}'"))); }
                    self.pos += 1;
                    if let Type::Array(len) = ty {
                        let (line, col) = self.position(); self.take(Kind::LBracket)?;
                        let (index, index_ty) = self.expression(0)?;
                        if index_ty != Type::Int { return Err(self.fail("array index must be an integer")); }
                        self.take(Kind::RBracket)?; self.take(Kind::Equal)?;
                        let (value, value_ty) = self.expression(0)?; self.take(Kind::Semi)?;
                        if value_ty != Type::Int { return Err(self.fail("array elements must be i64")); }
                        Statement::Store(id, len, index, value, line, col)
                    } else {
                        self.take(Kind::Equal)?;
                        let (value, value_ty) = self.expression(0)?; self.take(Kind::Semi)?;
                        if value_ty != ty { return Err(self.fail(&format!("cannot assign a {} value to {} variable '{name}'", value_ty.name(), ty.name()))); }
                        Statement::Assign(id, value)
                    }
                }
                Kind::Return => {
                    if self.in_main { return Err(self.fail("return is not supported in main")); }
                    self.pos += 1;
                    let (value, ty) = self.expression(0)?; self.take(Kind::Semi)?;
                    if ty != self.return_type { return Err(self.fail(&format!("return type must be {}", self.return_type.name()))); }
                    Statement::Return(value)
                }
                Kind::If => self.if_statement()?,
                Kind::While => {
                    self.pos += 1;
                    let (condition, ty) = self.expression(0)?;
                    if ty != Type::Bool { return Err(self.fail("condition must be a boolean expression")); }
                    if self.loop_depth >= 64 { return Err(self.fail("loop nesting limit exceeded (64)")); }
                    self.loop_depth += 1;
                    let body = self.block();
                    self.loop_depth -= 1;
                    Statement::While(condition, body?)
                }
                Kind::Break | Kind::Continue => {
                    let keyword = if *self.current() == Kind::Break { "break" } else { "continue" };
                    if self.loop_depth == 0 { return Err(self.fail(&format!("'{keyword}' outside of a loop"))); }
                    self.pos += 1; self.take(Kind::Semi)?;
                    if keyword == "break" { Statement::Break } else { Statement::Continue }
                }
                Kind::Assert => {
                    let (line, col) = self.position();
                    self.pos += 1; self.take(Kind::LParen)?;
                    let (condition, ty) = self.expression(0)?;
                    if ty != Type::Bool { return Err(self.fail("assert requires a boolean condition")); }
                    self.take(Kind::RParen)?; self.take(Kind::Semi)?;
                    Statement::Assert(condition, line, col)
                }
                Kind::Print => {
                    self.pos += 1; self.take(Kind::LParen)?;
                    let printed = if let Kind::Text(text) = self.current().clone() {
                        self.pos += 1; Printed::Text(text)
                    } else {
                        let (value, _) = self.expression(0)?; Printed::Expr(value)
                    };
                    self.take(Kind::RParen)?; self.take(Kind::Semi)?;
                    Statement::Print(printed)
                }
                _ => return Err(self.fail("expected declaration, assignment, print, or '}'")),
            };
            result.push(stmt);
        }
        self.take(Kind::RBrace)?;
        for name in declared { self.names.remove(&name); }
        Ok(result)
    }
    fn if_statement(&mut self) -> Result<Statement, String> {
        self.pos += 1;
        let (condition, ty) = self.expression(0)?;
        if ty != Type::Bool { return Err(self.fail("condition must be a boolean expression")); }
        let yes = self.block()?;
        let no = if *self.current() == Kind::Else {
            self.pos += 1;
            // `else if` chains recurse instead of nesting braces.
            if *self.current() == Kind::If { vec![self.if_statement()?] } else { self.block()? }
        } else { Vec::new() };
        Ok(Statement::If(condition, yes, no))
    }
}

// Expressions are emitted as sequential temporaries, preserving left-to-right
// evaluation. && and || lower to control flow so the right operand's side
// effects only happen when the left operand does not decide the result.
fn emit_expr(expr: &Expr, out: &mut String, next: &mut usize) -> (String, Type) {
    let (expression, ty) = match expr {
        Expr::Number(n) => (format!("INT64_C({n})"), Type::Int),
        Expr::BoolLit(b) => (if *b { "true" } else { "false" }.to_owned(), Type::Bool),
        Expr::Variable(id, ty) => (format!("v{id}"), *ty),
        Expr::Call(id, args, ty) => {
            let arguments = args.iter().map(|arg| match arg {
                Argument::Value(e) => emit_expr(e, out, next).0,
                Argument::Borrow(id) => format!("v{id}"),
            }).collect::<Vec<_>>().join(", ");
            (format!("ae_fn_{id}({arguments})"), *ty)
        }
        Expr::Index(id, len, index, line, col) => {
            let (i, _) = emit_expr(index, out, next);
            (format!("v{id}[ae_index({i}, {len}, {line}, {col})]"), Type::Int)
        }
        Expr::Neg(a, line, column) => {
            let (t, _) = emit_expr(a, out, next);
            (format!("ae_neg({t}, {line}, {column})"), Type::Int)
        }
        Expr::Not(a) => {
            let (t, _) = emit_expr(a, out, next);
            (format!("!{t}"), Type::Bool)
        }
        Expr::Binary(a, op, b, line, column, _) if *op == Kind::And || *op == Kind::Or => {
            let (l, _) = emit_expr(a, out, next);
            let id = *next; *next += 1;
            // `&&` starts false and fills the result only when the left side
            // is true; `||` mirrors it. The right operand's statements land
            // inside the if body, giving real short-circuit semantics.
            let initial = if *op == Kind::And { "false" } else { "true" };
            let guard = if *op == Kind::And { l } else { format!("!{l}") };
            out.push_str(&format!("    bool t{id} = {initial};\n    if ({guard}) {{\n"));
            let (r, _) = emit_expr(b, out, next);
            out.push_str(&format!("    t{id} = {r};\n    }}\n"));
            return (format!("t{id}"), Type::Bool);
        }
        Expr::Binary(a, op, b, line, column, _) => {
            let (l, _) = emit_expr(a, out, next); let (r, _) = emit_expr(b, out, next);
            let (statement, ty) = match op {
                Kind::EqEq => (format!("{l} == {r}"), Type::Bool),
                Kind::NotEq => (format!("{l} != {r}"), Type::Bool),
                Kind::Less => (format!("{l} < {r}"), Type::Bool),
                Kind::LessEq => (format!("{l} <= {r}"), Type::Bool),
                Kind::Greater => (format!("{l} > {r}"), Type::Bool),
                Kind::GreaterEq => (format!("{l} >= {r}"), Type::Bool),
                Kind::Plus => (format!("ae_add({l}, {r}, {line}, {column})"), Type::Int),
                Kind::Minus => (format!("ae_sub({l}, {r}, {line}, {column})"), Type::Int),
                Kind::Star => (format!("ae_mul({l}, {r}, {line}, {column})"), Type::Int),
                Kind::Slash => (format!("ae_div({l}, {r}, {line}, {column})"), Type::Int),
                Kind::Percent => (format!("ae_mod({l}, {r}, {line}, {column})"), Type::Int),
                _ => unreachable!(),
            };
            (statement, ty)
        }
    };
    let id = *next; *next += 1;
    let cty = match ty { Type::Bool => "bool", _ => "int64_t" };
    out.push_str(&format!("    {cty} t{id} = {expression};\n"));
    (format!("t{id}"), ty)
}
fn emit_print(printed: &Printed, out: &mut String, next: &mut usize) {
    match printed {
        Printed::Expr(e) => {
            let (t, ty) = emit_expr(e, out, next);
            match ty {
                Type::Bool => out.push_str(&format!("    if (printf(\"%s\\n\", {t} ? \"true\" : \"false\") < 0) ae_fail(\"output error\", 0, 0);\n")),
                _ => out.push_str(&format!("    if (printf(\"%\" PRId64 \"\\n\", {t}) < 0) ae_fail(\"output error\", 0, 0);\n")),
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
            Statement::Return(e) => { let (t, _) = emit_expr(e, out, next); out.push_str(&format!("    return {t};\n")); }
            Statement::Declare(id, e) => {
                let (t, ty) = emit_expr(e, out, next);
                let cty = match ty { Type::Bool => "bool", _ => "int64_t" };
                out.push_str(&format!("    {cty} v{id} = {t};\n"));
            }
            Statement::Array(id, values) => {
                let items: Vec<_> = values.iter().map(|e| emit_expr(e, out, next).0).collect();
                out.push_str(&format!("    int64_t v{id}[{}] = {{", items.len()));
                for item in items { out.push_str(&format!("{item},")); }
                out.push_str("};\n");
            }
            Statement::Store(id, len, index, value, line, col) => {
                let (i, _) = emit_expr(index, out, next);
                let checked = *next; *next += 1;
                out.push_str(&format!("    size_t t{checked} = ae_index({i}, {len}, {line}, {col});\n"));
                let (value, _) = emit_expr(value, out, next);
                out.push_str(&format!("    v{id}[t{checked}] = {value};\n"));
            }
            Statement::Assign(id, e) => { let (t, _) = emit_expr(e, out, next); out.push_str(&format!("    v{id} = {t};\n")); }
            Statement::Print(printed) => emit_print(printed, out, next),
            Statement::Assert(condition, line, col) => {
                let (t, _) = emit_expr(condition, out, next);
                out.push_str(&format!("    if (!({t})) ae_fail(\"assertion failed\", {line}, {col});\n"));
            }
            Statement::If(condition, yes, no) => {
                let (cond, _) = emit_expr(condition, out, next);
                out.push_str(&format!("    if ({cond}) {{\n")); emit_statements(yes, out, next);
                out.push_str("    } else {\n"); emit_statements(no, out, next); out.push_str("    }\n");
            }
            Statement::While(condition, body) => {
                out.push_str("    while (1) {\n");
                let (cond, _) = emit_expr(condition, out, next);
                out.push_str(&format!("    if (!({cond})) break;\n"));
                emit_statements(body, out, next); out.push_str("    }\n");
            }
            // The while(1) form re-evaluates the condition after `continue`.
            Statement::Break => out.push_str("    break;\n"),
            Statement::Continue => out.push_str("    continue;\n"),
        }
    }
}
fn parse(source: &str) -> Result<Vec<Function>, String> {
    let mut p = Parser { source, tokens: lex(source)?, pos: 0, names: HashMap::new(), next_var: 0, expression_depth: 0, expression_nodes: 0, block_depth: 0, loop_depth: 0, functions: HashMap::new(), in_main: false, return_type: Type::Int };
    p.collect_functions()?;
    p.program()
}
pub fn check(source: &str) -> Result<(), String> {
    parse(source).map(|_| ())
}
fn c_type(ty: Type, id: usize, declaration: bool) -> String {
    match ty {
        Type::Int => format!("int64_t v{id}"),
        Type::Bool => format!("bool v{id}"),
        Type::Array(_) if declaration => format!("const int64_t *v{id}"),
        Type::Array(_) => "const int64_t *".to_owned(),
    }
}
pub fn compile(source: &str) -> Result<String, String> {
    let functions = parse(source)?;
    let mut out = String::from(include_str!("runtime.c"));
    for function in &functions {
        if function.is_main { continue; }
        let params = if function.params.is_empty() { "void".to_owned() } else { function.params.iter().map(|&(id, ty)| c_type(ty, id, false)).collect::<Vec<_>>().join(", ") };
        let ret = if function.returns == Type::Bool { "bool" } else { "int64_t" };
        out.push_str(&format!("\nstatic {ret} ae_fn_{}({params});\n", function.id));
    }
    for function in &functions {
        if function.is_main { continue; }
        let params = if function.params.is_empty() { "void".to_owned() } else { function.params.iter().map(|&(id, ty)| c_type(ty, id, true)).collect::<Vec<_>>().join(", ") };
        let ret = if function.returns == Type::Bool { "bool" } else { "int64_t" };
        out.push_str(&format!("\nstatic {ret} ae_fn_{}({params}) {{\n", function.id));
        emit_statements(&function.body, &mut out, &mut 0);
        // Defensive backstop: even a future control-flow analysis bug must not
        // produce a C function that falls through without returning a value.
        out.push_str("    ae_fail(\"function exited without return\", 0, 0);\n}\n");
    }
    let main = functions.iter().find(|f| f.is_main).unwrap();
    out.push_str("\nint main(void) {\n");
    emit_statements(&main.body, &mut out, &mut 0);
    out.push_str("    if (fflush(stdout) == EOF) return 1;\n    return 0;\n}\n");
    Ok(out)
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
