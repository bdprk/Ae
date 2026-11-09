use std::collections::HashMap;

#[derive(Debug, Clone, PartialEq)]
enum Kind {
    Ident(String), Number(i64), Fn, Let, Var, Print, If, Else, While, Return, Break, Continue,
    EqEq, NotEq, Less, LessEq, Greater, GreaterEq,
    LParen, RParen, LBrace, RBrace, LBracket, RBracket, Comma, Colon, Arrow, Amp, Semi, Equal, Plus, Minus, Star, Slash, Percent, Eof,
}
#[derive(Debug, Clone)]
struct Token { kind: Kind, offset: usize, line: usize, column: usize }

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
        let kind = if c.is_ascii_alphabetic() || c == '_' {
            i += 1;
            while i < bytes.len() && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_') { i += 1; }
            match &source[start..i] {
                "fn" => Kind::Fn, "let" => Kind::Let, "var" => Kind::Var, "print" => Kind::Print,
                "if" => Kind::If, "else" => Kind::Else, "while" => Kind::While, "return" => Kind::Return,
                "break" => Kind::Break, "continue" => Kind::Continue,
                s => Kind::Ident(s.to_owned()),
            }
        } else if c.is_ascii_digit() {
            i += 1;
            while i < bytes.len() && bytes[i].is_ascii_digit() { i += 1; }
            Kind::Number(source[start..i].parse().map_err(|_| error(source, start, "integer literal exceeds i64 maximum"))?)
        } else if ["==", "!=", "<=", ">=", "->"].iter().any(|op| source[i..].starts_with(op)) {
            i += 2;
            match &source[start..i] { "==" => Kind::EqEq, "!=" => Kind::NotEq, "<=" => Kind::LessEq, ">=" => Kind::GreaterEq, "->" => Kind::Arrow, _ => unreachable!() }
        } else {
            i += c.len_utf8();
            match c {
                '(' => Kind::LParen, ')' => Kind::RParen, '{' => Kind::LBrace, '}' => Kind::RBrace,
                '[' => Kind::LBracket, ']' => Kind::RBracket, ',' => Kind::Comma, ':' => Kind::Colon, '&' => Kind::Amp,
                ';' => Kind::Semi, '=' => Kind::Equal, '+' => Kind::Plus, '-' => Kind::Minus,
                '*' => Kind::Star, '/' => Kind::Slash, '%' => Kind::Percent, '<' => Kind::Less, '>' => Kind::Greater,
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

#[derive(Debug)]
enum Expr { Number(i64), Variable(usize), Call(usize, Vec<Argument>), Index(usize, usize, Box<Expr>, usize, usize), Neg(Box<Expr>, usize, usize), Binary(Box<Expr>, Kind, Box<Expr>, usize, usize) }
#[derive(Debug)]
enum Argument { Value(Expr), Borrow(usize) }
#[derive(Debug)]
enum Statement { Return(Expr), Declare(usize, Expr), Array(usize, Vec<Expr>), Assign(usize, Expr), Store(usize, usize, Expr, Expr, usize, usize), Print(Expr), If(Condition, Vec<Statement>, Vec<Statement>), While(Condition, Vec<Statement>), Break, Continue }
#[derive(Debug)]
struct Condition(Expr, Kind, Expr);
#[derive(Debug)]
struct Function { id: usize, params: Vec<(usize, Option<usize>)>, body: Vec<Statement>, is_main: bool }
fn returns(stmts: &[Statement]) -> bool {
    stmts.iter().any(|s| match s {
        Statement::Return(_) => true,
        Statement::If(_, yes, no) => returns(yes) && returns(no),
        _ => false,
    })
}
struct Parser<'a> { source: &'a str, tokens: Vec<Token>, pos: usize, names: HashMap<String, (usize, bool, Option<usize>)>, next_var: usize, expression_depth: usize, expression_nodes: usize, block_depth: usize, loop_depth: usize, functions: HashMap<String, (usize, Vec<Option<usize>>)>, in_main: bool }
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
    fn expression(&mut self, min: u8) -> Result<Expr, String> {
        if self.expression_depth == 0 { self.expression_nodes = 0; }
        if self.expression_depth >= 128 { return Err(self.fail("expression nesting limit exceeded (128)")); }
        self.expression_depth += 1;
        let result = self.expression_inner(min);
        self.expression_depth -= 1;
        result
    }
    fn expression_inner(&mut self, min: u8) -> Result<Expr, String> {
        self.expression_nodes += 1;
        if self.expression_nodes > 128 { return Err(self.fail("expression complexity limit exceeded (128)")); }
        let mut lhs = match self.current().clone() {
            Kind::Number(n) => { self.pos += 1; Expr::Number(n) }
            Kind::Minus => {
                let (line, column) = self.position();
                self.pos += 1;
                // Operands bind tighter than any binary operator: -a * b is (-a) * b.
                let operand = self.expression(6)?;
                Expr::Neg(Box::new(operand), line, column)
            }
            Kind::Ident(name) => {
                let name_offset = self.tokens[self.pos].offset;
                self.pos += 1;
                if *self.current() == Kind::LParen {
                    let (id, types) = self.functions.get(&name).cloned().ok_or_else(|| error(self.source, name_offset, &format!("unknown function '{name}'")))?;
                    self.pos += 1;
                    let mut args = Vec::new();
                    if *self.current() != Kind::RParen {
                        loop {
                            if args.len() >= 64 { return Err(self.fail("function argument limit exceeded (64)")); }
                            let expected = types.get(args.len()).copied().ok_or_else(|| self.fail(&format!("too many arguments to '{name}'")))?;
                            if let Some(len) = expected {
                                self.take(Kind::Amp)?;
                                let arg = self.identifier()?;
                                let (var, _, actual) = *self.names.get(&arg).ok_or_else(|| self.fail(&format!("unknown array '{arg}'")))?;
                                if actual != Some(len) { return Err(self.fail(&format!("argument requires array of length {len}"))); }
                                args.push(Argument::Borrow(var));
                            } else { args.push(Argument::Value(self.expression(0)?)); }
                            if *self.current() != Kind::Comma { break; }
                            self.pos += 1;
                        }
                    }
                    self.take(Kind::RParen)?;
                    if args.len() != types.len() { return Err(self.fail(&format!("function '{name}' expects {} arguments, got {}", types.len(), args.len()))); }
                    Expr::Call(id, args)
                } else {
                    let (id, _, array_len) = *self.names.get(&name).ok_or_else(|| error(self.source, name_offset, &format!("unknown variable '{name}'")))?;
                    if let Some(len) = array_len {
                        let (line, col) = self.position(); self.take(Kind::LBracket)?;
                        let index = self.expression(0)?; self.take(Kind::RBracket)?;
                        Expr::Index(id, len, Box::new(index), line, col)
                    } else { Expr::Variable(id) }
                }
            }
            Kind::LParen => { self.pos += 1; let expr = self.expression(0)?; self.take(Kind::RParen)?; expr }
            _ => return Err(self.fail("expected expression")),
        };
        loop {
            let prec = match self.current() { Kind::Plus | Kind::Minus => 4, Kind::Star | Kind::Slash | Kind::Percent => 5, _ => break };
            if prec < min { break; }
            let op = self.current().clone();
            let (line, column) = self.position();
            self.pos += 1;
            let rhs = self.expression(prec + 1)?;
            lhs = Expr::Binary(Box::new(lhs), op, Box::new(rhs), line, column);
        }
        Ok(lhs)
    }
    // None denotes i64; Some(n) is a read-only borrowed fixed-size array.
    fn parameter_type(&mut self) -> Result<Option<usize>, String> {
        if *self.current() == Kind::Amp {
            self.pos += 1; self.take(Kind::LBracket)?;
            if self.identifier()? != "i64" { return Err(self.fail("array element type must be i64")); }
            self.take(Kind::Semi)?;
            let len = match self.current() {
                Kind::Number(n) if *n >= 1 && *n <= 4096 => *n as usize,
                _ => return Err(self.fail("array parameter length must be between 1 and 4096")),
            };
            self.pos += 1; self.take(Kind::RBracket)?;
            Ok(Some(len))
        } else {
            if self.identifier()? != "i64" { return Err(self.fail("parameter type must be i64 or &[i64; N]")); }
            Ok(None)
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
                if self.identifier()? != "i64" { return Err(self.fail("return type must be i64")); }
                self.functions.insert(name, (id, types));
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
            if is_main {
                if !params.is_empty() { return Err(self.fail("main cannot have parameters")); }
                main_found = true;
            } else {
                self.take(Kind::Arrow)?;
                if self.identifier()? != "i64" { return Err(self.fail("return type must be i64")); }
            }
            self.in_main = is_main;
            let body = self.block()?;
            if !is_main && !returns(&body) { return Err(self.fail("function may exit without returning i64")); }
            result.push(Function { id, params, body, is_main });
        }
        if !main_found { return Err(self.fail("missing main function")); }
        Ok(result)
    }
    fn condition(&mut self) -> Result<Condition, String> {
        let left = self.expression(0)?;
        let op = self.current().clone();
        if !matches!(op, Kind::EqEq | Kind::NotEq | Kind::Less | Kind::LessEq | Kind::Greater | Kind::GreaterEq) { return Err(self.fail("expected comparison operator")); }
        self.pos += 1;
        Ok(Condition(left, op, self.expression(0)?))
    }
    fn if_statement(&mut self) -> Result<Statement, String> {
        self.pos += 1;
        let condition = self.condition()?;
        let yes = self.block()?;
        let no = if *self.current() == Kind::Else {
            self.pos += 1;
            // `else if` chains recurse instead of nesting braces.
            if *self.current() == Kind::If { vec![self.if_statement()?] } else { self.block()? }
        } else { Vec::new() };
        Ok(Statement::If(condition, yes, no))
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
                    let (stmt, array_len) = if *self.current() == Kind::LBracket {
                        self.pos += 1;
                        let mut values = Vec::new();
                        if *self.current() == Kind::RBracket { return Err(self.fail("array must contain at least one element")); }
                        loop {
                            if values.len() == 4096 { return Err(self.fail("array element limit exceeded (4096)")); }
                            values.push(self.expression(0)?);
                            if *self.current() != Kind::Comma { break; }
                            self.pos += 1;
                        }
                        self.take(Kind::RBracket)?; self.take(Kind::Semi)?;
                        let len = values.len(); (Statement::Array(id, values), Some(len))
                    } else {
                        let value = self.expression(0)?; self.take(Kind::Semi)?;
                        (Statement::Declare(id, value), None)
                    };
                    self.names.insert(name.clone(), (id, mutable, array_len)); declared.push(name);
                    stmt
                }
                Kind::Ident(name) => {
                    let (id, mutable, array_len) = *self.names.get(&name).ok_or_else(|| self.fail(&format!("unknown variable '{name}'")))?;
                    if !mutable { return Err(self.fail(&format!("cannot assign to immutable variable '{name}'"))); }
                    self.pos += 1;
                    if let Some(len) = array_len {
                        let (line, col) = self.position(); self.take(Kind::LBracket)?;
                        let index = self.expression(0)?; self.take(Kind::RBracket)?; self.take(Kind::Equal)?;
                        let value = self.expression(0)?; self.take(Kind::Semi)?;
                        Statement::Store(id, len, index, value, line, col)
                    } else {
                        self.take(Kind::Equal)?; let value = self.expression(0)?; self.take(Kind::Semi)?;
                        Statement::Assign(id, value)
                    }
                }
                Kind::Return => {
                    if self.in_main { return Err(self.fail("return is not supported in main")); }
                    self.pos += 1; let value = self.expression(0)?; self.take(Kind::Semi)?;
                    Statement::Return(value)
                }
                Kind::If => self.if_statement()?,
                Kind::While => {
                    self.pos += 1; let condition = self.condition()?;
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
                Kind::Print => {
                    self.pos += 1; self.take(Kind::LParen)?;
                    let value = self.expression(0)?; self.take(Kind::RParen)?; self.take(Kind::Semi)?;
                    Statement::Print(value)
                }
                _ => return Err(self.fail("expected declaration, assignment, print, or '}'")),
            };
            result.push(stmt);
        }
        self.take(Kind::RBrace)?;
        for name in declared { self.names.remove(&name); }
        Ok(result)
    }
}

// Expressions are emitted as sequential temporaries, preserving left-to-right evaluation.
fn emit_expr(expr: &Expr, out: &mut String, next: &mut usize) -> usize {
    let expression = match expr {
        Expr::Number(n) => format!("INT64_C({n})"),
        Expr::Variable(id) => format!("v{id}"),
        Expr::Call(id, args) => {
            let arguments = args.iter().map(|arg| match arg {
                Argument::Value(e) => format!("t{}", emit_expr(e, out, next)),
                Argument::Borrow(id) => format!("v{id}"),
            }).collect::<Vec<_>>().join(", ");
            format!("ae_fn_{id}({arguments})")
        }
        Expr::Index(id, len, index, line, col) => {
            let i = emit_expr(index, out, next);
            format!("v{id}[ae_index(t{i}, {len}, {line}, {col})]")
        }
        Expr::Neg(a, line, column) => {
            let t = emit_expr(a, out, next);
            format!("ae_neg(t{t}, {line}, {column})")
        }
        Expr::Binary(a, op, b, line, column) => {
            let l = emit_expr(a, out, next); let r = emit_expr(b, out, next);
            let fun = match op { Kind::Plus => "add", Kind::Minus => "sub", Kind::Star => "mul", Kind::Slash => "div", Kind::Percent => "mod", _ => unreachable!() };
            format!("ae_{fun}(t{l}, t{r}, {line}, {column})")
        }
    };
    let id = *next; *next += 1;
    out.push_str(&format!("    int64_t t{id} = {expression};\n")); id
}
fn emit_condition(condition: &Condition, out: &mut String, next: &mut usize) -> String {
    let left = emit_expr(&condition.0, out, next);
    let right = emit_expr(&condition.2, out, next);
    let op = match condition.1 { Kind::EqEq => "==", Kind::NotEq => "!=", Kind::Less => "<", Kind::LessEq => "<=", Kind::Greater => ">", Kind::GreaterEq => ">=", _ => unreachable!() };
    format!("t{left} {op} t{right}")
}
fn emit_statements(stmts: &[Statement], out: &mut String, next: &mut usize) {
    for stmt in stmts {
        match stmt {
            Statement::Return(e) => { let t = emit_expr(e, out, next); out.push_str(&format!("    return t{t};\n")); }
            Statement::Declare(id, e) => { let t = emit_expr(e, out, next); out.push_str(&format!("    int64_t v{id} = t{t};\n")); }
            Statement::Array(id, values) => {
                let items: Vec<_> = values.iter().map(|e| emit_expr(e, out, next)).collect();
                out.push_str(&format!("    int64_t v{id}[{}] = {{", items.len()));
                for item in items { out.push_str(&format!("t{item},")); }
                out.push_str("};\n");
            }
            Statement::Store(id, len, index, value, line, col) => {
                let i = emit_expr(index, out, next);
                let checked = *next; *next += 1;
                out.push_str(&format!("    size_t t{checked} = ae_index(t{i}, {len}, {line}, {col});\n"));
                let value = emit_expr(value, out, next);
                out.push_str(&format!("    v{id}[t{checked}] = t{value};\n"));
            }
            Statement::Assign(id, e) => { let t = emit_expr(e, out, next); out.push_str(&format!("    v{id} = t{t};\n")); }
            Statement::Print(e) => { let t = emit_expr(e, out, next); out.push_str(&format!("    if (printf(\"%\" PRId64 \"\\n\", t{t}) < 0) ae_fail(\"output error\", 0, 0);\n")); }
            Statement::If(c, yes, no) => {
                let cond = emit_condition(c, out, next);
                out.push_str(&format!("    if ({cond}) {{\n")); emit_statements(yes, out, next);
                out.push_str("    } else {\n"); emit_statements(no, out, next); out.push_str("    }\n");
            }
            Statement::While(c, body) => {
                out.push_str("    while (1) {\n");
                let cond = emit_condition(c, out, next);
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
    let mut p = Parser { source, tokens: lex(source)?, pos: 0, names: HashMap::new(), next_var: 0, expression_depth: 0, expression_nodes: 0, block_depth: 0, loop_depth: 0, functions: HashMap::new(), in_main: false };
    p.collect_functions()?;
    p.program()
}
pub fn check(source: &str) -> Result<(), String> {
    parse(source).map(|_| ())
}
pub fn compile(source: &str) -> Result<String, String> {
    let functions = parse(source)?;
    let mut out = String::from(include_str!("runtime.c"));
    for function in &functions {
        if function.is_main { continue; }
        let params = if function.params.is_empty() { "void".to_owned() } else { function.params.iter().map(|(_, ty)| if ty.is_some() { "const int64_t *" } else { "int64_t" }).collect::<Vec<_>>().join(", ") };
        out.push_str(&format!("\nstatic int64_t ae_fn_{}({params});\n", function.id));
    }
    for function in &functions {
        if function.is_main { continue; }
        let params = if function.params.is_empty() { "void".to_owned() } else { function.params.iter().map(|(id, ty)| if ty.is_some() { format!("const int64_t *v{id}") } else { format!("int64_t v{id}") }).collect::<Vec<_>>().join(", ") };
        out.push_str(&format!("\nstatic int64_t ae_fn_{}({params}) {{\n", function.id));
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
    #[test] fn diagnostics_include_unicode_aware_position() {
        let e = compile("// 中文\nfn main() { print(@); }").unwrap_err();
        assert!(e.starts_with("2:19:"), "{e}");
    }
    #[test] fn malformed_inputs_do_not_panic() {
        // Deterministic corpus of arbitrary UTF-8 tokens and punctuation.
        let alphabet = ["a", "0", " ", "\n", "{", "}", "[", "]", "(", ")", ";", "+", "=", "/", "中", "🙂", "\0"];
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
