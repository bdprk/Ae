use std::fs;
use std::path::{Path, PathBuf};

const MANIFEST: &str = "ae.toml";

pub(super) fn entry_path(input: &Path) -> Result<PathBuf, String> {
    let manifest = if input.is_dir() {
        input.join(MANIFEST)
    } else if input.file_name().is_some_and(|name| name == MANIFEST) {
        input.to_owned()
    } else {
        return Ok(input.to_owned());
    };

    let fail = |message: &str| format!("{}: {message}", manifest.display());
    let source = fs::read_to_string(&manifest).map_err(|error| fail(&error.to_string()))?;
    let document = source.parse::<toml::Table>().map_err(|error| fail(&error.to_string()))?;
    let package = document.get("package").and_then(toml::Value::as_table)
        .ok_or_else(|| fail("expected a [package] table"))?;
    let field = |key| package.get(key).and_then(toml::Value::as_str)
        .ok_or_else(|| fail(&format!("package.{key} must be a string")));

    let name = field("name")?;
    if name.is_empty() || name.len() > 64 || !name.bytes().all(|byte| {
        byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'-' | b'_')
    }) {
        return Err(fail("package.name must have 1 to 64 lowercase letters, digits, underscores or hyphens"));
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
    if let Some(dependencies) = document.get("dependencies") {
        let dependencies = dependencies.as_table().ok_or_else(|| fail("expected a [dependencies] table"))?;
        if !dependencies.is_empty() {
            return Err(fail("package dependency resolution is not implemented yet"));
        }
    }

    Ok(manifest.with_file_name("src").join("main.ae"))
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
