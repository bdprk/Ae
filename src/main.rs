use std::{env, ffi::{OsStr, OsString}, fs, path::{Path, PathBuf}, process::{Command, ExitCode}, time::Instant};

mod frontend;
mod sha256;
use frontend::compile_path;

const USAGE: &str = "usage: ae <check|emit-c> <source.ae>\n       ae run <source.ae> [--release]\n       ae build <source.ae> -o <output> [--release]\n       ae cache [dir|clear|stats]\n       ae version";

struct TempDir(PathBuf);
impl TempDir {
    fn create(parent: &Path) -> Result<Self, String> {
        for counter in 0..1000 {
            let path = parent.join(format!(".ae-{}-{counter}", std::process::id()));
            match fs::create_dir(&path) {
                Ok(()) => return Ok(Self(path)),
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(e) => return Err(format!("cannot create temporary directory in {}: {e}", parent.display())),
            }
        }
        Err("cannot allocate temporary directory".into())
    }
}
impl Drop for TempDir {
    fn drop(&mut self) { let _ = fs::remove_dir_all(&self.0); }
}

fn cache_root() -> PathBuf {
    if let Some(dir) = env::var_os("AE_CACHE") { return PathBuf::from(dir); }
    if cfg!(windows) {
        if let Some(dir) = env::var_os("LOCALAPPDATA") { return PathBuf::from(dir).join("ae").join("cache"); }
    } else {
        if let Some(dir) = env::var_os("XDG_CACHE_HOME") { return PathBuf::from(dir).join("ae"); }
        if let Some(dir) = env::var_os("HOME") { return PathBuf::from(dir).join(".cache").join("ae"); }
    }
    env::temp_dir().join("ae-cache")
}

/// First line of the C compiler's --version output identifies the toolchain
/// inside cache keys; an unqueryable compiler yields a stable placeholder.
fn cc_identity(cc: &OsStr) -> String {
    Command::new(cc).arg("--version")
        .output()
        .ok()
        .map(|out| String::from_utf8_lossy(&out.stdout).lines().next().unwrap_or("").trim().to_owned())
        .filter(|line| !line.is_empty())
        .unwrap_or_else(|| "unknown".into())
}

fn timings_enabled() -> bool { env::var_os("AE_TIMINGS").as_deref() == Some(std::ffi::OsStr::new("1")) }

/// Build the generated C. Results are cached under a key covering every
/// input that can change the executable: generated C text, optimization
/// mode, C compiler identity, compiler version, and target platform.
fn build_c(c: &str, dir: &Path, release: bool, use_cache: bool) -> Result<PathBuf, String> {
    let exe_name = if cfg!(windows) { "program.exe" } else { "program" };
    let cfile = dir.join("program.c");
    let exe = dir.join(exe_name);
    fs::write(&cfile, c).map_err(|e| format!("writing generated C: {e}"))?;
    let cc = env::var_os("AE_CC").unwrap_or_else(|| "gcc".into());
    let optimization = if release { "-O2" } else { "-O0" };

    let mut cache_entry: Option<PathBuf> = None;
    if use_cache {
        let key = sha256::hex([
            c.as_bytes(),
            optimization.as_bytes(),
            cc_identity(&cc).as_bytes(),
            env!("CARGO_PKG_VERSION").as_bytes(),
            if cfg!(windows) { &b"windows"[..] } else { &b"unix"[..] },
        ].concat().as_slice());
        let entry = cache_root().join("entries").join(key);
        let cached = entry.join(exe_name);
        if cached.is_file() && fs::metadata(&cached).map(|m| m.len() > 0).unwrap_or(false) {
            let started = Instant::now();
            fs::copy(&cached, &exe).map_err(|e| format!("copying cached executable: {e}"))?;
            if timings_enabled() {
                eprintln!("ae cache: hit");
                eprintln!("ae timing: c_compile_and_link_ms={:.3} (cache hit)", started.elapsed().as_secs_f64() * 1000.0);
            }
            return Ok(exe);
        }
        cache_entry = Some(entry);
    }

    let started = Instant::now();
    let status = Command::new(&cc).arg("-std=c11").arg(optimization).arg(&cfile).arg("-o").arg(&exe)
        .status().map_err(|e| format!("cannot start C compiler {:?}: {e}; set AE_CC to its executable", cc))?;
    if timings_enabled() {
        eprintln!("ae cache: miss");
        eprintln!("ae timing: c_compile_and_link_ms={:.3}", started.elapsed().as_secs_f64() * 1000.0);
    }
    if !status.success() { return Err(format!("C compiler failed: {status}")); }

    // Publishing to the cache is best effort: the rename makes a partial
    // entry impossible to mistake for a complete one.
    if let Some(entry) = cache_entry {
        let _ = fs::create_dir_all(&entry);
        let staging = entry.join(format!("{exe_name}.{}.tmp", std::process::id()));
        if fs::copy(&exe, &staging).is_ok() {
            let _ = fs::rename(&staging, entry.join(exe_name));
        }
        let _ = fs::remove_file(entry.join(format!("{exe_name}.{}.tmp", std::process::id())));
    }
    Ok(exe)
}

fn cache_command(subcommand: Option<&OsString>) -> Result<u8, String> {
    let entries = cache_root().join("entries");
    match subcommand.and_then(|s| s.to_str()) {
        None | Some("stats") => {
            let mut count = 0usize;
            let mut bytes = 0u64;
            if let Ok(read) = fs::read_dir(&entries) {
                for entry in read.flatten() {
                    if let Ok(meta) = fs::metadata(entry.path().join(if cfg!(windows) { "program.exe" } else { "program" })) {
                        count += 1;
                        bytes += meta.len();
                    }
                }
            }
            println!("entries: {count}");
            println!("bytes: {bytes}");
            println!("dir: {}", entries.display());
            Ok(0)
        }
        Some("dir") => { println!("{}", entries.display()); Ok(0) }
        Some("clear") => {
            match fs::remove_dir_all(&entries) {
                Ok(()) => println!("cleared {}", entries.display()),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => println!("cache is already empty"),
                Err(e) => return Err(format!("clearing cache: {e}")),
            }
            Ok(0)
        }
        Some(other) => Err(format!("unknown cache subcommand '{other}'; expected dir, clear, or stats\n{USAGE}")),
    }
}

fn run(args: Vec<OsString>) -> Result<u8, String> {
    if args.is_empty() { return Err(USAGE.into()); }
    let cmd = args[0].to_str().ok_or("command must be UTF-8")?.to_owned();
    if cmd == "version" {
        if args.len() != 1 { return Err(USAGE.into()); }
        println!("ae {} ({}, {})", env!("CARGO_PKG_VERSION"),
            if cfg!(windows) { "windows" } else { "unix" },
            if cfg!(target_arch = "x86_64") { "x86-64" } else { "other-arch" });
        return Ok(0);
    }
    if cmd == "cache" { return cache_command(args.get(1)); }
    if args.len() < 2 { return Err(USAGE.into()); }
    let release = match cmd.as_str() {
        "run" if args.len() == 3 && args[2] == "--release" => true,
        "build" if args.len() == 5 && args[4] == "--release" => true,
        _ => false,
    };
    let output = match cmd.as_str() {
        "check" | "emit-c" | "run" if args.len() == 2 => None,
        "run" if release => None,
        "build" if (args.len() == 4 || release) && args[2] == "-o" => Some(PathBuf::from(&args[3])),
        "check" | "emit-c" | "run" | "build" => return Err(USAGE.into()),
        _ => return Err(format!("unknown command: {cmd}\n{USAGE}")),
    };
    let input = Path::new(&args[1]);
    let started = Instant::now();
    if cmd == "check" {
        frontend::check_path(input)?;
        if timings_enabled() {
            eprintln!("ae timing: frontend_ms={:.3}", started.elapsed().as_secs_f64() * 1000.0);
        }
        return Ok(0);
    }
    let c = compile_path(input)?;
    if timings_enabled() {
        eprintln!("ae timing: frontend_and_codegen_ms={:.3}", started.elapsed().as_secs_f64() * 1000.0);
    }
    match cmd.as_str() {
        "emit-c" => { print!("{c}"); Ok(0) },
        "run" => {
            let dir = TempDir::create(&env::temp_dir())?;
            let exe = build_c(&c, &dir.0, release, true)?;
            let status = Command::new(&exe).status().map_err(|e| format!("cannot start program: {e}"))?;
            Ok(status.code().and_then(|n| u8::try_from(n).ok()).unwrap_or(1))
        }
        "build" => {
            let output = output.unwrap();
            // A build never overwrites user files, including the input. The final copy uses
            // create_new as a second check so an intervening file is not overwritten either.
            let parent = output.parent().filter(|p| !p.as_os_str().is_empty()).unwrap_or(Path::new("."));
            let dir = TempDir::create(parent)?;
            let exe = build_c(&c, &dir.0, release, true)?;
            let mut src = fs::File::open(&exe).map_err(|e| format!("opening generated executable: {e}"))?;
            let mut dest = fs::OpenOptions::new().write(true).create_new(true).open(&output)
                .map_err(|e| format!("cannot create {} (existing files are not overwritten): {e}", output.display()))?;
            if let Err(e) = std::io::copy(&mut src, &mut dest) {
                drop(dest); let _ = fs::remove_file(&output);
                return Err(format!("writing {}: {e}", output.display()));
            }
            drop(dest);
            #[cfg(unix)] {
                use std::os::unix::fs::PermissionsExt;
                if let Err(e) = fs::set_permissions(&output, fs::Permissions::from_mode(0o755)) {
                    let _ = fs::remove_file(&output);
                    return Err(format!("setting executable permissions on {}: {e}", output.display()));
                }
            }
            Ok(0)
        }
        _ => unreachable!(),
    }
}

fn main() -> ExitCode {
    // Parsing, checking, and dropping deep ASTs recurse; give that work a
    // dedicated large stack instead of relying on the OS default for the
    // main thread. Nesting limits still bound recursion explicitly.
    let worker = std::thread::Builder::new()
        .stack_size(256 * 1024 * 1024)
        .spawn(|| run(env::args_os().skip(1).collect()));
    let result = match worker {
        Ok(worker) => match worker.join() {
            Ok(code) => code,
            Err(_) => return ExitCode::FAILURE,
        },
        Err(e) => Err(format!("cannot start compiler thread: {e}")),
    };
    match result {
        Ok(code) => ExitCode::from(code),
        Err(e) => { eprintln!("ae: {e}"); ExitCode::FAILURE }
    }
}
