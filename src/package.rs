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

pub(super) fn entry_path(input: &Path) -> Result<PathBuf, String> {
    let Some(manifest) = manifest_path(input) else {
        return Ok(input.to_owned());
    };
    let package = read_manifest(&manifest)?;
    if !package.dependencies.is_empty() {
        return Err(format!("{}: package dependency resolution is not implemented yet for compilation", manifest.display()));
    }
    Ok(manifest.with_file_name("src").join("main.ae"))
}

struct Manifest {
    name: String,
    version: String,
    dependencies: toml::Table,
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

    if document.contains_key("target") {
        return Err(fail("target-specific dependencies are not implemented yet"));
    }
    let name = name.to_owned();
    let version = version.to_owned();
    let dependencies = match document.remove("dependencies") {
        None => toml::Table::new(),
        Some(toml::Value::Table(table)) => table,
        Some(_) => return Err(fail("expected a [dependencies] table")),
    };
    Ok(Manifest { name, version, dependencies })
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
        for (alias, dependency) in package.dependencies {
            let relative = dependency.as_table().filter(|table| table.len() == 1)
                .and_then(|table| table.get("path")).and_then(toml::Value::as_str);
            let relative = relative.filter(|value| !value.is_empty()).ok_or_else(|| {
                format!("{}: dependency '{alias}' requires a nonempty path table", path.display())
            })?;
            let source = Path::new(relative);
            if source.has_root() || matches!(source.components().next(), Some(std::path::Component::Prefix(_))) {
                return Err(format!("{}: dependency '{alias}' path must be relative to its manifest", path.display()));
            }
            let dependency = path.parent().expect("a canonical file has a parent").join(source).join(MANIFEST);
            self.visit(&dependency)?;
            edges.push((alias, relative.to_owned()));
        }
        self.visiting.pop();
        self.visited.insert(path.clone());
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
