use std::{env, ffi::OsString, fs, path::{Path, PathBuf}, process::{Command, ExitCode}, time::Instant};

mod frontend;
use frontend::compile;

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

fn build_c(c: &str, dir: &Path, release: bool) -> Result<PathBuf, String> {
    let cfile = dir.join("program.c");
    let exe = dir.join(if cfg!(windows) { "program.exe" } else { "program" });
    fs::write(&cfile, c).map_err(|e| format!("writing generated C: {e}"))?;
    let cc = env::var_os("AE_CC").unwrap_or_else(|| "gcc".into());
    let optimization = if release { "-O2" } else { "-O0" };
    let started = Instant::now();
    let status = Command::new(&cc).arg("-std=c11").arg(optimization).arg(&cfile).arg("-o").arg(&exe)
        .status().map_err(|e| format!("cannot start C compiler {:?}: {e}; set AE_CC to its executable", cc))?;
    if env::var_os("AE_TIMINGS").as_deref() == Some(std::ffi::OsStr::new("1")) {
        eprintln!("ae timing: c_compile_and_link_ms={:.3}", started.elapsed().as_secs_f64() * 1000.0);
    }
    if !status.success() { return Err(format!("C compiler failed: {status}")); }
    Ok(exe)
}

fn run(args: Vec<OsString>) -> Result<u8, String> {
    let usage = "usage: ae <check|emit-c> <source.ae>\n       ae run <source.ae> [--release]\n       ae build <source.ae> -o <output> [--release]";
    if args.len() < 2 { return Err(usage.into()); }
    let cmd = args[0].to_str().ok_or("command must be UTF-8")?;
    let release = match cmd {
        "run" if args.len() == 3 && args[2] == "--release" => true,
        "build" if args.len() == 5 && args[4] == "--release" => true,
        _ => false,
    };
    let output = match cmd {
        "check" | "emit-c" | "run" if args.len() == 2 => None,
        "run" if release => None,
        "build" if (args.len() == 4 || release) && args[2] == "-o" => Some(PathBuf::from(&args[3])),
        "check" | "emit-c" | "run" | "build" => return Err(usage.into()),
        _ => return Err(format!("unknown command: {cmd}\n{usage}")),
    };
    let input = Path::new(&args[1]);
    let src = fs::read_to_string(input).map_err(|e| format!("{}: {e}", input.display()))?;
    let started = Instant::now();
    if cmd == "check" {
        frontend::check(&src).map_err(|e| format!("{}:{e}", input.display()))?;
        if env::var_os("AE_TIMINGS").as_deref() == Some(std::ffi::OsStr::new("1")) {
            eprintln!("ae timing: frontend_ms={:.3}", started.elapsed().as_secs_f64() * 1000.0);
        }
        return Ok(0);
    }
    let c = compile(&src).map_err(|e| format!("{}:{e}", input.display()))?;
    if env::var_os("AE_TIMINGS").as_deref() == Some(std::ffi::OsStr::new("1")) {
        eprintln!("ae timing: frontend_and_codegen_ms={:.3}", started.elapsed().as_secs_f64() * 1000.0);
    }
    match cmd {
        "emit-c" => { print!("{c}"); Ok(0) },
        "run" => {
            let dir = TempDir::create(&env::temp_dir())?;
            let exe = build_c(&c, &dir.0, release)?;
            let status = Command::new(&exe).status().map_err(|e| format!("cannot start program: {e}"))?;
            Ok(status.code().and_then(|n| u8::try_from(n).ok()).unwrap_or(1))
        }
        "build" => {
            let output = output.unwrap();
            // A build never overwrites user files, including the input. The final copy uses
            // create_new as a second check so an intervening file is not overwritten either.
            let parent = output.parent().filter(|p| !p.as_os_str().is_empty()).unwrap_or(Path::new("."));
            let dir = TempDir::create(parent)?;
            let exe = build_c(&c, &dir.0, release)?;
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
    match run(env::args_os().skip(1).collect()) {
        Ok(code) => ExitCode::from(code),
        Err(e) => { eprintln!("ae: {e}"); ExitCode::FAILURE }
    }
}
