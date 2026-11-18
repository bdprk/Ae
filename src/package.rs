use std::fs;
use std::path::{Path, PathBuf};

const MANIFEST: &str = "ae.toml";

const PACKAGE_NAME_ERROR: &str = "package.name must have 1 to 64 lowercase letters, digits, underscores or hyphens";

fn valid_name(name: &str) -> bool {
    !name.is_empty() && name.len() <= 64 && name.bytes().all(|byte| {
        byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'-' | b'_')
    })
}

pub(super) fn create_project(directory: &Path) -> Result<(), String> {
    let name = directory.file_name().and_then(|name| name.to_str())
        .filter(|name| valid_name(name)).ok_or(PACKAGE_NAME_ERROR)?;
    let manifest = format!(
        "[package]\nname = \"{name}\"\nversion = \"0.1.0\"\nae = \"{}\"\n",
        env!("CARGO_PKG_VERSION"),
    );

    fn create_file(path: &Path, content: &str) -> Result<(), String> {
        use std::io::Write;
        let mut file = fs::File::create_new(path).map_err(|error| format!("{}: {error}", path.display()))?;
        file.write_all(content.as_bytes()).map_err(|error| format!("{}: {error}", path.display()))
    }

    fs::create_dir(directory).map_err(|error| format!("{}: {error}", directory.display()))?;
    let source = directory.join("src");
    fs::create_dir(&source).map_err(|error| format!("{}: {error}", source.display()))?;
    create_file(&source.join("main.ae"), "fn main() {\n    print(\"Hello, AE!\");\n}\n")?;
    create_file(&directory.join(MANIFEST), &manifest)
}

pub(super) fn discover_manifest() -> Result<PathBuf, String> {
    let current = std::env::current_dir().map_err(|error| format!("cannot read current directory: {error}"))?;
    for directory in current.ancestors() {
        let manifest = directory.join(MANIFEST);
        if manifest.try_exists().map_err(|error| format!("{}: {error}", manifest.display()))? {
            return Ok(manifest);
        }
    }
    Err(format!("no {MANIFEST} found in {} or its parent directories", current.display()))
}

pub(super) fn manifest_path(input: &Path) -> Option<PathBuf> {
    if input.is_dir() {
        Some(input.join(MANIFEST))
    } else if input.file_name().is_some_and(|name| name == MANIFEST) {
        Some(input.to_owned())
    } else {
        None
    }
}

pub(super) struct PackageEntry {
    pub path: PathBuf,
    pub imports: ae_compiler::frontend::PackageImports,
    pub lock: Option<PackageLock>,
}

pub(super) fn entry_path(input: &Path) -> Result<PackageEntry, String> {
    let Some(manifest) = manifest_path(input) else {
        return Ok(PackageEntry { path: input.to_owned(), imports: Default::default(), lock: None });
    };
    let package = read_manifest(&manifest)?;
    let mut graph = PathGraph::default();
    let lock_path = manifest.with_file_name("ae.lock");
    let has_lock = lock_path.try_exists().map_err(|error| format!("{}: {error}", lock_path.display()))?;
    let lock = if package.dependencies.is_empty() && !has_lock {
        None
    } else {
        graph.visit(&manifest)?;
        Some(PackageLock { path: lock_path, manifests: graph.manifests })
    };
    Ok(PackageEntry { path: manifest.with_file_name("src").join("main.ae"), imports: graph.imports, lock })
}

pub(super) struct PackageLock {
    path: PathBuf,
    manifests: Vec<(PathBuf, String)>,
}

impl PackageLock {
    fn render(&self, units: &[ae_compiler::frontend::SourceUnit]) -> Result<String, String> {
        use std::fmt::Write;
        let base = self.path.parent().filter(|path| !path.as_os_str().is_empty()).unwrap_or(Path::new("."));
        let base = base.canonicalize().map_err(|error| format!("{}: {error}", base.display()))?;
        let local = |path: &Path| -> Result<String, String> {
            let from: Vec<_> = base.components().collect();
            let to: Vec<_> = path.components().collect();
            let shared = from.iter().zip(&to).take_while(|(a, b)| a == b).count();
            if shared == 0 {
                return Err(format!("cannot lock a file on another filesystem root: {}", path.display()));
            }
            let mut parts = vec![".."; from.len() - shared];
            for component in &to[shared..] {
                parts.push(component.as_os_str().to_str().ok_or_else(|| format!(
                    "cannot lock a non-UTF-8 path: {}", path.display()
                ))?);
            }
            Ok(parts.join("/"))
        };

        let mut text = String::from("ae-lock-v1\n");
        for (path, digest) in &self.manifests {
            writeln!(text, "manifest {digest} {:?}", local(path)?).unwrap();
        }
        let mut sources = Vec::new();
        for unit in units {
            let path = unit.path.canonicalize().map_err(|error| format!("{}: {error}", unit.path.display()))?;
            sources.push((local(&path)?, &unit.digest));
        }
        sources.sort();
        for (path, digest) in sources {
            writeln!(text, "source {digest} {path:?}").unwrap();
        }
        Ok(text)
    }

    pub fn verify(&self, units: &[ae_compiler::frontend::SourceUnit]) -> Result<(), String> {
        let recorded = fs::read_to_string(&self.path)
            .map_err(|error| format!("{}: {error}. Run ae update", self.path.display()))?;
        let actual = self.render(units)?;
        if recorded.lines().eq(actual.lines()) {
            return Ok(());
        }
        let mut locked_lines = recorded.lines();
        let mut actual_lines = actual.lines();
        let (expected, found) = loop {
            let locked = locked_lines.next();
            let observed = actual_lines.next();
            if locked != observed {
                break (locked.unwrap_or("<missing entry>"), observed.unwrap_or("<missing entry>"));
            }
        };
        Err(format!("{} is stale. Run ae update\n  locked: {expected}\n  actual: {found}", self.path.display()))
    }
}

pub(super) fn update_lock(manifest: &Path) -> Result<(), String> {
    let PackageEntry { path, imports, lock } = entry_path(manifest)?;
    let lock = match lock {
        Some(lock) => lock,
        None => {
            let canonical = manifest.canonicalize().map_err(|error| format!("{}: {error}", manifest.display()))?;
            let package = read_manifest(&canonical)?;
            PackageLock { path: manifest.with_file_name("ae.lock"), manifests: vec![(canonical, package.digest)] }
        }
    };
    let tracked = ae_compiler::frontend::with_package_imports(imports, || {
        ae_compiler::frontend::compile_path_tracked(&path)
    })?;
    let contents = lock.render(&tracked.units)?;
    let parent = lock.path.parent().filter(|path| !path.as_os_str().is_empty()).unwrap_or(Path::new("."));
    let temporary = super::TempDir::create(parent)?;
    let staged = temporary.0.join("ae.lock");
    fs::write(&staged, contents).map_err(|error| format!("{}: {error}", staged.display()))?;
    fs::rename(&staged, &lock.path).map_err(|error| format!("{}: {error}", lock.path.display()))
}

struct Manifest {
    name: String,
    version: String,
    dependencies: toml::Table,
    digest: String,
}

fn read_manifest(manifest: &Path) -> Result<Manifest, String> {
    let fail = |message: &str| format!("{}: {message}", manifest.display());
    let source = fs::read_to_string(&manifest).map_err(|error| fail(&error.to_string()))?;
    let mut document = source.parse::<toml::Table>().map_err(|error| fail(&error.to_string()))?;
    let package = document.get("package").and_then(toml::Value::as_table)
        .ok_or_else(|| fail("expected a [package] table"))?;
    let field = |key| package.get(key).and_then(toml::Value::as_str)
        .ok_or_else(|| fail(&format!("package.{key} must be a string")));

    let name = field("name")?;
    if !valid_name(name) {
        return Err(fail(PACKAGE_NAME_ERROR));
    }

    let version = field("version")?;
    if parse_version(version, false).is_none() {
        return Err(fail("package.version must be a three-component version such as 0.1.0"));
    }
    let floor = field("ae")?;
    let minimum = parse_version(floor, true).ok_or_else(|| {
        fail("package.ae must be a toolchain version such as 0.1 or 0.1.1")
    })?;
    let current = env!("CARGO_PKG_VERSION");
    if minimum > parse_version(current, false).expect("the compiler has a numeric release version") {
        return Err(fail(&format!("package requires AE {floor} or later, installed version is {current}")));
    }

    let name = name.to_owned();
    let version = version.to_owned();
    let mut dependencies = match document.remove("dependencies") {
        None => toml::Table::new(),
        Some(toml::Value::Table(table)) => table,
        Some(_) => return Err(fail("expected a [dependencies] table")),
    };
    if let Some(targets) = document.remove("target") {
        let targets = targets.as_table().ok_or_else(|| fail("expected a [target] table"))?;
        for (selector, value) in targets {
            let condition = selector.strip_prefix("cfg(").and_then(|prefix| prefix.strip_suffix(')'))
                .and_then(platform_matches)
                .ok_or_else(|| fail(&format!("unsupported target selector '{selector}'")))?;
            let table = value.as_table().filter(|table| table.len() == 1)
                .and_then(|table| table.get("dependencies")).and_then(toml::Value::as_table)
                .ok_or_else(|| fail(&format!("target '{selector}' requires a dependencies table")))?;
            if condition {
                for (alias, dependency) in table {
                    if let Some(previous) = dependencies.get(alias) {
                        if previous != dependency {
                            return Err(fail(&format!("multiple active declarations for dependency '{alias}'")));
                        }
                    } else {
                        dependencies.insert(alias.clone(), dependency.clone());
                    }
                }
            }
        }
    }
    Ok(Manifest { name, version, dependencies, digest: ae_compiler::sha256::hex(source.as_bytes()) })
}

fn platform_matches(platform: &str) -> Option<bool> {
    Some(match platform {
        "windows" => cfg!(windows),
        "unix" => cfg!(unix),
        "x86_64" => cfg!(target_arch = "x86_64"),
        "arm64" => cfg!(target_arch = "aarch64"),
        _ => return None,
    })
}

pub(super) fn describe_graph(root: &Path) -> Result<String, String> {
    let mut graph = PathGraph::default();
    graph.visit(root)?;
    Ok(graph.output)
}

#[derive(Default)]
struct PathGraph {
    visiting: Vec<PathBuf>,
    visited: std::collections::HashSet<PathBuf>,
    output: String,
    imports: ae_compiler::frontend::PackageImports,
    manifests: Vec<(PathBuf, String)>,
}

impl PathGraph {
    fn visit(&mut self, manifest: &Path) -> Result<(), String> {
        use std::fmt::Write;
        let path = manifest.canonicalize().map_err(|error| format!("{}: {error}", manifest.display()))?;
        if let Some(start) = self.visiting.iter().position(|ancestor| ancestor == &path) {
            let cycle = self.visiting[start..].iter().chain(std::iter::once(&path))
                .map(|member| member.display().to_string()).collect::<Vec<_>>().join(" -> ");
            return Err(format!("package dependency cycle: {cycle}"));
        }
        if self.visited.contains(&path) {
            return Ok(());
        }
        if self.visiting.len() == 128 {
            return Err(format!("{}: package dependency nesting exceeds 128 levels", manifest.display()));
        }

        let package = read_manifest(&path)?;
        self.visiting.push(path.clone());
        let mut edges = Vec::new();
        let mut imports = std::collections::BTreeMap::new();
        for (alias, dependency) in package.dependencies {
            let relative = dependency.as_table().filter(|table| table.len() == 1)
                .and_then(|table| table.get("path")).and_then(toml::Value::as_str);
            let relative = relative.filter(|value| !value.is_empty()).ok_or_else(|| {
                format!("{}: dependency resolution for '{alias}' requires a nonempty path table", path.display())
            })?;
            let source = Path::new(relative);
            if source.has_root() || matches!(source.components().next(), Some(std::path::Component::Prefix(_))) {
                return Err(format!("{}: dependency '{alias}' path must be relative to its manifest", path.display()));
            }
            let dependency = path.parent().expect("a canonical file has a parent").join(source).join(MANIFEST);
            self.visit(&dependency)?;
            let canonical = dependency.canonicalize().map_err(|error| format!("{}: {error}", dependency.display()))?;
            imports.insert(alias.clone(), canonical.with_file_name("src").join("lib.ae"));
            edges.push((alias, relative.to_owned()));
        }
        self.visiting.pop();
        self.imports.insert(path.with_file_name("src"), imports);
        self.visited.insert(path.clone());
        self.manifests.push((path.clone(), package.digest));
        writeln!(self.output, "{} {} ({})", package.name, package.version, path.display()).unwrap();
        for (alias, relative) in edges {
            writeln!(self.output, "  {alias} -> {relative}").unwrap();
        }
        Ok(())
    }
}

fn parse_version(value: &str, allow_missing_patch: bool) -> Option<(u64, u64, u64)> {
    fn component(text: &str) -> Option<u64> {
        if text.is_empty() || !text.bytes().all(|byte| byte.is_ascii_digit())
            || (text.len() > 1 && text.starts_with('0'))
        {
            return None;
        }
        text.parse().ok()
    }

    let mut parts = value.split('.');
    let major = component(parts.next()?)?;
    let minor = component(parts.next()?)?;
    let patch = match parts.next() {
        Some(text) => component(text)?,
        None if allow_missing_patch => 0,
        None => return None,
    };
    (parts.next().is_none()).then_some((major, minor, patch))
}
