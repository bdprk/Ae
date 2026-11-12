use super::*;

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
pub(super) fn parse_source(file: &str, source: &str, shared: &mut SharedFunctions, structs: &mut Vec<StructDef>, struct_names: &mut HashMap<String, usize>, enums: &mut Vec<EnumDef>, enum_names: &mut HashMap<String, usize>, generics: &mut Generics, next_id: &mut usize) -> Result<Vec<Function>, String> {
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
                    let mut replay_tokens = Rc::clone(&tokens);
                    loop {
                        let (mut functions, mut structs, mut struct_names, mut enums, mut enum_names, mut generics) =
                            (self.functions.clone(), self.structs.clone(), self.struct_names.clone(), self.enums.clone(), self.enum_names.clone(), self.generics.clone());
                        let mut parser = make_parser(file, source, Rc::clone(&replay_tokens), &mut functions, &mut structs, &mut struct_names, &mut enums, &mut enum_names, &mut generics);
                        parser.pos = index + 2;
                        let diagnostic = match parser.function_rest(depth == 0 && name == "main", id) {
                            Ok(_) => break,
                            Err(diagnostic) => diagnostic,
                        };
                        let replacement = independent_unknown_expression(&replay_tokens, &diagnostic);
                        if !result.contains(&diagnostic) { result.push(diagnostic); }
                        if result.len() == 32 { break; }
                        let Some((replacement, literal)) = replacement else { break };
                        Rc::make_mut(&mut replay_tokens)[replacement].kind = literal;
                    }
                    if result.len() == 32 { break; }
                }
                _ => {}
            }
        }
        if id_index != self.ids.len() && result.len() < 32 { return Vec::new(); }
        result
    }
}

// Replaying these bare unknown expressions with a literal adds no bindings,
// moves, or borrows. The parser checks both branches regardless of the literal.
// Complex expressions and diagnostics without exact token spans are not altered.
fn independent_unknown_expression(tokens: &[Token], diagnostic: &ae_span::Diagnostic) -> Option<(usize, Kind)> {
    if diagnostic.code.as_deref() != Some("E0101") { return None; }
    let range = &diagnostic.primary.as_ref()?.span.range;
    let index = tokens.iter().position(|token| token.offset == range.start && token.end == range.end)?;
    if !matches!(tokens[index].kind, Kind::Ident(_)) { return None; }
    if index >= 2 && matches!(tokens[index - 1].kind, Kind::LParen)
        && matches!(tokens.get(index + 1).map(|token| &token.kind), Some(Kind::RParen))
        && matches!(tokens.get(index + 2).map(|token| &token.kind), Some(Kind::Semi)) {
        return match tokens[index - 2].kind {
            Kind::Print => Some((index, Kind::Number(0))),
            Kind::Assert => Some((index, Kind::True)),
            _ => None,
        };
    }
    if index >= 1 && matches!(tokens[index - 1].kind, Kind::If | Kind::While)
        && matches!(tokens.get(index + 1).map(|token| &token.kind), Some(Kind::LBrace)) {
        return Some((index, Kind::True));
    }
    None
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

pub(super) fn load_program(entry: &Path) -> Result<Program, String> {
    load_program_full(entry, false).map(|loaded| loaded.program).map_err(Into::into)
}

pub fn check_path(path: &Path) -> Result<(), String> {
    load_program(path).map(|_| ())
}

// A lexically invalid file can still expose imports that precede the first
// error. Only use a complete prefix that independently lexes and whose use
// headers parse successfully. This frame is used solely for diagnostics.
fn verified_import_prefix(path: &Path, errors: &[ae_span::Diagnostic]) -> Option<LoadingFile> {
    let source = std::fs::read_to_string(path).ok()?;
    let file = path.display().to_string();
    let mut current = ae_syntax::lex_with_diagnostics(&source).err()?;
    for diagnostic in &mut current {
        if let Some(label) = &mut diagnostic.primary { label.span.file = Some(file.clone()); }
    }
    if current != errors { return None; }
    let first_offset = errors.first()?.primary.as_ref()?.span.range.start;
    let prefix_end = source.get(..first_offset)?.rfind(';')? + 1;
    let prefix = source.get(..prefix_end)?;
    let tokens = Rc::new(lex(prefix).ok()?);
    let uses = scan_uses(&file, prefix, tokens.as_ref()).ok()?;
    if uses.is_empty() { return None; }
    Some(LoadingFile { canonical: std::fs::canonicalize(path).ok()?, path: path.to_owned(), source, file, tokens, uses, next_import: 0 })
}

// Read-only supplemental traversal after an initial lexical error. A failed
// file may expose verified imports before its first error; siblings can still be scanned.
fn collect_import_lex_errors(entry: &Path) -> Vec<ae_span::Diagnostic> {
    const LIMIT: usize = 32;
    let mut visited = HashMap::new();
    let mut diagnostics = Vec::new();
    let mut stack = match begin_file(entry, &mut visited) {
        Ok(Some(frame)) => vec![frame],
        Err(FrontendError::LexDiagnostics(errors)) => {
            let frame = verified_import_prefix(entry, &errors);
            diagnostics.extend(errors);
            match frame { Some(frame) if diagnostics.len() < LIMIT => vec![frame], _ => return diagnostics }
        }
        _ => return diagnostics,
    };
    while let Some(frame) = stack.last_mut() {
        if let Some(import) = frame.uses.get(frame.next_import) {
            frame.next_import += 1;
            let child = frame.path.parent().unwrap_or(Path::new(".")).join(import);
            let canonical = std::fs::canonicalize(&child).ok();
            let was_visited = canonical.as_ref().is_some_and(|path| visited.contains_key(path));
            match begin_file(&child, &mut visited) {
                Ok(Some(next)) => stack.push(next),
                Ok(None) => {},
                Err(error) => {
                    let partial = match &error {
                        FrontendError::LexDiagnostics(errors) => verified_import_prefix(&child, errors),
                        _ => None,
                    };
                    let errors = match error {
                        FrontendError::LexDiagnostics(errors) | FrontendError::Diagnostics(errors) => errors,
                        FrontendError::Diagnostic(error) => vec![error],
                        FrontendError::Legacy(message) => vec![ae_span::Diagnostic {
                            code: None, message, primary: None, secondary: Vec::new(), suggestions: Vec::new(),
                        }],
                    };
                    diagnostics.extend(errors.into_iter().take(LIMIT - diagnostics.len()));
                    if diagnostics.len() == LIMIT { break; }
                    if let Some(frame) = partial {
                        stack.push(frame);
                        continue;
                    }
                    // Only mark a newly encountered failed file. A cycle points to
                    // an active frame and must not change that frame's state.
                    if !was_visited {
                        if let Some(canonical) = canonical { visited.insert(canonical, ModuleState::Loaded); }
                    }
                }
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
