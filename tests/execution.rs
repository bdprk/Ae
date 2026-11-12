use std::{fs, process::Command, sync::atomic::{AtomicUsize, Ordering}};
static NEXT: AtomicUsize = AtomicUsize::new(0);
fn execute(source: &str) -> std::process::Output {
    let path = std::env::temp_dir().join(format!("ae-integration-{}-{}.ae", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed)));
    fs::write(&path, source).unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_ae")).arg("run").arg(&path).output().unwrap();
    fs::remove_file(path).unwrap();
    output
}
#[test]
fn values_and_precedence_execute() {
    let output = execute("fn main() { let x = 2; var y = x + 3 * 4; y = y / 2; print(y); print(0 - 5); }");
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    assert_eq!(String::from_utf8_lossy(&output.stdout).replace("\r\n", "\n"), "7\n-5\n");
}
#[test]
fn arithmetic_failures_are_reported() {
    for (expr, expected) in [("9223372036854775807 + 1", "integer overflow"), ("1 / 0", "division by zero"), ("3037000500 * 3037000500", "integer overflow"), ("(0 - 9223372036854775807 - 1) / (0 - 1)", "integer overflow")] {
        let output = execute(&format!("fn main() {{ print({expr}); }}"));
        assert!(!output.status.success(), "{expr}");
        assert!(String::from_utf8_lossy(&output.stderr).contains(expected), "{expr}: {}", String::from_utf8_lossy(&output.stderr));
    }
}
#[test]
fn build_accepts_non_ascii_absolute_output_paths() {
    // The compiler used to pass the temporary directory to gcc as an absolute
    // path. On Windows the MinGW toolchain converts the command line to the ANSI
    // code page, so a path like `...\用户\...` reached the linker mangled and
    // the build failed with "cannot open output file" — invisible whenever the
    // cache answered first, because then gcc never ran. The temporary files are
    // now named relative to the compiler's working directory.
    let dir = unique_temp_dir("nonascii");
    let cjk = dir.join("中文目录");
    fs::create_dir(&cjk).unwrap();
    let source = dir.join("hello.ae");
    fs::write(&source, "fn main() { print(42); }\n").unwrap();
    let cache = dir.join("cache");
    fs::create_dir(&cache).unwrap();
    let output = cjk.join(if cfg!(windows) { "程序.exe" } else { "程序" });
    let out = Command::new(env!("CARGO_BIN_EXE_ae"))
        .arg("build").arg(&source).arg("-o").arg(&output)
        .env("AE_CACHE", &cache)
        .output().unwrap();
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    assert!(output.is_file(), "missing {}", output.display());
    let run = Command::new(&output).output().unwrap();
    assert_eq!(String::from_utf8_lossy(&run.stdout).replace("\r\n", "\n").trim(), "42");
    let _ = fs::remove_dir_all(&dir);
}
#[test]
fn build_creates_executable_and_refuses_to_overwrite() {
    let dir = std::env::temp_dir().join(format!("ae-build-test-{}-{}", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed)));
    fs::create_dir(&dir).unwrap();
    let input = dir.join("程序.ae");
    let executable = dir.join(if cfg!(windows) { "result.exe" } else { "result" });
    fs::write(&input, "fn main() { print(123); }").unwrap();
    let build = || Command::new(env!("CARGO_BIN_EXE_ae")).arg("build").arg(&input).arg("-o").arg(&executable).output().unwrap();
    let result = build();
    assert!(result.status.success(), "{}", String::from_utf8_lossy(&result.stderr));
    let output = Command::new(&executable).output().unwrap();
    assert!(output.status.success());
    assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), "123");
    let before = fs::read(&executable).unwrap();
    let result = build();
    assert!(!result.status.success());
    assert_eq!(fs::read(&executable).unwrap(), before);
    fs::remove_dir_all(dir).unwrap();
}
#[test]
fn check_error_identifies_file_and_location() {
    let path = std::env::temp_dir().join(format!("ae-invalid-{}-{}.ae", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed)));
    fs::write(&path, "fn main() { print(x); }").unwrap();
    let result = Command::new(env!("CARGO_BIN_EXE_ae")).arg("check").arg(&path).output().unwrap();
    fs::remove_file(&path).unwrap();
    assert!(!result.status.success());
    let message = String::from_utf8_lossy(&result.stderr);
    assert!(message.contains("1:19:"), "{message}");
    assert!(message.contains("unknown variable"), "{message}");
}
#[test]
fn control_flow_and_scopes_execute() {
    let source = "fn main() { var n = 0; var sum = 0; while n < 5 { let value = n * 2; sum = sum + value; n = n + 1; } if sum == 20 { let result = sum + 1; print(result); } else { print(0); } if n != 5 { print(999); } else { print(n); } }";
    let output = execute(source);
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    assert_eq!(String::from_utf8_lossy(&output.stdout).replace("\r\n", "\n"), "21\n5\n");
}
#[test]
fn loop_conditions_are_rechecked_and_short_branches_are_not_executed() {
    let source = "fn main() { var n = 2; while n > 0 { n = n - 1; } if n >= 1 { print(1 / 0); } else { print(n); } }";
    let output = execute(source);
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), "0");
}
#[test]
fn block_variables_do_not_escape() {
    let path = std::env::temp_dir().join(format!("ae-scope-{}-{}.ae", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed)));
    fs::write(&path, "fn main() { if 1 == 1 { let secret = 1; } print(secret); }").unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_ae")).arg("check").arg(&path).output().unwrap();
    fs::remove_file(path).unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("unknown variable 'secret'"));
}
#[test]
fn excessive_nesting_returns_diagnostic_instead_of_crashing() {
    let nested = format!("fn main() {{ print({}1{}); }}", "(".repeat(300), ")".repeat(300));
    let output = execute(&nested);
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("nesting limit"));
    let long = format!("fn main() {{ print({}); }}", vec!["1"; 300].join(" + "));
    let output = execute(&long);
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("complexity limit"));
    let blocks = format!("fn main() {{ {} print(1); {} }}", "if 1 == 1 { ".repeat(100), "}".repeat(100));
    let output = execute(&blocks);
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("block nesting limit"));
}
#[test]
fn signed_arithmetic_boundaries_match_checked_integer_semantics() {
    let success = execute("fn main() { print((0 - 9223372036854775807 - 1) * 1); print((0 - 2) * (0 - 3)); print((0 - 7) / 3); print(9223372036854775807 + (0 - 1)); }");
    assert!(success.status.success(), "{}", String::from_utf8_lossy(&success.stderr));
    assert_eq!(String::from_utf8_lossy(&success.stdout).replace("\r\n", "\n"), "-9223372036854775808\n6\n-2\n9223372036854775806\n");
    for expr in ["(0 - 9223372036854775807 - 1) - 1", "(0 - 9223372036854775807 - 1) * (0 - 1)", "(0 - 3037000500) * 3037000500", "9223372036854775807 - (0 - 1)"] {
        let output = execute(&format!("fn main() {{ print({expr}); }}"));
        assert!(!output.status.success(), "{expr}");
        assert!(String::from_utf8_lossy(&output.stderr).contains("integer overflow"), "{expr}");
    }
}
#[test]
fn runtime_error_reports_the_operator_location() {
    let output = execute("fn main() {\n  let x = 0;\n  print(7 / x);\n}");
    assert!(!output.status.success());
    let error = String::from_utf8_lossy(&output.stderr);
    assert!(error.contains("ae:3:11: division by zero"), "{error}");
}
#[test]
fn sibling_blocks_reuse_names_without_affecting_outer_scope() {
    let source = "fn main() { var outer = 1; if outer == 1 { let local = 3; outer = local; } if outer == 3 { let local = 8; print(local); } print(outer); }";
    let output = execute(source);
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    assert_eq!(String::from_utf8_lossy(&output.stdout).replace("\r\n", "\n"), "8\n3\n");
}
#[test]
fn release_mode_preserves_checked_arithmetic_and_output() {
    let dir = std::env::temp_dir().join(format!("ae-release-{}-{}", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed)));
    fs::create_dir(&dir).unwrap();
    let source = dir.join("input.ae");
    let output = dir.join(if cfg!(windows) { "result.exe" } else { "result" });
    fs::write(&source, "fn main() { var x = 9223372036854775807; print(x + 1); }").unwrap();
    let result = Command::new(env!("CARGO_BIN_EXE_ae")).arg("build").arg(&source).arg("-o").arg(&output).arg("--release").output().unwrap();
    assert!(result.status.success(), "{}", String::from_utf8_lossy(&result.stderr));
    let result = Command::new(&output).output().unwrap();
    assert!(!result.status.success());
    assert!(String::from_utf8_lossy(&result.stderr).contains("integer overflow"));
    fs::write(&source, "fn main() { var x = 2; while x < 5 { x = x + 1; } print(x); }").unwrap();
    let result = Command::new(env!("CARGO_BIN_EXE_ae")).arg("run").arg(&source).arg("--release").output().unwrap();
    assert!(result.status.success(), "{}", String::from_utf8_lossy(&result.stderr));
    assert_eq!(String::from_utf8_lossy(&result.stdout).trim(), "5");
    fs::remove_dir_all(dir).unwrap();
}
#[test]
fn build_is_optimized_by_default_and_debug_builds_behave_the_same() {
    // ADR-59: `build` writes the artifact a user ships, so it defaults to -O2;
    // --debug asks for the fast -O0 build. Both keep every check and must
    // produce the same output.
    let dir = std::env::temp_dir().join(format!("ae-build-levels-{}-{}", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed)));
    fs::create_dir(&dir).unwrap();
    let source = dir.join("input.ae");
    let optimized = dir.join(if cfg!(windows) { "optimized.exe" } else { "optimized" });
    let debug = dir.join(if cfg!(windows) { "debug.exe" } else { "debug" });
    fs::write(&source, "fn main() { var total = 0; for i in 0..1000 { total = total + i; } print(total); }").unwrap();
    for (flag, output) in [(None, &optimized), (Some("--debug"), &debug)] {
        let mut command = Command::new(env!("CARGO_BIN_EXE_ae"));
        command.arg("build").arg(&source).arg("-o").arg(output);
        if let Some(flag) = flag { command.arg(flag); }
        let result = command.output().unwrap();
        assert!(result.status.success(), "{}", String::from_utf8_lossy(&result.stderr));
        let result = Command::new(output).output().unwrap();
        assert!(result.status.success());
        assert_eq!(String::from_utf8_lossy(&result.stdout).trim(), "499500");
    }
    // An unknown flag is still refused instead of being ignored.
    let result = Command::new(env!("CARGO_BIN_EXE_ae")).arg("build").arg(&source).arg("-o").arg(dir.join("nope")).arg("--faster").output().unwrap();
    assert!(!result.status.success());
    fs::remove_dir_all(dir).unwrap();
}
#[test]
fn run_forwards_program_args_with_the_release_flag_before_the_separator() {
    let path = std::env::temp_dir().join(format!("ae-run-forward-{}-{}.ae", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed)));
    fs::write(&path, "fn main() { let a = args(); print(len(a)); print(a[1]); }").unwrap();
    let result = Command::new(env!("CARGO_BIN_EXE_ae")).arg("run").arg(&path).arg("--release").arg("--").arg("hello").output().unwrap();
    fs::remove_file(path).unwrap();
    assert!(result.status.success(), "{}", String::from_utf8_lossy(&result.stderr));
    assert_eq!(String::from_utf8_lossy(&result.stdout).replace("\r\n", "\n"), "2\nhello\n");
}
#[test]
fn math_builtins_compute_report_domain_errors() {
    // ADR-60: thin libm calls. Exact results print identically everywhere;
    // domain errors are IEEE (NaN), and the self-formatted text is stable.
    let source = "fn main() { print(sqrt(4.0)); print(abs(-2.5)); print(floor(2.7)); print(ceil(2.1)); print(round(2.5)); print(round(-2.5)); print(pow(2.0, 10.0)); print(exp(0.0)); print(log(1.0)); print(sin(0.0)); print(cos(0.0)); print(sqrt(-1.0)); print(to_string(sqrt(-1.0)) == \"nan\"); }";
    let output = execute(source);
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    assert_eq!(String::from_utf8_lossy(&output.stdout).replace("\r\n", "\n"), "2.0\n2.5\n2.0\n3.0\n3.0\n-3.0\n1024.0\n1.0\n0.0\n0.0\n1.0\nnan\ntrue\n");
}
#[test]
fn math_transcendental_values_land_within_a_tolerance() {
    // libm implementations may differ by an ulp across platforms, so the
    // portable contract is closeness, not byte-identical transcendental text.
    let source = "fn close(a: f64, b: f64) -> bool { let gap = abs(a - b); return gap < 0.000000000000001; } fn main() { print(close(sqrt(2.0), 1.4142135623730951)); print(close(sin(1.0), 0.8414709848078965)); print(close(cos(1.0), 0.5403023058681398)); print(close(atan2(1.0, 1.0), 0.7853981633974483)); print(close(log(exp(1.0)), 1.0)); print(close(pow(2.0, 0.5), sqrt(2.0))); }";
    let output = execute(source);
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    assert_eq!(String::from_utf8_lossy(&output.stdout).replace("\r\n", "\n"), "true\ntrue\ntrue\ntrue\ntrue\ntrue\n");
}
#[test]
fn min_max_ignore_nan_and_tie_zeros_deterministically() {
    // ADR-64: runtime min/max, not libm — one NaN yields the other, the ±0.0
    // tie follows totalOrder, so printed text is identical on every platform
    // and independent of argument order.
    let source = "fn main() { print(min(2.0, 3.0)); print(max(2.0, 3.0)); print(min(-0.0, 0.0)); print(min(0.0, -0.0)); print(max(-0.0, 0.0)); print(max(0.0, -0.0)); print(min(sqrt(-1.0), 5.0)); print(min(5.0, sqrt(-1.0))); print(max(sqrt(-1.0), 5.0)); print(max(sqrt(-1.0), sqrt(-1.0))); print(min(1.5, 1.5)); }";
    let output = execute(source);
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    assert_eq!(String::from_utf8_lossy(&output.stdout).replace("\r\n", "\n"), "2.0\n3.0\n-0.0\n-0.0\n0.0\n0.0\n5.0\n5.0\n5.0\nnan\n1.5\n");
}
#[test]
fn min_max_reject_wrong_types_and_redefinition() {
    for (source, expected) in [
        ("fn main() { print(min(1, 2)); }", "min expects an f64 argument"),
        ("fn main() { print(max(1.0)); }", "max expects 2 f64 argument(s)"),
        ("fn min(x: f64, y: f64) -> f64 { return x; } fn main() { print(min(1.0, 2.0)); }", "is a built-in"),
        ("fn main() { var max = 1.0; print(max); }", "is a built-in name and cannot be a variable"),
    ] {
        let message = check_rejects(source);
        assert!(message.contains(expected), "{source}: {message}");
    }
}
#[test]
fn math_builtins_reject_wrong_types_arities_and_redefinition() {
    for (source, expected) in [
        ("fn main() { print(sqrt(4)); }", "sqrt expects an f64 argument"),
        ("fn main() { print(abs(true)); }", "abs expects an f64 argument"),
        ("fn main() { print(atan2(1.0)); }", "atan2 expects 2 f64 argument(s)"),
        ("fn main() { print(pow(1.0, 2.0, 3.0)); }", "pow takes 2 argument(s)"),
        ("fn sqrt(x: f64) -> f64 { return x; } fn main() { print(sqrt(1.0)); }", "is a built-in"),
        ("fn main() { var sqrt = 1.0; print(sqrt); }", "is a built-in name and cannot be a variable"),
    ] {
        let message = check_rejects(source);
        assert!(message.contains(expected), "{source}: {message}");
    }
}
#[test]
fn float_vectors_store_index_and_iterate() {
    // ADR-61: vec[f64] behaves like vec with f64 elements — literal, checked
    // indexing, len, push/pop, and by-value for-in iteration.
    let source = "fn mean(values: &vec[f64]) -> f64 { var total = 0.0; for x in values { total = total + x; } return total / to_f64(len(values)); } fn main() { let v = vec[2.0, 4.0, 6.0]; print(len(v)); print(v[0]); print(v[2]); var w = vec[2.0, 4.0, 6.0]; w[1] = 5.5; print(w[1]); print(v[1]); push(w, 8.0); print(len(w)); pop(w); print(len(w)); var acc = f64vec_new(); push(acc, 1.5); push(acc, 2.5); print(len(acc)); print(acc[1]); print(mean(&v)); print(mean(&w)); }";
    let output = execute(source);
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    // mean(v)=4.0; mean(w) with w[1]=5.5 and no trailing 8.0 → (2+5.5+6)/3=4.5
    assert_eq!(String::from_utf8_lossy(&output.stdout).replace("\r\n", "\n"), "3\n2.0\n6.0\n5.5\n4.0\n4\n3\n2\n2.5\n4.0\n4.5\n");
}
#[test]
fn float_vectors_move_release_and_borrow_like_other_owned_types() {
    // Moves empty the source slot; borrows are read-only and refuse writes
    // while active; an early return still releases the live vector.
    let source = "fn consume(values: vec[f64]) -> f64 { if len(values) > 1 { return values[0] + values[1]; } return 0.0; } fn read_only(values: &vec[f64]) -> f64 { return values[1]; } fn maybe(keep: bool) -> f64 { var v = vec[10.0, 20.0, 30.0]; if !keep { return v[0]; } return v[2]; } fn main() { let v = vec[1.0, 2.0, 3.0]; print(read_only(&v)); print(consume(v)); print(maybe(true)); print(maybe(false)); }";
    let output = execute(source);
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    assert_eq!(String::from_utf8_lossy(&output.stdout).replace("\r\n", "\n"), "2.0\n3.0\n30.0\n10.0\n");
}
#[test]
fn float_vector_misuse_is_rejected() {
    for (source, expected) in [
        ("fn main() { let v = vec[1, 2.0]; }", "does not convert implicitly"),
        ("fn main() { let v = vec[\"a\"]; }", "vector elements must be i64 or f64"),
        ("fn main() { var v = vec[1.0]; push(v, 2); }", "vector elements must be f64"),
        ("fn main() { var v = vec[1.0]; sort(v); }", "not supported for vec[f64]"),
        ("fn main() { var v = vec[1.0]; reverse(v); }", "not supported for vec[f64]"),
        ("fn main() { let v = vec[1.0]; print(v[0.5]); }", "index must be an integer"),
        ("fn main() { let v = vec[1.0, 2.0]; let w = v; print(v[0]); }", "moved"),
        ("fn main() { var v = vec[1.0]; v[0] = 1; }", "vector elements must be f64"),
        ("fn main() { var v = vec[1.0]; for x in v { v[0] = 2.0; } }", "while it is borrowed"),
        ("fn main() { var v = f64vec_new(); let x = v; push(v, 1.0); }", "moved"),
        ("fn main() { let v = vec[1.0]; let x = v + 1.0; }", "mixed i64/f64 arithmetic"),
    ] {
        let message = check_rejects(source);
        assert!(message.contains(expected), "{source}: {message}");
    }
}
#[test]
fn float_vectors_live_in_structs_and_enums_and_release_exactly_once() {
    // Struct fields of vec[f64] type release with the struct (leak check runs
    // in the generated-C suite); enum payloads bind as read-only vec[f64]
    // borrows in the match arm, where element access works on the binding.
    let source = "struct Sample { values: vec[f64], label: string } enum Reading { Wave(vec[f64]), None } fn main() { var s = Sample { values: vec[1.0, 2.0], label: \"a\" }; print(s.label); let r = Reading::Wave(vec[3.5, 4.5]); match r { Reading::Wave(w) => { print(w[0] + w[1]); } Reading::None => { print(0.0); } } print(\"done\"); }";
    let output = execute(source);
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    assert_eq!(String::from_utf8_lossy(&output.stdout).replace("\r\n", "\n"), "a\n8.0\ndone\n");
}
#[test]
fn arrays_support_checked_read_write_and_expression_order() {
    let source = "fn main() { let original = [3, 4]; var values = [original[1], 5 + 2]; values[1] = values[0] + values[1]; print(values[1]); print(original[0]); }";
    let output = execute(source);
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    assert_eq!(String::from_utf8_lossy(&output.stdout).replace("\r\n", "\n"), "11\n3\n");
    for expr in ["print(values[2]);", "print(values[0 - 1]);", "values[2] = 1 / 0;"] {
        let output = execute(&format!("fn main() {{ var values = [7, 8]; {expr} }}"));
        assert!(!output.status.success(), "{expr}");
        assert!(String::from_utf8_lossy(&output.stderr).contains("array index out of bounds"), "{expr}: {}", String::from_utf8_lossy(&output.stderr));
    }
}
#[test]
fn array_type_and_mutability_errors_are_rejected() {
    for source in ["fn main() { let a = [1]; a[0] = 2; }", "fn main() { let a = []; }", "fn main() { let a = [1]; print(a); }", "fn main() { var a = [1]; a = 1; }", "fn main() { let a = 1; print(a[0]); }"] {
        let path = std::env::temp_dir().join(format!("ae-array-invalid-{}-{}.ae", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed)));
        fs::write(&path, source).unwrap();
        let result = Command::new(env!("CARGO_BIN_EXE_ae")).arg("check").arg(&path).output().unwrap();
        fs::remove_file(path).unwrap();
        assert!(!result.status.success(), "{source}");
    }
}
#[test]
fn release_array_bounds_checks_remain_enabled() {
    let path = std::env::temp_dir().join(format!("ae-array-release-{}-{}.ae", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed)));
    fs::write(&path, "fn main() { let values = [4, 5]; print(values[2]); }").unwrap();
    let result = Command::new(env!("CARGO_BIN_EXE_ae")).arg("run").arg(&path).arg("--release").output().unwrap();
    fs::remove_file(path).unwrap();
    assert!(!result.status.success());
    assert!(String::from_utf8_lossy(&result.stderr).contains("array index out of bounds"));
}
#[test]
fn generated_integer_program_matches_independent_reference_in_both_modes() {
    // Bounded operands avoid reference overflow, while exercising signed arithmetic,
    // precedence, arrays, repeated assignment, and both C optimization modes.
    let mut state = 0x52fe_7062_d18a_9387u64;
    let mut source = String::from("fn main() { var a = [0, 0, 0];\n");
    let mut expected = String::new();
    for i in 0..250 {
        state ^= state << 13; state ^= state >> 7; state ^= state << 17;
        let a = (state % 20001) as i64 - 10000;
        state ^= state << 13; state ^= state >> 7; state ^= state << 17;
        let b = (state % 20001) as i64 - 10000;
        state ^= state << 13; state ^= state >> 7; state ^= state << 17;
        let c = (state % 200 + 1) as i64;
        let repr = |n: i64| if n < 0 { format!("(0 - {})", -n) } else { n.to_string() };
        let index = i % 3;
        source.push_str(&format!("a[{index}] = ({} * {} + {}) / {}; print(a[{index}]);\n", repr(a), repr(b), repr(a), c));
        expected.push_str(&format!("{}\n", (a * b + a) / c));
    }
    source.push_str("}\n");
    let path = std::env::temp_dir().join(format!("ae-diff-{}-{}.ae", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed)));
    fs::write(&path, source).unwrap();
    for release in [false, true] {
        let mut command = Command::new(env!("CARGO_BIN_EXE_ae"));
        command.arg("run").arg(&path);
        if release { command.arg("--release"); }
        let output = command.output().unwrap();
        assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
        assert_eq!(String::from_utf8_lossy(&output.stdout).replace("\r\n", "\n"), expected);
    }
    fs::remove_file(path).unwrap();
}
#[test]
fn opt_in_timings_preserve_program_stdout() {
    let path = std::env::temp_dir().join(format!("ae-timing-{}-{}.ae", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed)));
    fs::write(&path, "fn main() { print(42); }").unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_ae")).arg("run").arg(&path).env("AE_TIMINGS", "1").output().unwrap();
    fs::remove_file(path).unwrap();
    assert!(output.status.success());
    assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), "42");
    let err = String::from_utf8_lossy(&output.stderr);
    assert!(err.contains("frontend_and_codegen_ms="), "{err}");
    assert!(err.contains("c_compile_and_link_ms="), "{err}");
}
#[test]
fn check_does_not_require_c_compiler() {
    let path = std::env::temp_dir().join(format!("ae-check-{}-{}.ae", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed)));
    fs::write(&path, "fn main() { let a = [1, 2]; print(a[0]); }").unwrap();
    let result = Command::new(env!("CARGO_BIN_EXE_ae")).arg("check").arg(&path).env("AE_CC", "nonexistent-ae-compiler").env("AE_TIMINGS", "1").output().unwrap();
    fs::remove_file(path).unwrap();
    assert!(result.status.success(), "{}", String::from_utf8_lossy(&result.stderr));
    let stderr = String::from_utf8_lossy(&result.stderr);
    assert!(stderr.contains("frontend_ms="));
    assert!(!stderr.contains("c_compile_and_link_ms="));
}
#[test]
fn functions_execute_recursion_and_ordered_arguments() {
    let source = "fn add(a: i64, b: i64) -> i64 { return a + b; } fn fact(n: i64) -> i64 { if n <= 1 { return 1; } else { return n * fact(n - 1); } } fn main() { print(add(fact(5), 3)); }";
    let output = execute(source);
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), "123");
    let output = execute("fn f(a: i64, b: i64) -> i64 { return a + b; } fn main() { print(f(1 / 0, 9223372036854775807 + 1)); }");
    let err = String::from_utf8_lossy(&output.stderr);
    assert!(!output.status.success());
    assert!(err.contains("division by zero"), "{err}");
}
#[test]
fn invalid_functions_are_rejected() {
    for source in [
        "fn f(x: i64) -> i64 { if x > 0 { return 1; } } fn main() {}",
        "fn f(x: i64) -> i64 { x = 2; return x; } fn main() {}",
        "fn f(x: i64) -> i64 { return x; } fn main() { print(f()); }",
        "fn f() -> i64 { return 1; } fn f() -> i64 { return 2; } fn main() {}",
        "fn main() { return 1; }", "fn main(x: i64) {}",
    ] {
        let path = std::env::temp_dir().join(format!("ae-fn-invalid-{}-{}.ae", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed)));
        fs::write(&path, source).unwrap();
        let result = Command::new(env!("CARGO_BIN_EXE_ae")).arg("check").arg(&path).output().unwrap();
        fs::remove_file(path).unwrap();
        assert!(!result.status.success(), "{source}");
    }
}
#[test]
fn function_calls_evaluate_arguments_left_to_right_even_with_side_effects() {
    let source = "fn show(x: i64) -> i64 { print(x); return x; } fn subtract(a: i64, b: i64) -> i64 { return a - b; } fn main() { print(subtract(show(7), show(3))); }";
    let output = execute(source);
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    assert_eq!(String::from_utf8_lossy(&output.stdout).replace("\r\n", "\n"), "7\n3\n4\n");
}
#[test]
fn nested_return_paths_and_early_return_behave_consistently() {
    let source = "fn classify(x: i64) -> i64 { if x < 0 { return 1; } else { if x == 0 { return 2; } else { return 3; } } } fn early(x: i64) -> i64 { if x == 0 { return 4; } return 5; } fn main() { print(classify(0 - 1)); print(classify(0)); print(classify(2)); print(early(0)); print(early(3)); }";
    let output = execute(source);
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    assert_eq!(String::from_utf8_lossy(&output.stdout).replace("\r\n", "\n"), "1\n2\n3\n4\n5\n");
}
#[test]
fn emitted_c_passes_strict_return_type_check() {
    let path = std::env::temp_dir().join(format!("ae-c-check-{}-{}.c", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed)));
    let source = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("examples/functions.ae");
    let generated = Command::new(env!("CARGO_BIN_EXE_ae")).arg("emit-c").arg(source).output().unwrap();
    assert!(generated.status.success());
    fs::write(&path, generated.stdout).unwrap();
    let cc = std::env::var_os("AE_CC").unwrap_or_else(|| "gcc".into());
    let output = Command::new(cc).args(["-std=c11", "-pedantic-errors", "-Werror=return-type", "-fsyntax-only"]).arg(&path).output().unwrap();
    fs::remove_file(path).unwrap();
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
}
#[test]
fn windows_header_is_included_only_when_the_program_needs_it() {
    // A pure-computation program must not pay for parsing <windows.h>; a
    // program that reaches the file layer must still have every declaration.
    // Both generated units have to pass strict C11 syntax checking.
    let cc = std::env::var_os("AE_CC").unwrap_or_else(|| "gcc".into());
    let cases = [
        ("examples/float_math.ae", false),
        ("examples/file_tidy.ae", true),
        ("examples/run_tool.ae", true),
    ];
    for (example, needs_header) in cases {
        let source = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(example);
        let generated = Command::new(env!("CARGO_BIN_EXE_ae")).arg("emit-c").arg(&source).output().unwrap();
        assert!(generated.status.success(), "{example}: {}", String::from_utf8_lossy(&generated.stderr));
        let text = String::from_utf8(generated.stdout).unwrap();
        assert_eq!(text.contains("#include <windows.h>"), needs_header, "{example}: unexpected header decision");
        assert_eq!(text.contains("windows.h omitted"), !needs_header, "{example}: unexpected omission comment");
        let path = std::env::temp_dir().join(format!("ae-win-header-{}-{}.c", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed)));
        fs::write(&path, text).unwrap();
        let checked = Command::new(&cc).args(["-std=c11", "-pedantic-errors", "-Werror=return-type", "-fsyntax-only"]).arg(&path).output().unwrap();
        fs::remove_file(path).unwrap();
        assert!(checked.status.success(), "{example}: {}", String::from_utf8_lossy(&checked.stderr));
    }
}
#[test]
fn forward_calls_and_mutual_recursion_work_in_both_build_modes() {
    let source = "fn main() { print(even(10)); print(odd(10)); } fn even(n: i64) -> i64 { if n == 0 { return 1; } else { return odd(n - 1); } } fn odd(n: i64) -> i64 { if n == 0 { return 0; } else { return even(n - 1); } }";
    let path = std::env::temp_dir().join(format!("ae-forward-{}-{}.ae", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed)));
    fs::write(&path, source).unwrap();
    for release in [false, true] {
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_ae"));
        cmd.arg("run").arg(&path);
        if release { cmd.arg("--release"); }
        let output = cmd.output().unwrap();
        assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
        assert_eq!(String::from_utf8_lossy(&output.stdout).replace("\r\n", "\n"), "1\n0\n");
    }
    fs::remove_file(path).unwrap();
}
#[test]
fn forward_declarations_still_enforce_arity_and_unique_names() {
    for source in ["fn main() { print(f()); } fn f(x: i64) -> i64 { return x; }", "fn main() {} fn f() -> i64 { return 1; } fn f() -> i64 { return 2; }", "fn main() { if 1 == 1 { print(1); }"] {
        let path = std::env::temp_dir().join(format!("ae-forward-invalid-{}-{}.ae", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed)));
        fs::write(&path, source).unwrap();
        let output = Command::new(env!("CARGO_BIN_EXE_ae")).arg("check").arg(&path).output().unwrap();
        fs::remove_file(path).unwrap();
        assert!(!output.status.success(), "{source}");
    }
}
#[test]
fn read_only_array_borrows_support_forwarding_and_bounds_checks() {
    let source = "fn first(a: &[i64; 3]) -> i64 { return a[0]; } fn sum(a: &[i64; 3]) -> i64 { return first(&a) + a[1] + a[2]; } fn main() { var data = [4, 5, 6]; print(sum(&data)); data[0] = 8; print(first(&data)); }";
    let path = std::env::temp_dir().join(format!("ae-borrow-{}-{}.ae", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed)));
    fs::write(&path, source).unwrap();
    for release in [false, true] {
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_ae"));
        cmd.arg("run").arg(&path);
        if release { cmd.arg("--release"); }
        let output = cmd.output().unwrap();
        assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
        assert_eq!(String::from_utf8_lossy(&output.stdout).replace("\r\n", "\n"), "15\n8\n");
    }
    fs::write(&path, "fn f(a: &[i64; 2]) -> i64 { return a[2]; } fn main() { let x = [1, 2]; print(f(&x)); }").unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_ae")).arg("run").arg(&path).arg("--release").output().unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("array index out of bounds"));
    fs::remove_file(path).unwrap();
}
#[test]
fn invalid_borrows_are_rejected() {
    for source in [
        "fn f(a: &[i64; 2]) -> i64 { a[0] = 3; return 1; } fn main() {}",
        "fn f(a: &[i64; 2]) -> i64 { return a[0]; } fn main() { let a = [1]; print(f(&a)); }",
        "fn f(a: &[i64; 2]) -> i64 { return a[0]; } fn main() { let a = [1, 2]; print(f(a)); }",
        "fn f(a: &[i64; 2]) -> i64 { return a[0]; } fn main() { let x = 1; print(f(&x)); }",
        "fn f(a: &[i64; 0]) -> i64 { return 0; } fn main() {}",
        "fn f(a: &[i64; 2]) -> i64 { return a; } fn main() {}",
    ] {
        let path = std::env::temp_dir().join(format!("ae-borrow-invalid-{}-{}.ae", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed)));
        fs::write(&path, source).unwrap();
        let output = Command::new(env!("CARGO_BIN_EXE_ae")).arg("check").arg(&path).output().unwrap();
        fs::remove_file(path).unwrap();
        assert!(!output.status.success(), "{source}");
    }
}
#[test]
fn modulo_and_negation_follow_documented_semantics() {
    let output = execute("fn main() { print(7 % 3); print((0 - 7) % 3); print(7 % (0 - 3)); print(-5 + 2); print(2 * -3); print(-(2 + 3)); }");
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    assert_eq!(String::from_utf8_lossy(&output.stdout).replace("\r\n", "\n"), "1\n-1\n1\n-3\n-6\n-5\n");
}
#[test]
fn modulo_and_negation_boundaries_fail_safely() {
    for (expr, expected) in [
        ("1 % 0", "division by zero"),
        ("(0 - 9223372036854775807 - 1) % (0 - 1)", "integer overflow"),
        ("-(0 - 9223372036854775807 - 1)", "integer overflow"),
    ] {
        let output = execute(&format!("fn main() {{ print({expr}); }}"));
        assert!(!output.status.success(), "{expr}");
        let err = String::from_utf8_lossy(&output.stderr);
        assert!(err.contains(expected), "{expr}: {err}");
    }
}
#[test]
fn break_and_continue_control_loop_flow() {
    // i=1..11: odds accumulate until i>9 breaks; evens continue without accumulating.
    let source = "fn main() { var i = 0; var total = 0; while 1 == 1 { i = i + 1; if i % 2 == 0 { continue; } if i > 9 { break; } total = total + i; } print(total); print(i); }";
    let output = execute(source);
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    assert_eq!(String::from_utf8_lossy(&output.stdout).replace("\r\n", "\n"), "25\n11\n");
    // `continue` must re-evaluate the loop condition, not skip it.
    let source = "fn main() { var n = 0; var hits = 0; while n < 5 { n = n + 1; if n == 3 { continue; } hits = hits + 1; } print(hits); }";
    let output = execute(source);
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), "4");
}
#[test]
fn break_and_continue_outside_loops_are_rejected() {
    for source in ["fn main() { break; }", "fn main() { continue; }", "fn main() { if 1 == 1 { break; } }", "fn f() -> i64 { continue; return 1; } fn main() {}"] {
        let path = std::env::temp_dir().join(format!("ae-loop-invalid-{}-{}.ae", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed)));
        fs::write(&path, source).unwrap();
        let output = Command::new(env!("CARGO_BIN_EXE_ae")).arg("check").arg(&path).output().unwrap();
        fs::remove_file(path).unwrap();
        assert!(!output.status.success(), "{source}");
    }
}
#[test]
fn else_if_chains_evaluate_in_order() {
    let source = "fn classify(n: i64) -> i64 { if n < 0 { return 0 - 1; } else if n == 0 { return 0; } else if n < 10 { return 1; } else { return 2; } } fn main() { print(classify(0 - 5)); print(classify(0)); print(classify(5)); print(classify(50)); }";
    let output = execute(source);
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    assert_eq!(String::from_utf8_lossy(&output.stdout).replace("\r\n", "\n"), "-1\n0\n1\n2\n");
    // A chain without a final else may run no branch at all.
    let source = "fn main() { var x = 5; if x == 1 { print(1); } else if x == 2 { print(2); } print(9); }";
    let output = execute(source);
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), "9");
}
#[test]
fn booleans_and_short_circuit_evaluation_are_correct() {
    let source = "fn show(x: i64) -> bool { print(x); return true; } fn main() { var hit = false; hit = show(1) && show(2); print(hit); hit = show(3) && false; print(hit); hit = false && show(4); print(hit); hit = true || show(5); print(hit); hit = show(6) || show(7); print(hit); hit = !hit; print(hit); }";
    let output = execute(source);
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    // show(1), show(2) both run; show(3) runs and decides; false skips show(4);
    // true skips show(5); show(6) decides so show(7) is skipped.
    assert_eq!(String::from_utf8_lossy(&output.stdout).replace("\r\n", "\n"), "1\n2\ntrue\n3\nfalse\nfalse\ntrue\n6\ntrue\nfalse\n");
}
#[test]
fn bool_functions_and_params_round_trip() {
    let source = "fn is_even(n: i64) -> bool { return n % 2 == 0; } fn xor(a: bool, b: bool) -> bool { if a && b { return false; } else { return a || b; } } fn main() { print(is_even(10)); print(is_even(7)); print(xor(true, false)); print(xor(true, true)); }";
    let output = execute(source);
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    assert_eq!(String::from_utf8_lossy(&output.stdout).replace("\r\n", "\n"), "true\nfalse\ntrue\nfalse\n");
}
#[test]
fn assert_passes_and_fails_at_the_right_location() {
    let output = execute("fn main() { assert(1 == 1); assert(2 > 1); print(9); }");
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), "9");
    let output = execute("fn main() {\n  let x = 5;\n  assert(x == 6);\n}");
    assert!(!output.status.success());
    let err = String::from_utf8_lossy(&output.stderr);
    assert!(err.contains("ae:3:3: assertion failed"), "{err}");
}
#[test]
fn print_handles_strings_with_escapes_and_unicode() {
    let output = execute("fn main() { print(\"hello, world\"); print(\"tab\\there\"); print(\"中文 😀\"); print(\"quote\\\"inside\"); print(len(\"a\\rb\")); print(\"back\\\\slash\"); }");
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    assert_eq!(String::from_utf8_lossy(&output.stdout).replace("\r\n", "\n"), "hello, world\ntab\there\n中文 😀\nquote\"inside\n3\nback\\slash\n");
    // `\r` is three bytes only when it lands in the middle of a line: a lone
    // carriage return at the end of printed text would be swallowed by the
    // terminal, so the length assertion above is the real check.
    for source in ["fn main() { print(\"bad \\q\"); }", "fn main() { print(\"open }", "fn main() { print(\"a\nb\"); }"] {
        let path = std::env::temp_dir().join(format!("ae-str-invalid-{}-{}.ae", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed)));
        fs::write(&path, source).unwrap();
        let result = Command::new(env!("CARGO_BIN_EXE_ae")).arg("check").arg(&path).output().unwrap();
        fs::remove_file(path).unwrap();
        assert!(!result.status.success(), "{source}");
    }
}
#[test]
fn owned_strings_move_concatenate_and_release() {
    // print borrows; let/assign/arguments/returns move; + consumes both
    // operands; assignment re-initializes a moved variable.
    let source = "fn shout(name: string) -> string { return name + \"!\"; } fn main() { var greeting = \"hello\"; print(greeting); let loud = shout(greeting); print(loud); greeting = \"hi\"; print(greeting); let question = greeting + \" there\"; print(question); print(\"done\"); }";
    let output = execute(source);
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    assert_eq!(String::from_utf8_lossy(&output.stdout).replace("\r\n", "\n"), "hello\nhello!\nhi\nhi there\ndone\n");
}
#[test]
fn moved_strings_are_rejected_at_compile_time() {
    for source in [
        "fn main() { let a = \"x\"; let b = a; print(a); }",
        "fn main() { let a = \"x\"; let b = a; let c = a; }",
        "fn main() { let a = \"x\"; f(a); print(a); } fn f(s: string) -> i64 { return 1; }",
        "fn main() { var s = \"x\"; s = s; }",
        "fn main() { let s = \"x\"; let t = s + s; }",
        "fn main() { let a = \"x\"; if true { let b = a; } print(a); }",
    ] {
        let path = std::env::temp_dir().join(format!("ae-move-invalid-{}-{}.ae", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed)));
        fs::write(&path, source).unwrap();
        let output = Command::new(env!("CARGO_BIN_EXE_ae")).arg("check").arg(&path).output().unwrap();
        fs::remove_file(path).unwrap();
        assert!(!output.status.success(), "{source}");
    }
}
#[test]
fn strings_pass_to_functions_and_free_on_all_exit_paths() {
    // Early return, loop break, and scope exit must all release without
    // double-free; output stays correct.
    let source = "fn pick(n: i64) -> string { let a = \"a\"; let b = \"b\"; if n == 0 { return a + \"0\"; } var i = 0; while i < 3 { let tmp = \"t\"; if i == 2 { break; } i = i + 1; } return b + \"9\"; } fn main() { print(pick(0)); print(pick(1)); var s = \"x\"; if true { let inner = s + \"y\"; print(inner); } s = \"z\"; print(s); }";
    let output = execute(source);
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    assert_eq!(String::from_utf8_lossy(&output.stdout).replace("\r\n", "\n"), "a0\nb9\nxy\nz\n");
}
#[test]
fn build_cache_serves_identical_executables_and_rebuilds_on_corruption() {
    let dir = std::env::temp_dir().join(format!("ae-cache-test-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    let cache = dir.join("cache");
    let source = dir.join("prog.ae");
    fs::create_dir(&dir).unwrap();
    fs::write(&source, "fn main() { print(\"cached\"); }").unwrap();
    let executable = dir.join(if cfg!(windows) { "out.exe" } else { "out" });
    let build = |out: &std::path::Path| Command::new(env!("CARGO_BIN_EXE_ae"))
        .arg("build").arg(&source).arg("-o").arg(out)
        .env("AE_CACHE", &cache).env("AE_TIMINGS", "1").output().unwrap();
    // Cold build misses; a second build to a new path must hit the cache.
    assert!(build(&executable).status.success());
    let second = dir.join(if cfg!(windows) { "out2.exe" } else { "out2" });
    let hit = build(&second);
    assert!(hit.status.success(), "{}", String::from_utf8_lossy(&hit.stderr));
    let err = String::from_utf8_lossy(&hit.stderr);
    assert!(err.contains("cache: hit"), "expected cache hit, got: {err}");
    for exe in [&executable, &second] {
        let output = Command::new(exe).output().unwrap();
        assert!(output.status.success());
        assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), "cached");
    }
    // Corrupting the cached entry must not poison future builds: the manifest
    // no longer describes the bytes on disk, so the entry is dropped and the
    // program is rebuilt instead of serving a dead executable.
    let entries = cache.join("entries");
    let entry = fs::read_dir(&entries).unwrap().next().unwrap().unwrap().path();
    let exe_name = if cfg!(windows) { "program.exe" } else { "program" };
    fs::write(entry.join(exe_name), b"garbage").unwrap();
    let third = dir.join(if cfg!(windows) { "out3.exe" } else { "out3" });
    let rebuilt = build(&third);
    assert!(rebuilt.status.success(), "{}", String::from_utf8_lossy(&rebuilt.stderr));
    assert!(String::from_utf8_lossy(&rebuilt.stderr).contains("cache: miss"), "{}", String::from_utf8_lossy(&rebuilt.stderr));
    let repaired = Command::new(&third).output().unwrap();
    assert_eq!(String::from_utf8_lossy(&repaired.stdout).trim(), "cached");
    // A cache clear makes the next build miss and still succeed.
    let clear = Command::new(env!("CARGO_BIN_EXE_ae")).arg("cache").arg("clear")
        .env("AE_CACHE", &cache).output().unwrap();
    assert!(clear.status.success());
    let stats = Command::new(env!("CARGO_BIN_EXE_ae")).arg("cache").arg("stats")
        .env("AE_CACHE", &cache).output().unwrap();
    assert_eq!(String::from_utf8_lossy(&stats.stdout).lines().next().unwrap_or(""), "entries: 0");
    let fourth = dir.join(if cfg!(windows) { "out4.exe" } else { "out4" });
    let miss = build(&fourth);
    assert!(miss.status.success(), "{}", String::from_utf8_lossy(&miss.stderr));
    assert!(String::from_utf8_lossy(&miss.stderr).contains("cache: miss"));
    let output = Command::new(&fourth).output().unwrap();
    assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), "cached");
    let _ = fs::remove_dir_all(&dir);
}
#[test]
fn frontend_cache_reuses_generated_c_until_an_imported_file_changes() {
    let dir = std::env::temp_dir().join(format!("ae-frontend-cache-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    let cache = dir.join("cache");
    let source = dir.join("prog.ae");
    let lib = dir.join("lib.ae");
    fs::create_dir(&dir).unwrap();
    fs::write(&lib, "pub fn answer() -> i64 { return 41; }").unwrap();
    fs::write(&source, "use \"lib.ae\";\nfn main() { print(answer() + 1); }").unwrap();
    let build = |out: &std::path::Path| Command::new(env!("CARGO_BIN_EXE_ae"))
        .arg("build").arg(&source).arg("-o").arg(out)
        .env("AE_CACHE", &cache).env("AE_TIMINGS", "1").output().unwrap();
    let stderr = |out: &std::process::Output| String::from_utf8_lossy(&out.stderr).into_owned();

    // A cold build compiles the frontend and publishes the generated C.
    let first = dir.join(if cfg!(windows) { "one.exe" } else { "one" });
    let cold = build(&first);
    assert!(cold.status.success(), "{}", stderr(&cold));
    assert!(stderr(&cold).contains("frontend-cache: miss"), "{}", stderr(&cold));

    // Nothing moved, so the second build must not run the frontend at all,
    // and it must still produce the same executable.
    let second = dir.join(if cfg!(windows) { "two.exe" } else { "two" });
    let warm = build(&second);
    assert!(warm.status.success(), "{}", stderr(&warm));
    assert!(stderr(&warm).contains("frontend-cache: hit"), "{}", stderr(&warm));
    assert_eq!(fs::read(&first).unwrap(), fs::read(&second).unwrap());
    let output = Command::new(&second).output().unwrap();
    assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), "42");

    // Editing the imported file, not the entry file, must invalidate the entry:
    // the unit list is what the cache is keyed on.
    fs::write(&lib, "pub fn answer() -> i64 { return 42; }").unwrap();
    let third = dir.join(if cfg!(windows) { "three.exe" } else { "three" });
    let stale = build(&third);
    assert!(stale.status.success(), "{}", stderr(&stale));
    assert!(stderr(&stale).contains("frontend-cache: miss"), "{}", stderr(&stale));
    let output = Command::new(&third).output().unwrap();
    assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), "43");

    // A damaged frontend entry is a miss, never a wrong binary.
    let frontend = cache.join("frontend");
    let entry = fs::read_dir(&frontend).unwrap().next().unwrap().unwrap().path();
    fs::write(entry.join("program.c"), b"garbage").unwrap();
    let fourth = dir.join(if cfg!(windows) { "four.exe" } else { "four" });
    let repaired = build(&fourth);
    assert!(repaired.status.success(), "{}", stderr(&repaired));
    assert!(stderr(&repaired).contains("frontend-cache: miss"), "{}", stderr(&repaired));
    let output = Command::new(&fourth).output().unwrap();
    assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), "43");

    // The report counts the frontend entries, and clearing removes them too.
    let stats = || String::from_utf8_lossy(&Command::new(env!("CARGO_BIN_EXE_ae"))
        .arg("cache").arg("stats").env("AE_CACHE", &cache).output().unwrap().stdout).into_owned();
    assert!(stats().contains("frontend entries: 1"), "{}", stats());
    let clear = Command::new(env!("CARGO_BIN_EXE_ae")).arg("cache").arg("clear")
        .env("AE_CACHE", &cache).output().unwrap();
    assert!(clear.status.success());
    assert!(stats().contains("frontend entries: 0"), "{}", stats());
    let _ = fs::remove_dir_all(&dir);
}
#[test]
fn cache_and_version_commands_work() {
    let dir = std::env::temp_dir().join(format!("ae-cache-dir-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    let cache = dir.join("cache");
    let dir_out = Command::new(env!("CARGO_BIN_EXE_ae")).arg("cache").arg("dir")
        .env("AE_CACHE", &cache).output().unwrap();
    assert!(dir_out.status.success());
    assert!(String::from_utf8_lossy(&dir_out.stdout).contains("cache"));
    let version = Command::new(env!("CARGO_BIN_EXE_ae")).arg("version").output().unwrap();
    assert!(version.status.success());
    let text = String::from_utf8_lossy(&version.stdout);
    // Track the manifest instead of a hard-coded number, so a version bump
    // (such as ADR-60's cache-key invalidation) cannot break this test.
    assert!(text.contains(concat!("ae ", env!("CARGO_PKG_VERSION"))), "{text}");
    let unknown = Command::new(env!("CARGO_BIN_EXE_ae")).arg("cache").arg("wat").output().unwrap();
    assert!(!unknown.status.success());
    let _ = fs::remove_dir_all(&dir);
}
#[test]
fn for_loops_iterate_ranges_with_break_and_continue() {
    // continue must still advance the counter; break exits early.
    let source = "fn main() { var total = 0; for i in 0..10 { if i % 2 == 0 { continue; } if i > 7 { break; } total = total + i; } print(total); for i in 3..3 { print(999); } for i in 0..3 { for j in 0..2 { total = total + 1; } } print(total); }";
    let output = execute(source);
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    // odds below 8: 1+3+5+7 = 16; empty range prints nothing; nested adds 6 → 22
    assert_eq!(String::from_utf8_lossy(&output.stdout).replace("\r\n", "\n"), "16\n22\n");
}
#[test]
fn for_range_bounds_evaluate_once_in_order() {
    let source = "fn show(x: i64) -> i64 { print(x); return x; } fn main() { for i in show(1)..show(4) { print(i); } }";
    let output = execute(source);
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    assert_eq!(String::from_utf8_lossy(&output.stdout).replace("\r\n", "\n"), "1\n4\n1\n2\n3\n");
}
#[test]
fn len_reports_array_lengths_and_string_sizes() {
    // Array len is compile-time; string len counts UTF-8 bytes at runtime.
    let source = "fn main() { let a = [5, 6, 7]; print(len(a)); let s = \"héllo\"; print(len(s)); print(len(s + \"!\")); var n = 0; for i in 0..len(a) { n = n + a[i]; } print(n); }";
    let output = execute(source);
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    // "héllo" = h + é (2 bytes) + l + l + o = 6 bytes; +1 for "!"
    assert_eq!(String::from_utf8_lossy(&output.stdout).replace("\r\n", "\n"), "3\n6\n7\n18\n");
}
#[test]
fn for_and_len_misuse_is_rejected() {
    for source in [
        "fn len(x: i64) -> i64 { return x; } fn main() {}",
        "fn main() { for i in 0..3 { i = 2; } }",
        "fn main() { for i in true..3 { print(i); } }",
        "fn main() { for i in 0..\"x\" { print(i); } }",
        "fn main() { for i in 0 { print(i); } }",
        "fn main() { let s = 1; print(len(s)); }",
        "fn main() { print(len(1)); }",
        "fn main() { let a = [1]; print(len(a + 1)); }",
        "fn main() { let a = \"x\"; let b = a; print(len(a)); }",
    ] {
        let path = std::env::temp_dir().join(format!("ae-for-invalid-{}-{}.ae", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed)));
        fs::write(&path, source).unwrap();
        let output = Command::new(env!("CARGO_BIN_EXE_ae")).arg("check").arg(&path).output().unwrap();
        fs::remove_file(path).unwrap();
        assert!(!output.status.success(), "{source}");
    }
}
#[test]
fn for_loop_counter_overflow_fails_safely() {
    // The hidden increment is checked arithmetic: i reaching i64::MAX fails.
    let output = execute("fn main() { for i in 9223372036854775805..9223372036854775807 { print(i); } print(9223372036854775807); }");
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    let output = execute("fn main() { for i in 9223372036854775806..9223372036854775807 { print(i); } }");
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), "9223372036854775806");
}
#[test]
fn vectors_grow_push_pop_and_bounds_check() {
    // Growth crosses several realloc doublings; len tracks every change.
    let source = "fn main() { var v = vec[]; print(len(v)); for i in 0..100 { push(v, i * i); } print(len(v)); print(v[99]); pop(v); print(len(v)); print(v[98]); var w = vec[5, 6]; push(w, 7); print(len(w)); print(w[0] + w[1] + w[2]); }";
    let output = execute(source);
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    assert_eq!(String::from_utf8_lossy(&output.stdout).replace("\r\n", "\n"), "0\n100\n9801\n99\n9604\n3\n18\n");
}
#[test]
fn vector_bounds_failures_report_locations() {
    // Write checks the index before evaluating the value (division by zero
    // must not fire first).
    for (source, expected) in [
        ("fn main() { var v = vec[1]; print(v[1]); }", "array index out of bounds"),
        ("fn main() { var v = vec[1]; print(v[0 - 1]); }", "array index out of bounds"),
        ("fn main() { var v = vec[1]; v[5] = 1 / 0; }", "array index out of bounds"),
        ("fn main() { var v = vec[]; pop(v); }", "pop from empty vector"),
    ] {
        let output = execute(source);
        assert!(!output.status.success(), "{source}");
        let err = String::from_utf8_lossy(&output.stderr);
        assert!(err.contains(expected), "{source}: {err}");
    }
}
#[test]
fn vectors_move_through_functions_and_release() {
    // Passing moves; returning moves out; loop break releases body vectors.
    let source = "fn sum(v: vec) -> i64 { var total = 0; for i in 0..len(v) { total = total + v[i]; } return total; } fn make(n: i64) -> vec { var v = vec[]; for i in 1..n + 1 { push(v, i); } return v; } fn main() { var a = make(4); print(len(a)); print(sum(a)); a = vec[9]; print(sum(a)); var b = make(3); if true { let inner = make(2); print(len(inner)); } print(len(b)); }";
    let output = execute(source);
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    assert_eq!(String::from_utf8_lossy(&output.stdout).replace("\r\n", "\n"), "4\n10\n9\n2\n3\n");
}
#[test]
fn vector_misuse_is_rejected_at_compile_time() {
    for source in [
        "fn main() { let v = vec[1]; push(v, 2); }",
        "fn main() { var v = vec[1]; let w = v; push(v, 2); }",
        "fn main() { var v = vec[1]; let w = v; print(len(v)); }",
        "fn main() { let v = vec[true]; }",
        "fn main() { print(vec[1, 2]); }",
        "fn push(v: vec) -> i64 { return 1; } fn main() {}",
        "fn main() { var len = 1; }",
        "fn main() { var v = vec[1]; var w = vec[2]; v = w; push(v, 3); print(len(w)); }",
        "fn main() { var s = \"x\"; push(s, 1); }",
        "fn main() { var v = vec[1]; v = 5; }",
    ] {
        let path = std::env::temp_dir().join(format!("ae-vec-invalid-{}-{}.ae", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed)));
        fs::write(&path, source).unwrap();
        let output = Command::new(env!("CARGO_BIN_EXE_ae")).arg("check").arg(&path).output().unwrap();
        fs::remove_file(path).unwrap();
        assert!(!output.status.success(), "{source}");
    }
}
#[test]
fn moving_an_outer_value_inside_a_loop_is_rejected() {
    // The body runs again, so a value moved out of an outer variable would be
    // read after the move on the next iteration. Without a fixed point over the
    // body the compiler rejects the move instead of relying on the runtime
    // sentinel (ADR-20).
    for source in [
        "fn main() { var v = vec[1, 2]; while len(v) > 0 { print(v[0]); let w = v; } }",
        "fn main() { var s = \"x\"; var i = 0; while i < 2 { let t = s; print(len(t)); i = i + 1; } }",
        "fn main() { var s = \"x\"; for i in 0..2 { let t = s; print(len(t)); } }",
        "fn main() { let v = vec[1]; var s = \"x\"; for x in v { let t = s; print(len(t)); } }",
    ] {
        let path = std::env::temp_dir().join(format!("ae-loop-move-{}-{}.ae", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed)));
        fs::write(&path, source).unwrap();
        let output = Command::new(env!("CARGO_BIN_EXE_ae")).arg("check").arg(&path).output().unwrap();
        fs::remove_file(path).unwrap();
        assert!(!output.status.success(), "accepted a move inside a loop: {source}");
        let err = String::from_utf8_lossy(&output.stderr);
        assert!(err.contains("inside a loop body") && err.contains("re-initialize"), "{err}");
    }
    // Re-initializing before the body ends keeps the next iteration valid.
    let output = execute("fn main() { var s = \"x\"; var i = 0; while i < 2 { print(len(s)); s = \"yy\"; i = i + 1; } print(s); }");
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    assert_eq!(String::from_utf8_lossy(&output.stdout).replace("\r\n", "\n"), "1\n2\nyy\n");
}
#[test]
fn returning_a_live_owned_local_keeps_the_buffer_valid() {
    // Regression: the exit release list must skip slots the return moved
    // out of, for both owned types.
    let source = "fn pick_v() -> vec { var v = vec[7, 8]; return v; } fn pick_s() -> string { let s = \"live\"; return s; } fn main() { var v = pick_v(); print(len(v)); print(v[1]); print(pick_s()); }";
    let output = execute(source);
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    assert_eq!(String::from_utf8_lossy(&output.stdout).replace("\r\n", "\n"), "2\n8\nlive\n");
}
#[test]
fn len_works_in_binary_and_condition_contexts() {
    // Regression: len() fast paths once skipped the binary-operator loop.
    let source = "fn main() { var v = vec[1, 2, 3]; let a = [9, 9, 9, 9]; let s = \"abcd\"; if len(v) > 2 { print(len(v) + 1); } print(len(a) * 2); print(len(s) == 4 && len(v) < len(a)); }";
    let output = execute(source);
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    assert_eq!(String::from_utf8_lossy(&output.stdout).replace("\r\n", "\n"), "4\n8\ntrue\n");
}
#[test]
fn deep_import_chain_loads_without_recursive_host_stack() {
    let dir = unique_temp_dir("deep-imports");
    const COUNT: usize = 700;
    for index in 0..COUNT {
        let name = format!("module_{index:04}.ae");
        let mut source = if index + 1 < COUNT {
            format!("use \"module_{:04}.ae\";\n", index + 1)
        } else { String::new() };
        if index == 0 { source.push_str("fn main() { print(7); }\n"); }
        fs::write(dir.join(name), source).unwrap();
    }
    let entry = dir.join("module_0000.ae");
    let library_entry = entry.clone();
    let library_result = std::thread::Builder::new().stack_size(512 * 1024)
        .spawn(move || ae_compiler::frontend::check_path(&library_entry)).unwrap()
        .join().expect("small-stack check must not panic");
    assert!(library_result.is_ok(), "{library_result:?}");
    let result = Command::new(env!("CARGO_BIN_EXE_ae")).arg("check").arg(&entry).output().unwrap();
    assert!(result.status.success(), "{}", String::from_utf8_lossy(&result.stderr));
    let deps = Command::new(env!("CARGO_BIN_EXE_ae")).arg("deps").arg(&entry).output().unwrap();
    assert!(deps.status.success(), "{}", String::from_utf8_lossy(&deps.stderr));
    let text = String::from_utf8_lossy(&deps.stdout);
    let modules: Vec<_> = text.lines().filter(|s| s.starts_with("module ")).collect();
    assert_eq!(modules.len(), COUNT);
    assert_eq!(modules.first().copied(), Some("module module_0699.ae"));
    assert_eq!(modules.last().copied(), Some("module module_0000.ae"));
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn diamond_and_repeated_imports_load_one_module_and_cycles_still_fail() {
    let dir = unique_temp_dir("diamond");
    fs::create_dir(dir.join("sub")).unwrap();
    fs::write(dir.join("shared.ae"), "pub fn shared() -> i64 { return 7; }\n").unwrap();
    fs::write(dir.join("left.ae"), "use \"shared.ae\";\npub fn left() -> i64 { return shared(); }\n").unwrap();
    fs::write(dir.join("right.ae"), "use \"sub/../shared.ae\";\npub fn right() -> i64 { return shared() + 1; }\n").unwrap();
    let main = dir.join("main.ae");
    fs::write(&main, "use \"left.ae\";\nuse \"right.ae\";\nuse \"shared.ae\";\nfn main() { print(left()); print(right()); }\n").unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_ae")).arg("run").arg(&main).output().unwrap();
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    assert_eq!(String::from_utf8_lossy(&output.stdout).replace("\r\n", "\n"), "7\n8\n");
    let output = Command::new(env!("CARGO_BIN_EXE_ae")).arg("deps").arg(&main).output().unwrap();
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    assert_eq!(String::from_utf8_lossy(&output.stdout).lines().filter(|line| *line == "module shared.ae").count(), 1);

    fs::write(dir.join("shared.ae"), "use \"left.ae\";\npub fn shared() -> i64 { return 7; }\n").unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_ae")).arg("check").arg(&main).output().unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("circular import"));
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn modules_import_transitively_and_reject_cycles_or_conflicts() {
    let dir = std::env::temp_dir().join(format!("ae-mod-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(dir.join("sub")).unwrap();
    // Cross-file names need `pub`; `hidden` stays private on purpose.
    fs::write(dir.join("lib.ae"), "use \"sub/deep.ae\";\npub fn twice(x: i64) -> i64 { return x * 2; }\npub fn quad(x: i64) -> i64 { return twice(twice(x)); }\nfn hidden() -> i64 { return 0; }\n").unwrap();
    fs::write(dir.join("sub/deep.ae"), "pub fn base() -> i64 { return 3; }\n").unwrap();
    let main_file = dir.join("main.ae");
    let executable = dir.join(if cfg!(windows) { "prog.exe" } else { "prog" });
    let build_with = |text: &str, out: &std::path::Path| {
        fs::write(&main_file, text).unwrap();
        let _ = fs::remove_file(out);
        Command::new(env!("CARGO_BIN_EXE_ae")).arg("build").arg(&main_file).arg("-o").arg(out).output().unwrap()
    };
    // Transitive import: main -> lib -> sub/deep.
    let result = build_with("use \"lib.ae\";\nfn main() { print(base()); print(quad(base())); }", &executable);
    assert!(result.status.success(), "{}", String::from_utf8_lossy(&result.stderr));
    let output = Command::new(&executable).output().unwrap();
    assert_eq!(String::from_utf8_lossy(&output.stdout).replace("\r\n", "\n"), "3\n12\n");
    // A name that exists but is not exported is rejected, not silently filtered.
    let result = build_with("use \"lib.ae\";\nfn main() { print(hidden()); }", &executable);
    assert!(!result.status.success());
    let err = String::from_utf8_lossy(&result.stderr).to_string();
    assert!(err.contains("'hidden' is private to") && err.contains("mark it 'pub'"), "{err}");
    // A private type cannot appear in an exported signature either.
    fs::write(dir.join("leaky.ae"), "struct Secret { n: i64 }\npub fn reveal(s: Secret) -> i64 { return s.n; }\n").unwrap();
    let result = build_with("use \"leaky.ae\";\nfn main() { print(1); }", &executable);
    assert!(!result.status.success());
    let err = String::from_utf8_lossy(&result.stderr).to_string();
    assert!(err.contains("exposes private struct 'Secret'"), "{err}");
    // Cycles are rejected with a diagnostic.
    fs::write(dir.join("cycle1.ae"), "use \"cycle2.ae\";\nfn a() -> i64 { return 1; }\nfn main() { print(a()); }").unwrap();
    fs::write(dir.join("cycle2.ae"), "use \"cycle1.ae\";\nfn b() -> i64 { return 2; }\n").unwrap();
    let result = build_with("use \"cycle1.ae\";\nfn main() { print(1); }", &executable);
    assert!(!result.status.success());
    assert!(String::from_utf8_lossy(&result.stderr).contains("circular import"), "{}", String::from_utf8_lossy(&result.stderr));
    // Duplicate names across files are rejected.
    fs::write(dir.join("dupe.ae"), "fn twice(x: i64) -> i64 { return x; }\n").unwrap();
    let result = build_with("use \"lib.ae\";\nuse \"dupe.ae\";\nfn main() { print(1); }", &executable);
    assert!(!result.status.success());
    assert!(String::from_utf8_lossy(&result.stderr).contains("duplicate function"), "{}", String::from_utf8_lossy(&result.stderr));
    // Missing import files fail cleanly; two mains are rejected.
    let result = build_with("use \"nope.ae\";\nfn main() { print(1); }", &executable);
    assert!(!result.status.success());
    assert!(String::from_utf8_lossy(&result.stderr).contains("cannot open"), "{}", String::from_utf8_lossy(&result.stderr));
    fs::write(dir.join("other.ae"), "fn main() { print(2); }").unwrap();
    let result = build_with("use \"other.ae\";\nfn main() { print(1); }", &executable);
    assert!(!result.status.success());
    assert!(String::from_utf8_lossy(&result.stderr).contains("exactly one main"), "{}", String::from_utf8_lossy(&result.stderr));
    // Errors name the file and position.
    fs::write(dir.join("lib.ae"), "fn twice(x: i64) -> i64 { return x * 2; }\nfn quad(x: i64) -> i64 { return oops; }\n").unwrap();
    let result = build_with("use \"lib.ae\";\nfn main() { print(quad(1)); }", &executable);
    assert!(!result.status.success());
    let err = String::from_utf8_lossy(&result.stderr);
    assert!(err.contains("lib.ae") && err.contains("unknown variable"), "{err}");
    let _ = fs::remove_dir_all(&dir);
}
#[test]
fn file_io_round_trips_unicode_paths_and_content() {
    let dir = std::env::temp_dir().join(format!("ae-io-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir(&dir).unwrap();
    // Non-ASCII file name exercises UTF-8 path handling end to end.
    let data_file = dir.join("数据-😀.txt");
    let data_path = data_file.display().to_string().replace('\\', "/");
    let literal = format!("\"{}\"", data_path.replace('\\', "\\\\"));
    let source = format!(
        "fn main() {{\n  let text = \"line one\\n中文行\\n\";\n  let n = write_file({lit}, text);\n  print(n);\n  let back = read_file({lit});\n  print(len(back));\n  print(back == text);\n}}",
        lit = literal
    );
    let path = std::env::temp_dir().join(format!("ae-io-src-{}.ae", std::process::id()));
    fs::write(&path, &source).unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_ae")).arg("run").arg(&path).output().unwrap();
    fs::remove_file(path).unwrap();
    assert!(output.status.success(), "{} / {}", String::from_utf8_lossy(&output.stderr), source);
    // "line one\n中文行\n" = 9 + 9 + 1 = 19 bytes (中文 = 6 bytes).
    assert_eq!(String::from_utf8_lossy(&output.stdout).replace("\r\n", "\n"), "19\n19\ntrue\n");
    assert_eq!(fs::read_to_string(&data_file).unwrap(), "line one\n中文行\n");
    let _ = fs::remove_dir_all(&dir);
}
#[test]
fn string_equality_compares_content() {
    let source = "fn main() { let a = \"hello\"; let b = \"hell\" + \"o\"; print(a == b); print(a != b); print(\"x\" == \"y\"); print(a == \"hello\"); print(read_file(\"z-无.txt\") == read_file(\"z-无.txt\")); }";
    let output = execute(source);
    assert!(!output.status.success()); // read of missing file fails at runtime — checks ordering below instead
    let source = "fn main() { let a = \"hello\"; let b = \"hell\" + \"o\"; print(a == b); print(a != b); print(\"x\" == \"y\"); print(a == \"hello\"); }";
    let output = execute(source);
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    assert_eq!(String::from_utf8_lossy(&output.stdout).replace("\r\n", "\n"), "true\nfalse\nfalse\ntrue\n");
}
#[test]
fn file_io_failures_are_runtime_diagnostics() {
    let output = execute("fn main() { let text = read_file(\"definitely-missing-⟳.txt\"); print(len(text)); }");
    assert!(!output.status.success());
    let err = String::from_utf8_lossy(&output.stderr);
    assert!(err.contains("cannot read file"), "{err}");
    let output = execute("fn main() { let n = write_file(\"/no/such/dir/x.txt\", \"a\"); print(n); }");
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("cannot write file"));
}
#[test]
fn bytes_builtin_converts_strings_for_indexing() {
    let source = "fn main() { let data = bytes(\"A中\"); print(len(data)); print(data[0]); print(data[1] * 256 + data[2]); var hits = 0; for i in 0..len(data) { if data[i] > 127 { hits = hits + 1; } } print(hits); }";
    let output = execute(source);
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    // "A中" = 65, 228, 184, 173 (4 bytes); 中 > 127 in every byte → 3 hits
    assert_eq!(String::from_utf8_lossy(&output.stdout).replace("\r\n", "\n"), "4\n65\n58552\n3\n");
}
#[test]
fn file_builtins_are_reserved() {
    for source in ["fn read_file(p: string) -> string { return p; } fn main() {}", "fn main() { var bytes = 1; }", "fn write_file(a: string, b: string) -> i64 { return 1; } fn main() {}"] {
        let path = std::env::temp_dir().join(format!("ae-io-invalid-{}-{}.ae", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed)));
        fs::write(&path, source).unwrap();
        let output = Command::new(env!("CARGO_BIN_EXE_ae")).arg("check").arg(&path).output().unwrap();
        fs::remove_file(path).unwrap();
        assert!(!output.status.success(), "{source}");
    }
}
#[test]
fn contains_and_slice_cover_text_processing() {
    let source = "fn main() { let t = \"the quick brown fox\"; print(contains(t, \"quick\")); print(contains(t, \"lazy\")); print(contains(t, \"\")); let word = slice(t, 4, 9); print(word); print(len(word)); print(word == \"quick\"); print(slice(t, 0, 3) + \" \" + slice(t, 16, 19)); }";
    let output = execute(source);
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    assert_eq!(String::from_utf8_lossy(&output.stdout).replace("\r\n", "\n"), "true\nfalse\ntrue\nquick\n5\ntrue\nthe fox\n");
    for (source, expected) in [
        ("fn main() { print(slice(\"abc\", 1, 5)); }", "string index out of bounds"),
        ("fn main() { print(slice(\"abc\", 2, 1)); }", "string index out of bounds"),
        ("fn main() { print(slice(\"abc\", 0 - 1, 2)); }", "string index out of bounds"),
    ] {
        let output = execute(source);
        assert!(!output.status.success(), "{source}");
        assert!(String::from_utf8_lossy(&output.stderr).contains(expected), "{source}");
    }
}
#[test]
fn for_in_iterates_vector_elements_by_value() {
    let source = "fn main() { var total = 0; let v = vec[3, 1, 4, 1, 5]; for x in v { total = total + x; } print(total); var hits = 0; for b in v { if b > 2 { hits = hits + 1; continue; } hits = hits + 100; } print(hits); var nested = 0; for x in v { for y in v { if x == y { nested = nested + 1; } } } print(nested); for x in v { if x == 4 { break; } total = total + 1000; } print(total); }";
    let output = execute(source);
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    // total=14; hits: 3,4,5 exceed 2 → +3; the two 1s add 100 each → 203;
    // equal pairs: 1 appears twice so (1,1) matches 4 times, plus (3,3)(4,4)(5,5) → 7;
    // break at x==4: x=3 and x=1 each add 1000 → total 14+2000=2014
    assert_eq!(String::from_utf8_lossy(&output.stdout).replace("\r\n", "\n"), "14\n203\n7\n2014\n");
    for source in [
        "fn main() { let v = vec[1]; for x in v { x = 2; } }",
        "fn main() { let v = vec[1]; for x in v { let w = v; } }",
        "fn make(n: i64) -> vec { var v = vec[]; for i in 1..n + 1 { push(v, i); } return v; } fn main() { for x in make(3) { print(x); } }",
        "fn main() { for x in 5 { print(x); } }",
        "fn f(v: &vec) -> i64 { var t = 0; for x in v { t = t + x; } return t; } fn main() { print(f(&vec[1])); }",
    ] {
        let path = std::env::temp_dir().join(format!("ae-forin-invalid-{}-{}.ae", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed)));
        fs::write(&path, source).unwrap();
        let output = Command::new(env!("CARGO_BIN_EXE_ae")).arg("check").arg(&path).output().unwrap();
        fs::remove_file(path).unwrap();
        assert!(!output.status.success(), "{source}");
    }
}
#[test]
fn structs_construct_access_and_move() {
    let source = "struct Point { x: i64, y: i64 } struct Named { id: i64, name: string } fn describe(n: Named) -> i64 { print(n.name); return n.id; } fn main() { var p = Point{x: 3, y: 4}; print(p.x + p.y); p.x = 30; print(p.x * p.y); let n = Named{id: 7, name: \"ae\"}; print(describe(n)); var q = Point{x: 1, y: 2}; q = Point{x: q.x + 10, y: q.y}; print(q.x + q.y); let s = \"tail\"; print(s == slice(\"head-tail\", 5, 9)); }";
    let output = execute(source);
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    assert_eq!(String::from_utf8_lossy(&output.stdout).replace("\r\n", "\n"), "7\n120\nae\n7\n13\ntrue\n");
    for source in [
        "struct P { x: i64 } fn main() { let p = P{x: 1}; print(p.z); }",
        "struct P { x: i64 } fn main() { let p = P{x: 1, y: 2}; }",
        "struct P { x: i64 } fn main() { let p = P{y: 1}; }",
        "struct P { x: i64, s: string } fn main() { let p = P{x: 1, s: \"a\"}; let t = p.s; }",
        "struct P { x: i64 } fn main() { var p = P{x: 1}; p.x = true; }",
        "struct P { x: i64, p: P } fn main() { let q = P{x: 1}; }",
        "struct P { x: i64 } fn main() { let P = 1; }",
    ] {
        let path = std::env::temp_dir().join(format!("ae-struct-invalid-{}-{}.ae", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed)));
        fs::write(&path, source).unwrap();
        let output = Command::new(env!("CARGO_BIN_EXE_ae")).arg("check").arg(&path).output().unwrap();
        fs::remove_file(path).unwrap();
        assert!(!output.status.success(), "{source}");
    }
}
#[test]
fn struct_fields_release_on_all_exit_paths() {
    // Owned fields inside structs must release on scope exit, early return,
    // and break, without double-free (ASan on Linux verifies).
    let source = "struct Holder { tag: i64, data: string } fn pick(n: i64) -> i64 { var h = Holder{tag: n, data: \"payload\"}; if n == 0 { return h.tag; } for i in 0..2 { let tmp = Holder{tag: i, data: \"t\"}; if i == 1 { break; } } print(h.data); return h.tag; } fn main() { print(pick(0)); print(pick(5)); }";
    let output = execute(source);
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    assert_eq!(String::from_utf8_lossy(&output.stdout).replace("\r\n", "\n"), "0\npayload\n5\n");
}
#[test]
fn generic_structs_instantiate_one_layout_per_argument_list() {
    // The declaration is a shape; each argument list is a type. Two uses of the
    // same list share one layout (so they are interchangeable), and different
    // lists never mix.
    let source = r#"struct Pair[K, V] { left: K, right: V }
struct Tag { name: string, n: i64 }
fn read(p: Pair[i64, string]) -> i64 {
    print(p.right);
    return p.left;
}
fn main() {
    var p = Pair[string, i64] { left: "a", right: 1 };
    p.right = p.right + 1;
    print(p.left);
    print(p.right);
    let t = Pair[i64, Tag] { left: 5, right: Tag { name: "t", n: 9 } };
    print(t.left);
    print(t.right.name);
    let mixed = Pair[i64, string] { left: 7, right: "seven" };
    print(read(mixed));
    let nested = Pair[string, Pair[string, string]] { left: "x", right: Pair[string, string] { left: "y", right: "z" } };
    print(nested.right.left);
}"#;
    let output = execute(source);
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    assert_eq!(String::from_utf8_lossy(&output.stdout).replace("\r\n", "\n"), "a\n2\n5\nt\nseven\n7\ny\n");
    for source in [
        // Too few type arguments, and a generic name without any.
        "struct P[T] { f: T } fn main() { let p = P[string, i64] { f: \"a\" }; }",
        "struct P[T] { f: T } fn main() { let p = P { f: \"a\" }; }",
        // A different argument list is a different type.
        "struct P[T] { f: T } fn take(p: P[string]) -> i64 { return 1; } fn main() { print(take(P[i64] { f: 2 })); }",
        // The field keeps its declared parameter type.
        "struct P[T] { f: T } fn main() { let p = P[string] { f: 1 }; }",
        // A generic declaration still owns its name.
        "struct P[T] { f: T } struct P { g: i64 } fn main() {}",
        "struct P[T] { f: T } enum P { A } fn main() {}",
        // No recursive layout: the name is not in scope inside its own body.
        "struct P[T] { next: P[T] } fn main() {}",
        // Type parameters are single uppercase letters.
        "struct P[a] { f: a } fn main() {}",
    ] {
        let path = std::env::temp_dir().join(format!("ae-generic-struct-{}-{}.ae", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed)));
        fs::write(&path, source).unwrap();
        let output = Command::new(env!("CARGO_BIN_EXE_ae")).arg("check").arg(&path).output().unwrap();
        fs::remove_file(path).unwrap();
        assert!(!output.status.success(), "{source}");
    }
}
#[test]
fn generic_functions_are_templates_for_one_argument_list() {
    // The body is parsed once per argument list, so a `T` that is a string in
    // one instance is an i64 in another, and instantiating a generic function
    // from inside another generic body composes the substitution.
    let source = r#"fn identity[T](x: T) -> T {
    return x;
}
fn first[T](a: T, b: T) -> T {
    return a;
}
fn wrap[T](x: T) -> Option[T] {
    return Option[T]::Some(x);
}
fn double_wrap[T](x: T) -> Option[T] {
    let inner = wrap[T](x);
    return inner;
}
struct Pair[K, V] { left: K, right: V }
fn pair_of[T](x: T, n: i64) -> Pair[T, i64] {
    return Pair[T, i64] { left: x, right: n };
}
fn main() {
    print(identity[i64](41));
    print(identity[string]("ada"));
    print(first[string]("left", "right"));
    let found = double_wrap[string]("wrapped");
    match found {
        Option::Some(text) => { print(text); }
        Option::None => { print("none"); }
    }
    let pair = pair_of[string]("k", 2);
    print(pair.left);
    print(pair.right);
}"#;
    let output = execute(source);
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    assert_eq!(String::from_utf8_lossy(&output.stdout).replace("\r\n", "\n"), "41\nada\nleft\nwrapped\nk\n2\n");
    for (source, expected) in [
        // The type arguments are mandatory: there is no inference.
        ("fn id[T](x: T) -> T { return x; } fn main() { print(id(5)); }", "is a generic function"),
        ("fn id[T](x: T) -> T { return x; } fn main() { print(id[i64, string](5)); }", "takes 1 type argument(s), found 2"),
        // A body is checked after substitution, against the concrete type.
        ("fn bad[T](x: T) -> i64 { return x + 1; } fn main() { print(bad[string](\"a\")); }", "'+' requires two strings"),
        // Instantiating itself with the same arguments would never finish.
        ("fn again[T](x: T) -> T { return again[T](x); } fn main() { print(again[i64](1)); }", "would never finish"),
        // main cannot be generic, and a generic name is still one name.
        ("fn main[T]() -> i64 { return 1; }", "main cannot be generic"),
        ("fn id[T](x: T) -> T { return x; } fn id(x: i64) -> i64 { return x; } fn main() { print(1); }", "duplicate function 'id'"),
    ] {
        let path = std::env::temp_dir().join(format!("ae-generic-fn-{}-{}.ae", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed)));
        fs::write(&path, source).unwrap();
        let output = Command::new(env!("CARGO_BIN_EXE_ae")).arg("check").arg(&path).output().unwrap();
        fs::remove_file(path).unwrap();
        assert!(!output.status.success(), "accepted invalid program: {source}");
        let err = String::from_utf8_lossy(&output.stderr);
        assert!(err.contains(expected), "{source}: {err}");
    }
}
#[test]
fn changing_the_working_directory_moves_relative_paths() {
    // set_cwd is a process-wide mutation: after it, relative paths resolve
    // against the new directory, and the previous one stays reachable only
    // because the test saved it.
    let root = std::env::temp_dir().join(format!("ae-cwd-{}-{}", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed)));
    fs::create_dir(&root).unwrap();
    let inner = root.join("inner");
    let literal = |p: &std::path::Path| p.display().to_string().replace('\\', "\\\\");
    let source = format!(r#"fn main() {{
    let made = try_create_dir("{inner}");
    match made {{
        StatusResult::Ok => {{ print("created"); }}
        StatusResult::Err(reason) => {{ print("create failed"); }}
    }}
    let moved = set_cwd("{inner}");
    match moved {{
        StatusResult::Ok => {{ print("moved"); }}
        StatusResult::Err(reason) => {{ print("move failed"); }}
    }}
    let written = try_write_file("inside.txt", "data");
    match written {{
        WriteResult::Ok(count) => {{ print(count); }}
        WriteResult::Err(reason) => {{ print("write failed"); }}
    }}
    print(path_exists("inside.txt"));
    let back = set_cwd("{root}");
    match back {{
        StatusResult::Ok => {{ print("back"); }}
        StatusResult::Err(reason) => {{ print("back failed"); }}
    }}
    print(path_exists("inner/inside.txt"));
    let missing = set_cwd("{inner}/absent");
    match missing {{
        StatusResult::Ok => {{ print("unexpected success"); }}
        StatusResult::Err(reason) => {{ print("missing directory is an error"); }}
    }}
}}"#, inner = literal(&inner), root = literal(&root));
    let output = execute(&source);
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    assert_eq!(String::from_utf8_lossy(&output.stdout).replace("\r\n", "\n"), "created\nmoved\n4\ntrue\nback\ntrue\nmissing directory is an error\n");
    fs::remove_dir_all(&root).unwrap();
}
#[test]
fn generic_constraints_reject_arguments_at_the_call_site() {
    // A bound is a shape check on the argument, not a proof about the body:
    // the body is still checked once per instantiation. The error therefore
    // points at the call site, with the allowed types spelled out.
    let source = r#"fn add[T: Add](a: T, b: T) -> T { return a + b; }
fn larger[T: Ord](a: T, b: T) -> T { if a > b { return a; } return b; }
fn same[T: Eq](a: T, b: T) -> bool { return a == b; }
fn copy_twice[T: Copy](x: T) -> T { let y = x; return y; }
fn both[T: Add + Eq](a: T, b: T) -> bool { return (a + b) == a; }
fn main() {
    print(add[i64](2, 3));
    print(add[string]("ae", "ther"));
    print(larger[i64](4, 9));
    print(same[bool](true, true));
    print(same[string]("x", "x"));
    print(copy_twice[i64](7));
    print(both[i64](0, 5));
    print(true == true);
    print(true != false);
    print(false == true);
}"#;
    let output = execute(source);
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    assert_eq!(String::from_utf8_lossy(&output.stdout).replace("\r\n", "\n"), "5\nae\n9\ntrue\ntrue\n7\nfalse\ntrue\ntrue\nfalse\n");
    for source in [
        "fn add[T: Add](a: T, b: T) -> T { return a + b; } fn main() { print(add[bool](true, false)); }",
        "fn larger[T: Ord](a: T, b: T) -> T { if a > b { return a; } return b; } fn main() { print(larger[string](\"a\", \"b\")); }",
        "fn copy_twice[T: Copy](x: T) -> T { return x; } fn main() { print(copy_twice[string](\"a\")); }",
        "fn same[T: Eq](a: T, b: T) -> bool { return a == b; } fn main() { print(same[vec](vec[], vec[])); }",
        "fn f[T: Add + Eq](a: T) -> T { return a; } fn main() { print(f[bool](true)); }",
        "fn f[T: Printable](a: T) -> T { return a; } fn main() { print(f[i64](1)); }",
        "fn f[T: Add + Add](a: T) -> T { return a; } fn main() { print(f[i64](1)); }",
        "struct P[T: Copy] { x: T } fn main() { print(1); }",
        "enum E[T: Copy] { A(T), B } fn main() { print(1); }",
    ] {
        let path = std::env::temp_dir().join(format!("ae-bound-{}-{}.ae", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed)));
        fs::write(&path, source).unwrap();
        let output = Command::new(env!("CARGO_BIN_EXE_ae")).arg("check").arg(&path).output().unwrap();
        fs::remove_file(path).unwrap();
        assert!(!output.status.success(), "{source}");
    }
}

#[test]
fn generic_body_capabilities_are_checked_at_the_declaration() {
    // ADR-40: Copy still admits bool, which cannot be added. The diagnostic
    // appears at the declaration even though the function is never called.
    let path = std::env::temp_dir().join(format!("ae-weak-bound-{}-{}.ae", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed)));
    fs::write(&path, "fn f[T: Copy](a: T, b: T) -> T { return a + b; }\nfn main() { print(1); }").unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_ae")).arg("check").arg(&path).output().unwrap();
    fs::remove_file(&path).unwrap();
    assert!(!output.status.success());
    let message = String::from_utf8_lossy(&output.stderr);
    assert!(message.contains("declares T: Copy"), "{message}");
    assert!(message.contains("does not support it"), "{message}");
    assert!(message.contains(":1:5:"), "diagnostic must point at the constraint: {message}");

    // Narrowing the constraint to the exact supported set is accepted and runs
    // with each admitted type; no new runtime machinery is involved.
    let source = r#"fn f[T: Add](a: T, b: T) -> T { return a + b; }
fn main() { print(f[i64](2, 3)); print(f[string]("a", "b")); print(f[f64](1.5, 2.5)); }"#;
    let output = execute(source);
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    assert_eq!(String::from_utf8_lossy(&output.stdout).replace("\r\n", "\n"), "5\nab\n4.0\n");
}

#[test]
fn many_generic_declarations_in_one_file_keep_their_own_bodies() {
    // ADR-41: sharing one token buffer per file must not move any declaration's
    // `start`; each instantiation still parses its own parameter list and body.
    let mut source = String::new();
    for i in 0..40 {
        source.push_str(&format!("fn f{i}[T: Add](a: T, b: T) -> T {{ return a + b; }}\n"));
        source.push_str(&format!("fn g{i}[T: Ord](a: T, b: T) -> T {{ if a > b {{ return a; }} return b; }}\n"));
    }
    source.push_str("fn main() { ");
    for i in 0..40 {
        source.push_str(&format!("print(f{i}[i64]({i}, 1)); print(g{i}[i64]({i}, 100)); "));
    }
    source.push_str("}");
    let output = execute(&source);
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    let mut expected = Vec::new();
    for i in 0..40 {
        expected.push((i + 1).to_string());
        expected.push("100".to_owned());
    }
    assert_eq!(String::from_utf8_lossy(&output.stdout).replace("\r\n", "\n"), expected.join("\n") + "\n");
}

#[test]
fn capability_audit_applies_to_imported_generic_functions() {
    let dir = std::env::temp_dir().join(format!("ae-w4-import-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir(&dir).unwrap();
    let lib = dir.join("lib.ae");
    let main = dir.join("main.ae");
    fs::write(&main, "use \"lib.ae\";\nfn main() { print(combine[i64](1, 2)); }\n").unwrap();

    // The imported generic is audited in its own file: a weak constraint must
    // be rejected before the importer's call site is ever checked.
    fs::write(&lib, "pub fn combine[T: Eq](a: T, b: T) -> T { return a + b; }\n").unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_ae")).arg("check").arg(&main).output().unwrap();
    assert!(!output.status.success());
    let message = String::from_utf8_lossy(&output.stderr);
    assert!(message.contains("declares T: Eq"), "{message}");
    assert!(message.contains("lib.ae"), "diagnostic must name the defining file: {message}");

    fs::write(&lib, "pub fn combine[T: Add](a: T, b: T) -> T { return a + b; }\n").unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_ae")).arg("run").arg(&main).output().unwrap();
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    assert_eq!(String::from_utf8_lossy(&output.stdout).replace("\r\n", "\n"), "3\n");
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn question_mark_propagates_none_as_well_as_err() {
    // An Option has the same two-variant shape as a result: None is returned
    // to the caller, Some binds its payload. A bare `call()?;` discards the
    // payload, which must still be released exactly once.
    let source = r#"fn maybe_find(flag: bool) -> Option[string] {
    if flag { return Option[string]::Some("found"); }
    return Option[string]::None();
}
fn upper(flag: bool) -> Option[string] { let text = maybe_find(flag)?; return Option[string]::Some(text + "!"); }
fn discard(flag: bool) -> Option[string] { maybe_find(flag)?; return Option[string]::Some("always"); }
fn maybe_numbers(flag: bool) -> Option[vec] {
    if flag { return Option[vec]::Some(vec[1, 2, 3]); }
    return Option[vec]::None();
}
fn count_numbers(flag: bool) -> Option[vec] { maybe_numbers(flag)?; return Option[vec]::Some(vec[9]); }
fn keep_numbers(flag: bool) -> Option[vec] { let items = maybe_numbers(flag)?; return Option[vec]::Some(items); }
enum Maybe { Some(string), None }
fn make_maybe() -> Maybe { return Maybe::Some("custom"); }
fn use_maybe() -> Maybe { let v = make_maybe()?; return Maybe::Some(v); }
fn main() {
    let a = upper(true);
    match a { Option::Some(s) => { print(s); } Option::None => { print("none"); } }
    let b = upper(false);
    match b { Option::Some(s) => { print(s); } Option::None => { print("none"); } }
    let c = discard(true);
    match c { Option::Some(s) => { print(s); } Option::None => { print("none"); } }
    let d = discard(false);
    match d { Option::Some(s) => { print(s); } Option::None => { print("none"); } }
    let e = count_numbers(true);
    match e { Option::Some(items) => { print(len(items)); } Option::None => { print("none"); } }
    let f = count_numbers(false);
    match f { Option::Some(items) => { print(len(items)); } Option::None => { print("none"); } }
    let g = keep_numbers(true);
    match g { Option::Some(items) => { print(len(items)); } Option::None => { print("none"); } }
    let h = keep_numbers(false);
    match h { Option::Some(items) => { print(len(items)); } Option::None => { print("none"); } }
    let m = use_maybe();
    match m { Maybe::Some(s) => { print(s); } Maybe::None => { print("none"); } }
}"#;
    let output = execute(source);
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    assert_eq!(String::from_utf8_lossy(&output.stdout).replace("\r\n", "\n"), "found!\nnone\nalways\nnone\n1\nnone\n3\nnone\ncustom\n");
    for source in [
        "enum Weird { A(i64), B(i64) } fn f() -> Weird { return Weird::A(1); } fn g() -> Weird { let v = f()?; return Weird::A(v); } fn main() { print(1); }",
        "fn f() -> Option[i64] { return Option[i64]::None(); } fn main() { let v = f()?; print(v); }",
        "enum Maybe { Some(i64, i64), None } fn f() -> Maybe { return Maybe::Some(1, 2); } fn g() -> Maybe { let v = f()?; return Maybe::Some(v, 2); } fn main() { print(1); }",
        "enum Maybe { Some(i64), None(string) } fn f() -> Maybe { return Maybe::Some(1); } fn g() -> Maybe { let v = f()?; return Maybe::Some(v); } fn main() { print(1); }",
        "fn f() -> Option[string] { return Option[string]::None(); } fn g() -> Option[i64] { let v = f()?; return Option[i64]::Some(1); } fn main() { print(1); }",
        "fn f() -> Option[string] { return Option[string]::None(); } fn g() -> Option[string] { f(); return Option[string]::Some(\"x\"); } fn main() { print(1); }",
    ] {
        let path = std::env::temp_dir().join(format!("ae-option-{}-{}.ae", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed)));
        fs::write(&path, source).unwrap();
        let output = Command::new(env!("CARGO_BIN_EXE_ae")).arg("check").arg(&path).output().unwrap();
        fs::remove_file(path).unwrap();
        assert!(!output.status.success(), "{source}");
    }
}

#[test]
fn split_and_args_produce_string_vectors() {
    let source = "fn main() { let parts = split(\"a,b,,c\", \",\"); print(len(parts)); print(parts[0]); print(len(parts[2])); let whole = split(\"hello\", \",\"); print(len(whole)); print(whole[0]); var empties = 0; for i in 0..len(parts) { if len(parts[i]) == 0 { empties = empties + 1; } } print(empties); print(join(parts, \"-\")); print(join(parts, \"\")); let a = args(); print(len(a) >= 1); }";
    let output = execute(source);
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    // join(parts, "-") = "a-b--c"; join(parts, "") = "abc"; args() includes at least argv[0]
    assert_eq!(String::from_utf8_lossy(&output.stdout).replace("\r\n", "\n"), "4\na\n0\n1\nhello\n1\na-b--c\nabc\ntrue\n");
    let output = execute("fn main() { let v = split(\"x\", \"\"); print(len(v)); }");
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("empty separator"));
    let output = execute("fn main() { let v = split(\"a,b\", \",\"); print(v[5]); }");
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("array index out of bounds"));
}
#[test]
fn program_arguments_arrive_as_utf8_on_every_platform() {
    // Windows hands `main` the command line in the ANSI code page, so a path
    // typed on a Chinese system used to arrive as GBK bytes while every file API
    // expects UTF-8: the program could not open its own arguments. `args()` now
    // converts each argument through UTF-16 once (ADR-54). The temporary
    // directory name is ASCII but the subdirectory and the file are not, which is
    // exactly the shape that failed.
    let dir = unique_temp_dir("argv-utf8");
    let cjk = dir.join("中文目录");
    fs::create_dir(&cjk).unwrap();
    let target = cjk.join("note.txt");
    let source = dir.join("argv.ae");
    fs::write(&source, "fn main() { let p = args(); let n = write_file(p[1], \"ok\"); print(n); print(read_file(p[1])); }").unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_ae"))
        .arg("run").arg(&source).arg("--").arg(&target)
        .output().unwrap();
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    assert_eq!(String::from_utf8_lossy(&output.stdout).replace("\r\n", "\n").trim(), "2\nok");
    // The same text is a valid UTF-8 argument: three bytes per character.
    let source = dir.join("len.ae");
    fs::write(&source, "fn main() { let p = args(); print(len(p[1])); }").unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_ae"))
        .arg("run").arg(&source).arg("--").arg("中文目录")
        .output().unwrap();
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    assert_eq!(String::from_utf8_lossy(&output.stdout).replace("\r\n", "\n").trim(), "12");
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn print_keeps_every_shape_in_order_with_non_ascii_text() {
    // ADR-55 moved every shape onto one sink so a console run (unbuffered) keeps
    // the order the program wrote. Through a pipe the bytes must be exactly the
    // UTF-8 the program produced, with one newline per value and nothing else.
    let source = r#"
fn main() {
    print("中文测试");
    let s = "混合 ascii 与 中文";
    print(s);
    print(42);
    print(true);
    print(2.5);
    print("引号 \" 与 制表符 \t 结束");
}
"#;
    let output = execute(source);
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    let text = String::from_utf8_lossy(&output.stdout).replace("\r\n", "\n");
    assert_eq!(text, "中文测试\n混合 ascii 与 中文\n42\ntrue\n2.5\n引号 \" 与 制表符 \t 结束\n");
    // A `\n` escape inside a literal is one newline on output and does not
    // truncate what follows it, now that every shape shares the same sink.
    let output = execute("fn main() { print(\"a\\nb\"); }");
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    assert_eq!(String::from_utf8_lossy(&output.stdout).replace("\r\n", "\n"), "a\nb\n");
}

#[test]
fn tokens_stream_the_same_boundaries_as_split_without_materialising() {
    // The lazy cursor must agree with split token for token, restart on a second
    // loop over the same binding, accept a struct field as its base, and keep
    // working with break/continue (which release body-local strings).
    let source = r#"
struct Rec { body: string }
fn mismatches(text: string, sep: string) -> i64 {
    let parts = split(text, sep);
    let cur = tokenize(text, sep);
    var i = 0;
    var bad = 0;
    for t in cur {
        if i >= len(parts) { bad = bad + 1; } else if parts[i] != t { bad = bad + 1; }
        i = i + 1;
    }
    if i != len(parts) { bad = bad + 1; }
    return bad;
}
fn main() {
    print(mismatches("alpha beta gamma", " "));
    print(mismatches("", " "));
    print(mismatches(" ", " "));
    print(mismatches("a b ", " "));
    print(mismatches("a  b", " "));
    print(mismatches("alpha::beta::::gamma", "::"));
    print(mismatches("no-separator", "::"));
    let line = "one two three four";
    let cur = tokenize(line, " ");
    var seen = 0;
    for t in cur {
        let copy = t + "!";
        if len(t) == 3 { continue; }
        if t == "four" { break; }
        seen = seen + len(copy);
    }
    print(seen);
    var again = 0;
    for t in cur { again = again + 1; }
    let rec = Rec { body: "p q r" };
    let fields = tokenize(rec.body, " ");
    var n = 0;
    for f in fields { n = n + 1; }
    print(again);
    print(n);
}
"#;
    let output = execute(source);
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).replace("\r\n", "\n"),
        "0\n0\n0\n0\n0\n0\n0\n6\n4\n3\n"
    );
}

#[test]
fn tokens_reject_a_base_that_moves_is_written_or_is_a_temporary() {
    // The cursor borrows its base for the scope, so the base cannot be moved,
    // reassigned or have its field written while the cursor is alive; a
    // temporary makes no sense (it would be freed before the first iteration).
    let cases = [
        ("fn main() { var text = \"a b\"; let c = tokenize(text, \" \"); text = \"x\"; for t in c { print(t); } }", "while it is borrowed"),
        ("fn main() { let text = \"a b\"; let c = tokenize(text, \" \"); let m = text; for t in c { print(t); } print(m); }", "while it is borrowed"),
        ("struct R { body: string } fn main() { var r = R { body: \"a b\" }; let c = tokenize(r.body, \" \"); r.body = \"z\"; for t in c { print(t); } }", "while it is borrowed"),
        ("fn main() { let text = \"a b\"; for t in tokenize(text, \" \") { print(t); } }", "bare cursor variable"),
        ("fn main() { let text = \"a b\"; let c = tokenize(text, \" \"); print(len(c)); }", "token cursor"),
        ("fn main() { let text = \"a b\"; let c = tokenize(text, \" \"); print(c); }", "token cursor"),
        ("fn main() { let c = tokenize(\"a b\", \" \"); for t in c { print(t); } }", "tokenize expects a string variable"),
    ];
    for (source, expected) in cases {
        let output = execute(source);
        assert!(!output.status.success(), "accepted: {source}");
        let err = String::from_utf8_lossy(&output.stderr);
        assert!(err.contains(expected), "{source}: {err}");
    }
    // An empty separator is a runtime failure at the call site, like split.
    let output = execute("fn main() { let text = \"a b\"; let c = tokenize(text, \"\"); for t in c { print(t); } }");
    assert!(!output.status.success());
    let err = String::from_utf8_lossy(&output.stderr);
    assert!(err.contains("empty separator"), "{err}");
    // The failure points at the call, not at 0:0 (the cursor carries its site).
    assert!(err.contains(":1:39"), "location missing: {err}");

    // The separator is copied into the cursor, so it has a fixed 16-byte
    // capacity: exactly 16 works, 17 is rejected rather than allocated.
    let output = execute("fn main() { let t = \"a0123456789abcdefb\"; let c = tokenize(t, \"0123456789abcdef\"); var n = 0; for x in c { print(x); n = n + 1; } print(n); }");
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    assert_eq!(String::from_utf8_lossy(&output.stdout).replace("\r\n", "\n"), "a\nb\n2\n");
    let output = execute("fn main() { let t = \"aXb\"; let c = tokenize(t, \"0123456789abcdefX\"); for x in c { print(x); } }");
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("longer than 16 bytes"));
}

#[test]
fn field_views_borrow_the_whole_struct_variable() {
    // Reading a string field through a view is allowed; the variable counts as
    // borrowed, so writes and moves are rejected while the view can be read.
    let output = execute("struct P { name: string } fn main() { let p = P { name: \"hello world\" }; let h = slice_view(p.name, 0, 5); print(h); print(len(p.name)); }");
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    assert_eq!(String::from_utf8_lossy(&output.stdout).replace("\r\n", "\n"), "hello\n11\n");
    for (source, expected) in [
        ("struct P { name: string } fn main() { var p = P { name: \"hello\" }; let h = slice_view(p.name, 0, 2); p.name = \"replaced\"; print(h); }", "while it is borrowed"),
        ("struct P { name: string } fn main() { var p = P { name: \"hello\" }; let h = slice_view(p.name, 0, 2); p = P { name: \"replaced\" }; print(h); }", "while it is borrowed"),
        ("struct P { name: string } fn main() { let p = P { name: \"hello\" }; let h = slice_view(p.name, 0, 2); let q = p; print(h); }", "while it is borrowed"),
        ("struct I { name: string } struct O { inner: I } fn main() { let o = O { inner: I { name: \"x\" } }; let h = slice_view(o.inner.name, 0, 1); print(h); }", "only on a struct variable"),
        ("struct P { count: i64 } fn main() { let p = P { count: 1 }; let h = slice_view(p.count, 0, 1); print(h); }", "slice_view expects"),
    ] {
        let out = execute(source);
        assert!(!out.status.success(), "accepted: {source}");
        let err = String::from_utf8_lossy(&out.stderr);
        assert!(err.contains(expected), "{source}: {err}");
    }
}
#[test]
fn fused_split_and_lines_keep_their_edge_cases() {
    // ADR-49 gives split/lines one fused arena. The scanning rules must not
    // change: left-to-right, non-overlapping multi-byte separators, empty
    // fields preserved, and no trailing empty line after a final newline.
    let output = execute("fn main() { let v = split(\"aaaa\", \"aa\"); print(len(v)); print(len(v[0])); print(len(v[2])); let w = split(\",a,\", \",\"); print(len(w)); print(w[0]); print(w[2]); let e = split(\"\", \",\"); print(len(e)); print(len(e[0])); }");
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    assert_eq!(String::from_utf8_lossy(&output.stdout).replace("\r\n", "\n"), "3\n0\n0\n3\n\n\n1\n0\n");
    let output = execute("fn main() { print(len(lines(\"\"))); print(len(lines(\"a\\n\"))); print(len(lines(\"\\n\"))); print(len(lines(\"a\\n\\nb\"))); let crlf = lines(\"a\\r\\nb\"); print(len(crlf)); print(len(crlf[0])); print(len(lines(\"a\"))); }");
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    assert_eq!(String::from_utf8_lossy(&output.stdout).replace("\r\n", "\n"), "0\n1\n1\n3\n2\n2\n1\n");
    // Destroying arena elements (pop, store) and growing past the exact
    // capacity (push) must not free arena memory or leak the replacement.
    let output = execute("fn main() { var v = split(\"a b c d e\", \" \"); pop(v); print(len(v)); v[0] = \"zz\"; print(v[0]); push(v, \"tail\"); print(v[4]); print(join(v, \"|\")); }");
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    assert_eq!(String::from_utf8_lossy(&output.stdout).replace("\r\n", "\n"), "4\nzz\ntail\nzz|b|c|d|tail\n");
    // An empty lines() result has exact capacity 0; pushing must grow the
    // pointer array instead of doubling zero bytes.
    let output = execute("fn main() { var v = lines(\"\"); print(len(v)); push(v, \"grown\"); print(len(v)); print(v[0]); var s = split(\"\", \",\"); push(s, \"b\"); print(len(s)); print(s[1]); }");
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    assert_eq!(String::from_utf8_lossy(&output.stdout).replace("\r\n", "\n"), "0\n1\ngrown\n2\nb\n");
}
#[test]
fn strvec_elements_are_borrow_only_and_push_pop_work() {
    let source = "fn main() { var v = split(\"one,two\", \",\"); print(len(v)); push(v, \"three\"); print(len(v)); print(v[2]); pop(v); print(len(v)); v[0] = \"ONE\"; print(v[0]); print(contains(v[1], \"wo\")); }";
    let output = execute(source);
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    assert_eq!(String::from_utf8_lossy(&output.stdout).replace("\r\n", "\n"), "2\n3\nthree\n2\nONE\ntrue\n");
    // A borrowed element may be concatenated: the result copies it and the
    // element stays valid and owned by the vector.
    let joined = execute("fn main() { let v = split(\"a\", \",\"); let s = v[0] + \"!\"; print(s); print(v[0]); }");
    assert!(joined.status.success(), "{}", String::from_utf8_lossy(&joined.stderr));
    assert_eq!(String::from_utf8_lossy(&joined.stdout).replace("\r\n", "\n"), "a!\na\n");
    for source in [
        "fn main() { var v = split(\"a\", \",\"); let w = v[0]; pop(v); print(len(w)); }",
        "fn main() { let v = split(\"a\", \",\"); let w = v[0]; let x = v; }",
        "fn main() { let v = split(\"a\", \",\"); print(v); }",
        "fn split(s: string, x: string) -> i64 { return 1; } fn main() {}",
        "fn args() -> i64 { return 1; } fn main() {}",
    ] {
        let path = std::env::temp_dir().join(format!("ae-sv-invalid-{}-{}.ae", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed)));
        fs::write(&path, source).unwrap();
        let output = Command::new(env!("CARGO_BIN_EXE_ae")).arg("check").arg(&path).output().unwrap();
        fs::remove_file(path).unwrap();
        assert!(!output.status.success(), "{source}");
    }
}
#[test]
fn slice_views_are_zero_copy_and_borrow_tracked() {
    let source = "fn main() { let t = \"the quick brown fox\"; let q = slice_view(t, 4, 9); print(q); print(len(q)); print(contains(q, \"quick\")); print(q == \"quick\"); let head = slice_view(t, 0, 3); print(head); print(slice(head, 0, 2)); }";
    let output = execute(source);
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    assert_eq!(String::from_utf8_lossy(&output.stdout).replace("\r\n", "\n"), "quick\n5\ntrue\ntrue\nthe\nth\n");
    // Out-of-bounds views fail at runtime with the source location.
    let oob = execute("fn main() { let t = \"abc\"; let v = slice_view(t, 0, 9); print(len(v)); }");
    assert!(!oob.status.success());
    assert!(String::from_utf8_lossy(&oob.stderr).contains("string index out of bounds"));
    for source in [
        // moving or reassigning a borrowed base is an error
        "fn main() { let t = \"abc\"; let v = slice_view(t, 0, 1); let w = t; print(len(v)); }",
        "fn main() { var t = \"abc\"; let v = slice_view(t, 0, 1); t = \"x\"; print(len(v)); }",
        // slice_view needs a bare string variable
        "fn main() { let v = slice_view(\"abc\", 0, 1); print(len(v)); }",
        // a view cannot be moved into a function that owns its argument
        "fn take(s: string) -> i64 { return len(s); } fn main() { let t = \"abc\"; let v = slice_view(t, 0, 1); print(take(v)); }",
        // views cannot be returned (no lifetime tracking across functions)
        "fn grab() -> ? { let t = \"abc\"; return slice_view(t, 0, 1); } fn main() {}",
    ] {
        let path = std::env::temp_dir().join(format!("ae-view-invalid-{}-{}.ae", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed)));
        fs::write(&path, source).unwrap();
        let output = Command::new(env!("CARGO_BIN_EXE_ae")).arg("check").arg(&path).output().unwrap();
        fs::remove_file(path).unwrap();
        assert!(!output.status.success(), "{source}");
    }
}
#[test]
fn for_in_iterates_string_vector_elements_as_views() {
    let source = "fn main() { let parts = split(\"alpha,beta,gamma\", \",\"); var total = 0; for w in parts { total = total + len(w); print(w); if contains(w, \"eta\") { print(1); } else { print(0); } } print(total); for w in parts { if w == \"beta\" { print(w); } } }";
    let output = execute(source);
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    assert_eq!(String::from_utf8_lossy(&output.stdout).replace("\r\n", "\n"), "alpha\n0\nbeta\n1\ngamma\n0\n14\nbeta\n");
    // The container cannot be modified or moved while iterated.
    for source in [
        "fn main() { let v = split(\"a,b\", \",\"); for w in v { push(v, \"x\"); } }",
        "fn main() { let v = split(\"a,b\", \",\"); for w in v { let held = v; } }",
        "fn main() { let v = split(\"a,b\", \",\"); for w in v { pop(v); } }",
    ] {
        let path = std::env::temp_dir().join(format!("ae-forin-sv-invalid-{}-{}.ae", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed)));
        fs::write(&path, source).unwrap();
        let output = Command::new(env!("CARGO_BIN_EXE_ae")).arg("check").arg(&path).output().unwrap();
        fs::remove_file(path).unwrap();
        assert!(!output.status.success(), "{source}");
    }
}
#[test]
fn lines_splits_on_newlines_like_common_stdlibs() {
    let source = "fn main() { let l = lines(\"a\\nbb\\nccc\"); print(len(l)); print(l[0]); print(l[2]); let t = lines(\"x\\n\"); print(len(t)); let e = lines(\"\"); print(len(e)); let m = lines(\"\\n\\n\"); print(len(m)); let u = lines(\"中\\n文\"); print(u[0]); print(join(l, \"+\")); }";
    let output = execute(source);
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    assert_eq!(String::from_utf8_lossy(&output.stdout).replace("\r\n", "\n"), "3\na\nccc\n1\n0\n2\n中\na+bb+ccc\n");
    // Trailing newline produces no final empty line; interior empties remain.
    let output = execute("fn main() { let l = lines(\"x\\n\\ny\"); print(len(l)); print(l[1]); }");
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    assert_eq!(String::from_utf8_lossy(&output.stdout).replace("\r\n", "\n"), "3\n\n");
    // lines() is reserved.
    let path = std::env::temp_dir().join(format!("ae-lines-invalid-{}-{}.ae", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed)));
    fs::write(&path, "fn lines(s: string) -> i64 { return 1; } fn main() {}").unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_ae")).arg("check").arg(&path).output().unwrap();
    fs::remove_file(path).unwrap();
    assert!(!output.status.success());
}
#[test]
fn count_lines_with_matches_reference_semantics() {
    // Reference: split into lines (trailing newline yields no final empty),
    // count lines whose content contains the pattern.
    let nl = "\n";
    let cases = [
        (format!("ae here{nl}nope{nl}ae again"), "ae", 2),
        (String::new(), "x", 0),
        ("one".to_string(), "two", 0),
        ("yes".to_string(), "", 1),
        (format!("x{nl}{nl}y"), "", 3),
        (format!("a{nl}b{nl}"), "b", 1),
        (format!("aaa{nl}aa{nl}a"), "aa", 2),
        (format!("{nl}{nl}"), "z", 0),
    ];
    fn ae_lit(t: &str) -> String {
        t.replace('\\', "\\\\").replace('"', "\\\"").replace('\n', "\\n")
    }
    for (text, pat, expected) in cases {
        let src_text = ae_lit(&text);
        let src_pat = ae_lit(&pat);
        let source = format!("fn main() {{ print(count_lines_with(\"{src_text}\", \"{src_pat}\")); }}");
        let output = execute(&source);
        assert!(output.status.success(), "{source}: {}", String::from_utf8_lossy(&output.stderr));
        assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), expected.to_string(), "{text:?} / {pat:?}");
    }
}
#[test]
fn cli_grep_reads_pattern_and_file_from_args() {
    let dir = std::env::temp_dir().join(format!("ae-cli-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir(&dir).unwrap();
    let src = dir.join("grep_cli.ae");
    fs::copy(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("examples/grep_cli.ae"), &src).unwrap();
    let data = dir.join("data.txt");
    fs::write(&data, "hit one\nmiss\nhit two\n").unwrap();
    let executable = dir.join(if cfg!(windows) { "g.exe" } else { "g" });
    let result = Command::new(env!("CARGO_BIN_EXE_ae")).arg("build").arg(&src).arg("-o").arg(&executable).output().unwrap();
    assert!(result.status.success(), "{}", String::from_utf8_lossy(&result.stderr));
    let run = |args: &[&str]| Command::new(&executable).args(args).current_dir(&dir).output().unwrap();
    let hit = run(&["hit", "data.txt"]);
    assert!(hit.status.success());
    assert_eq!(String::from_utf8_lossy(&hit.stdout).replace("\r\n", "\n").trim(), "2");
    let miss = run(&["zzz", "data.txt"]);
    assert_eq!(String::from_utf8_lossy(&miss.stdout).replace("\r\n", "\n").trim(), "0");
    let usage = run(&[]);
    assert!(String::from_utf8_lossy(&usage.stdout).contains("usage"));
    let _ = fs::remove_dir_all(&dir);
}
#[test]
fn vec_borrows_are_read_only_and_forwardable() {
    // Borrowed vectors forward like array borrows; mutation and moves are rejected.
    let source = "fn total(v: &vec) -> i64 { var t = 0; for i in 0..len(v) { t = t + v[i]; } return t; } fn last_two(v: &vec) -> i64 { return v[len(v) - 2] + v[len(v) - 1]; } fn main() { var data = vec[1, 2, 3]; print(total(&data)); print(last_two(&data)); data[0] = 10; print(total(&data)); }";
    let output = execute(source);
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    assert_eq!(String::from_utf8_lossy(&output.stdout).replace("\r\n", "\n"), "6\n5\n15\n");
    for source in [
        "fn f(v: &vec) -> i64 { push(v, 1); return 0; } fn main() { var d = vec[1]; print(f(&d)); }",
        "fn f(v: &vec) -> i64 { v[0] = 5; return 0; } fn main() { var d = vec[1]; print(f(&d)); }",
        "fn f(v: &vec) -> i64 { let w = *v; return 0; } fn main() { var d = vec[1]; print(f(&d)); }",
        "fn f(v: &vec) -> i64 { return 0; } fn main() { print(f(vec[1])); }",
        "fn f(v: &vec) -> i64 { return 0; } fn main() { let x = 1; print(f(&x)); }",
    ] {
        let path = std::env::temp_dir().join(format!("ae-vecref-invalid-{}-{}.ae", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed)));
        fs::write(&path, source).unwrap();
        let output = Command::new(env!("CARGO_BIN_EXE_ae")).arg("check").arg(&path).output().unwrap();
        fs::remove_file(path).unwrap();
        assert!(!output.status.success(), "{source}");
    }
}
fn check_rejects(source: &str) -> String {
    let path = std::env::temp_dir().join(format!("ae-enum-invalid-{}-{}.ae", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed)));
    fs::write(&path, source).unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_ae")).arg("check").arg(&path).output().unwrap();
    fs::remove_file(path).unwrap();
    assert!(!output.status.success(), "accepted invalid program: {source}");
    String::from_utf8_lossy(&output.stderr).into_owned()
}
#[test]
fn enums_match_payloads_execute() {
    let source = "enum Shape { Circle(i64), Rect(i64, i64), Empty }
enum Msg { Text(string), Flag(bool) }
fn area(s: Shape) -> i64 { match s { Shape::Circle(r) => { return 3 * r * r; } Shape::Rect(w, h) => { return w * h; } Shape::Empty => { return 0; } } }
fn render(m: Msg) -> string { match m { Msg::Text(t) => { return \"t:\" + slice(t, 0, len(t)); } Msg::Flag(b) => { if b { return \"yes\"; } return \"no\"; } } }
fn main() {
    print(area(Shape::Circle(3)));
    print(area(Shape::Rect(4, 5)));
    print(area(Shape::Empty));
    let m = Msg::Text(\"hi\");
    print(render(m));
    print(render(Msg::Flag(false)));
}";
    let output = execute(source);
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    assert_eq!(String::from_utf8_lossy(&output.stdout).replace("\r\n", "\n"), "27\n20\n0\nt:hi\nno\n");
}
#[test]
fn match_arms_do_not_capture_loop_control_flow() {
    // An arm body's break/continue must leave the enclosing loop, not the match.
    let source = "enum Signal { Stop, Skip, Go(i64) }
fn main() {
    var i = 0;
    var total = 0;
    while i < 10 {
        i = i + 1;
        var signal = Signal::Go(i);
        if i == 2 { signal = Signal::Skip; }
        if i == 4 { signal = Signal::Stop; }
        match signal {
            Signal::Stop => { break; }
            Signal::Skip => { continue; }
            Signal::Go(n) => { total = total + n; }
        }
    }
    print(i);
    print(total);
}";
    let output = execute(source);
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    // i stops at 4 (stop); total collects 1 + 3 = 4 (2 skipped, 4 breaks first).
    assert_eq!(String::from_utf8_lossy(&output.stdout).replace("\r\n", "\n"), "4\n4\n");
}
#[test]
fn enum_values_move_and_assign_deterministically() {
    let source = "enum Slot { Empty, Text(string) }
fn describe(s: Slot) -> string { match s { Slot::Empty => { return \"empty\"; } Slot::Text(t) => { return \"text \" + slice(t, 0, len(t)); } } }
fn main() {
    var slot = Slot::Text(\"one\");
    slot = Slot::Text(\"two\");
    print(describe(slot));
    slot = Slot::Empty;
    print(describe(slot));
}";
    let output = execute(source);
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    assert_eq!(String::from_utf8_lossy(&output.stdout).replace("\r\n", "\n"), "text two\nempty\n");
}
#[test]
fn invalid_enum_programs_are_rejected() {
    let cases = [
        ("enum E { A, B } fn main() { let x = E::A; match x { E::A => { print(1); } } }", "not exhaustive"),
        ("enum E { A, B } fn main() { let x = E::A; match x { E::A => { print(1); } E::A => { print(2); } E::B => { print(3); } } }", "matched twice"),
        ("enum E { A(i64) } fn main() { let x = E::A(1, 2); print(1); }", "payload"),
        ("enum E { A(i64) } fn main() { let x = E::A; print(1); }", "payload"),
        ("enum E { A(i64) } fn main() { let x = E::A(1); match x { E::A => { print(1); } } }", "binding"),
        ("enum E { A } fn main() { let x = E::A; match x { F::A => { print(1); } } }", "pattern must name"),
        ("enum E { A } enum E { B } fn main() { print(1); }", "duplicate enum"),
        ("enum E { A, A } fn main() { print(1); }", "duplicate variant"),
        ("enum E { A } fn main() { let E = 1; print(E); }", "names an enum"),
        ("enum E { A } fn main() { let x = 1; match x { E::A => { print(1); } } }", "bare enum variable"),
        ("enum E { A } fn main() { let x = E::A; let y = x; match x { E::A => { print(1); } } }", "moved"),
        ("enum E { A(string) } fn take(s: string) -> i64 { return len(s); } fn main() { let x = E::A(\"s\"); match x { E::A(t) => { print(take(t)); } } }", "must be string"),
    ];
    for (source, expected) in cases {
        let message = check_rejects(source);
        assert!(message.contains(expected), "{source}\n  expected '{expected}', got '{message}'");
    }
}

#[test]
fn prelude_results_report_recoverable_errors() {
    let source = r#"fn main() {
    let parsed = parse_int("42");
    match parsed {
        IntResult::Ok(value) => { print(value + 1); }
        IntResult::Err(reason) => { print("bad"); }
    }
    let rejected = parse_int("12x");
    match rejected {
        IntResult::Ok(value) => { print(value); }
        IntResult::Err(reason) => { print(len(reason) > 0); }
    }
    let empty = parse_int("");
    match empty {
        IntResult::Ok(value) => { print(value); }
        IntResult::Err(reason) => { print("empty rejected"); }
    }
    let huge = parse_int("9223372036854775808");
    match huge {
        IntResult::Ok(value) => { print(value); }
        IntResult::Err(reason) => { print("overflow rejected"); }
    }
    let lowest = parse_int("-9223372036854775808");
    match lowest {
        IntResult::Ok(value) => { print(value + 1); }
        IntResult::Err(reason) => { print("bad"); }
    }
    print(to_string(0 - 1234));
}"#;
    let output = execute(source);
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    assert_eq!(String::from_utf8_lossy(&output.stdout).replace("\r\n", "\n"), "43\ntrue\nempty rejected\noverflow rejected\n-9223372036854775807\n-1234\n");
}
#[test]
fn fallible_file_io_reports_errors_as_values() {
    let dir = std::env::temp_dir().join(format!("ae-fallible-{}-{}", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed)));
    fs::create_dir(&dir).unwrap();
    let data = dir.join("data.txt");
    let summary = dir.join("summary.txt");
    let missing = dir.join("missing.txt");
    let literal = |p: &std::path::Path| p.display().to_string().replace('\\', "\\\\");
    let source = format!(r#"fn main() {{
    let opened = try_read_file("{data}");
    match opened {{
        ReadResult::Ok(text) => {{ print(len(text)); }}
        ReadResult::Err(reason) => {{ print("unexpected failure"); }}
    }}
    let absent = try_read_file("{missing}");
    match absent {{
        ReadResult::Ok(text) => {{ print("unexpected success"); }}
        ReadResult::Err(reason) => {{ print(len(reason) > 0); }}
    }}
    let saved = try_write_file("{summary}", "ok");
    match saved {{
        WriteResult::Ok(written) => {{ print(written); }}
        WriteResult::Err(reason) => {{ print("write failed"); }}
    }}
}}"#, data = literal(&data), missing = literal(&missing), summary = literal(&summary));
    fs::write(&data, "hello").unwrap();
    let output = execute(&source);
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    assert_eq!(String::from_utf8_lossy(&output.stdout).replace("\r\n", "\n"), "5\ntrue\n2\n");
    assert_eq!(fs::read_to_string(&summary).unwrap(), "ok");
    fs::remove_dir_all(dir).unwrap();
}
#[test]
fn concatenation_copies_borrowed_strings() {
    // A borrowed string (field, element, view) must be copied by `+` and stay
    // valid: handing it to str_cat directly would double-free its buffer.
    let source = r#"struct Label { text: string }
fn main() {
    let label = Label { text: "field" };
    print(label.text + "/tail");
    print(label.text);
    let base = "abcdef";
    let view = slice_view(base, 1, 4);
    print("[" + view + "]");
    print(view);
    let parts = split("a-b-c", "-");
    print(parts[0] + "!" + parts[2]);
    print(parts[1]);
}"#;
    let output = execute(source);
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    assert_eq!(String::from_utf8_lossy(&output.stdout).replace("\r\n", "\n"), "field/tail\nfield\n[bcd]\nbcd\na!c\nb\n");
}
#[test]
fn invalid_prelude_usage_is_rejected() {
    for source in [
        "fn main() { let r = try_read_file(1); print(1); }",
        "fn main() { let r = try_write_file(\"a\", 1); print(1); }",
        "fn main() { let r = parse_int(1); print(1); }",
        "fn main() { print(to_string(\"x\")); }",
        "fn main() { print(to_string(true)); }",
        "fn main() { let r = try_read_file(\"a\", \"b\"); print(1); }",
        "enum ReadResult { A } fn main() { print(1); }",
    ] {
        let path = std::env::temp_dir().join(format!("ae-prelude-invalid-{}-{}.ae", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed)));
        fs::write(&path, source).unwrap();
        let output = Command::new(env!("CARGO_BIN_EXE_ae")).arg("check").arg(&path).output().unwrap();
        fs::remove_file(path).unwrap();
        assert!(!output.status.success(), "accepted invalid program: {source}");
    }
}

#[test]
fn directory_traversal_is_sorted_and_reports_errors() {
    let root = std::env::temp_dir().join(format!("ae-dir-{}-{}", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed)));
    fs::create_dir_all(root.join("nested")).unwrap();
    fs::write(root.join("b.txt"), "b").unwrap();
    fs::write(root.join("a.txt"), "a").unwrap();
    fs::write(root.join("nested/c.txt"), "c").unwrap();
    let missing = root.join("absent");
    let literal = |p: &std::path::Path| p.display().to_string().replace('\\', "\\\\");
    let source = format!(r#"fn main() {{
    let listed = try_list_dir("{root}");
    match listed {{
        DirResult::Ok(names) => {{
            print(len(names));
            for name in names {{
                print(name);
                print(path_is_dir("{root}/" + name));
            }}
        }}
        DirResult::Err(reason) => {{ print("unexpected failure"); }}
    }}
    let absent = try_list_dir("{missing}");
    match absent {{
        DirResult::Ok(names) => {{ print("unexpected success"); }}
        DirResult::Err(reason) => {{ print(len(reason) > 0); }}
    }}
    print(path_exists("{root}/a.txt"));
    print(path_exists("{missing}"));
}}"#, root = literal(&root), missing = literal(&missing));
    let output = execute(&source);
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    // Entries are sorted by bytes: a.txt, b.txt, nested.
    assert_eq!(String::from_utf8_lossy(&output.stdout).replace("\r\n", "\n"), "3\na.txt\nfalse\nb.txt\nfalse\nnested\ntrue\ntrue\ntrue\nfalse\n");
    fs::remove_dir_all(root).unwrap();
}
#[test]
fn read_only_string_parameters_do_not_consume_their_arguments() {
    let source = r#"fn label(text: &string) -> string {
    return "[" + text + "]";
}
fn twice(text: &string) -> i64 {
    print(label(text));
    print(label(text));
    return len(text);
}
fn main() {
    let name = "hello";
    print(twice(name));
    print(name);
    print(twice("literal"));
    let base = "abcdef";
    let view = slice_view(base, 1, 3);
    print(twice(view));
}"#;
    let output = execute(source);
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    assert_eq!(String::from_utf8_lossy(&output.stdout).replace("\r\n", "\n"), "[hello]\n[hello]\n5\nhello\n[literal]\n[literal]\n7\n[bc]\n[bc]\n2\n");
}
#[test]
fn borrowed_string_vectors_can_be_iterated_and_read() {
    let source = r#"fn count(parts: &vec[string]) -> i64 {
    return len(parts);
}
fn longest(parts: &vec[string]) -> i64 {
    var best = 0;
    for part in parts {
        if len(part) > best { best = len(part); }
    }
    return best;
}
fn main() {
    let parts = split("aa,bbb,c", ",");
    print(count(&parts));
    print(longest(&parts));
    print(parts[1]);
}"#;
    let output = execute(source);
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    assert_eq!(String::from_utf8_lossy(&output.stdout).replace("\r\n", "\n"), "3\n3\nbbb\n");
}
#[test]
fn invalid_filesystem_and_borrow_usage_is_rejected() {
    for source in [
        "fn main() { print(try_list_dir(1)); }",
        "fn main() { print(path_exists(1)); }",
        "fn main() { print(path_is_dir(1)); }",
        // a read-only parameter cannot be returned or handed on as owned
        "fn grab(s: &string) -> string { return s; } fn main() { print(len(grab(\"x\"))); }",
        "fn take(s: string) -> i64 { return len(s); } fn own(s: &string) -> i64 { return take(s); } fn main() { print(own(\"x\")); }",
        // a vec is not a string
        "fn f(s: &string) -> i64 { return len(s); } fn main() { let v = vec[1]; print(f(v)); }",
        // a borrowed string vector cannot be modified
        "fn f(parts: &vec[string]) -> i64 { push(parts, \"x\"); return 0; } fn main() { let p = split(\"a\", \",\"); print(f(&p)); }",
        "fn f(parts: &vec[string]) -> i64 { let q = parts; return 0; } fn main() { let p = split(\"a\", \",\"); print(f(&p)); }",
    ] {
        let path = std::env::temp_dir().join(format!("ae-fs-invalid-{}-{}.ae", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed)));
        fs::write(&path, source).unwrap();
        let output = Command::new(env!("CARGO_BIN_EXE_ae")).arg("check").arg(&path).output().unwrap();
        fs::remove_file(path).unwrap();
        assert!(!output.status.success(), "accepted invalid program: {source}");
    }
}
#[test]
fn path_algebra_joins_names_and_parents() {
    // Separator handling is uniform: '/' is emitted on both platforms so the
    // same program prints the same paths everywhere.
    let source = r#"fn main() {
    print(path_join("dir", "name.txt"));
    print(path_join("dir/", "name.txt"));
    print(path_join("dir", "/abs.txt"));
    print(path_join("", "name.txt"));
    print(path_join("dir", ""));
    print("[" + path_name("a/b/c.txt") + "]");
    print("[" + path_name("a/b/") + "]");
    print("[" + path_name("plain") + "]");
    print("[" + path_parent("a/b/c.txt") + "]");
    print("[" + path_parent("a/b") + "]");
    print("[" + path_parent("plain") + "]");
    print("[" + path_parent("/a") + "]");
}"#;
    let output = execute(source);
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).replace("\r\n", "\n"),
        "dir/name.txt\ndir/name.txt\n/abs.txt\nname.txt\ndir\n[c.txt]\n[b]\n[plain]\n[a/b]\n[a]\n[]\n[/]\n"
    );
}
#[test]
fn file_metadata_reports_size_and_missing_paths() {
    let dir = std::env::temp_dir().join(format!("ae-meta-{}-{}", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed)));
    fs::create_dir(&dir).unwrap();
    let data = dir.join("data.txt");
    let missing = dir.join("missing.txt");
    let literal = |p: &std::path::Path| p.display().to_string().replace('\\', "\\\\");
    let source = format!(r#"fn main() {{
    let found = try_file_info("{data}");
    match found {{
        FileResult::Ok(size, modified, is_dir) => {{
            print(size);
            print(is_dir);
            print(modified > 0);
        }}
        FileResult::Err(reason) => {{ print("unexpected failure"); }}
    }}
    let folder = try_file_info("{dir}");
    match folder {{
        FileResult::Ok(size, modified, is_dir) => {{ print(is_dir); }}
        FileResult::Err(reason) => {{ print("unexpected failure"); }}
    }}
    let absent = try_file_info("{missing}");
    match absent {{
        FileResult::Ok(size, modified, is_dir) => {{ print("unexpected success"); }}
        FileResult::Err(reason) => {{ print(len(reason) > 0); }}
    }}
}}"#, data = literal(&data), dir = literal(&dir), missing = literal(&missing));
    fs::write(&data, "hello world").unwrap();
    let output = execute(&source);
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    assert_eq!(String::from_utf8_lossy(&output.stdout).replace("\r\n", "\n"), "11\nfalse\ntrue\ntrue\ntrue\n");
    fs::remove_dir_all(dir).unwrap();
}
#[test]
fn file_operations_create_copy_rename_and_remove() {
    let dir = std::env::temp_dir().join(format!("ae-ops-{}-{}", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed)));
    fs::create_dir(&dir).unwrap();
    let work = dir.join("work");
    let nested = work.join("nested/deep");
    let literal = |p: &std::path::Path| p.display().to_string().replace('\\', "\\\\");
    let source = format!(r#"fn main() {{
    let made = try_create_dir("{nested}");
    match made {{
        StatusResult::Ok => {{ print("mkdir ok"); }}
        StatusResult::Err(reason) => {{ print("mkdir failed"); }}
    }}
    let again = try_create_dir("{nested}");
    match again {{
        StatusResult::Ok => {{ print("mkdir idempotent"); }}
        StatusResult::Err(reason) => {{ print("mkdir repeated failed"); }}
    }}
    let original = path_join("{nested}", "one.txt");
    assert(write_file(original, "payload") == 7);
    let copy = path_join("{nested}", "two.txt");
    let copied = try_copy_file(original, copy);
    match copied {{
        StatusResult::Ok => {{ print("copy ok"); }}
        StatusResult::Err(reason) => {{ print("copy failed"); }}
    }}
    let moved = path_join("{nested}", "three.txt");
    let renamed = try_rename_path(copy, moved);
    match renamed {{
        StatusResult::Ok => {{ print("rename ok"); }}
        StatusResult::Err(reason) => {{ print("rename failed"); }}
    }}
    // A directory is not a file: removal must refuse it instead of silently
    // deleting an empty tree on one platform only.
    let refused = try_remove_file("{work}");
    match refused {{
        StatusResult::Ok => {{ print("removed a directory"); }}
        StatusResult::Err(reason) => {{ print("directory refused"); }}
    }}
    let gone = try_remove_file(moved);
    match gone {{
        StatusResult::Ok => {{ print("remove ok"); }}
        StatusResult::Err(reason) => {{ print("remove failed"); }}
    }}
    let absent = try_remove_file(moved);
    match absent {{
        StatusResult::Ok => {{ print("removed twice"); }}
        StatusResult::Err(reason) => {{ print("missing reported"); }}
    }}
}}"#, nested = literal(&nested), work = literal(&work));
    let output = execute(&source);
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).replace("\r\n", "\n"),
        "mkdir ok\nmkdir idempotent\ncopy ok\nrename ok\ndirectory refused\nremove ok\nmissing reported\n"
    );
    // The original survived, the copy moved to its new name, and the moved
    // file was really removed.
    assert_eq!(fs::read_to_string(nested.join("one.txt")).unwrap(), "payload");
    assert!(!nested.join("two.txt").exists());
    assert!(!nested.join("three.txt").exists());
    fs::remove_dir_all(dir).unwrap();
}
#[test]
fn directory_removal_requires_an_empty_directory() {
    let dir = std::env::temp_dir().join(format!("ae-rmdir-{}-{}", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed)));
    fs::create_dir_all(dir.join("full")).unwrap();
    fs::create_dir_all(dir.join("empty")).unwrap();
    fs::write(dir.join("full/file.txt"), "x").unwrap();
    let literal = |p: &std::path::Path| p.display().to_string().replace('\\', "\\\\");
    let source = format!(r#"fn main() {{
    let busy = try_remove_dir("{full}");
    match busy {{
        StatusResult::Ok => {{ print("removed a non-empty directory"); }}
        StatusResult::Err(reason) => {{ print("non-empty refused"); }}
    }}
    let empty = try_remove_dir("{empty}");
    match empty {{
        StatusResult::Ok => {{ print("empty removed"); }}
        StatusResult::Err(reason) => {{ print("empty failed"); }}
    }}
}}"#, full = literal(&dir.join("full")), empty = literal(&dir.join("empty")));
    let output = execute(&source);
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    assert_eq!(String::from_utf8_lossy(&output.stdout).replace("\r\n", "\n"), "non-empty refused\nempty removed\n");
    assert!(dir.join("full/file.txt").exists());
    assert!(!dir.join("empty").exists());
    fs::remove_dir_all(dir).unwrap();
}
#[test]
fn invalid_path_helpers_are_rejected() {
    for source in [
        "fn main() { print(path_join(1, \"b\")); }",
        "fn main() { print(path_name(1)); }",
        "fn main() { print(path_parent(1)); }",
        "fn main() { let x = try_file_info(1); print(1); }",
        "fn main() { let x = try_copy_file(\"a\", 1); print(1); }",
        "fn main() { let x = try_rename_path(1, \"b\"); print(1); }",
        "fn main() { let x = try_remove_file(1); print(1); }",
        "fn main() { let x = try_create_dir(1); print(1); }",
        "fn main() { let x = try_remove_dir(1); print(1); }",
        "fn main() { let path_join = 1; }",
    ] {
        let path = std::env::temp_dir().join(format!("ae-path-invalid-{}-{}.ae", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed)));
        fs::write(&path, source).unwrap();
        let output = Command::new(env!("CARGO_BIN_EXE_ae")).arg("check").arg(&path).output().unwrap();
        fs::remove_file(path).unwrap();
        assert!(!output.status.success(), "accepted invalid program: {source}");
    }
}
#[test]
fn run_forwards_program_arguments_after_double_dash() {
    let dir = std::env::temp_dir().join(format!("ae-argv-{}-{}", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed)));
    fs::create_dir(&dir).unwrap();
    let source = dir.join("argv.ae");
    fs::write(&source, "fn main() { let a = args(); print(len(a) - 1); print(a[1]); }").unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_ae"))
        .arg("run").arg(&source).arg("--").arg("first").arg("second")
        .output().unwrap();
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    assert_eq!(String::from_utf8_lossy(&output.stdout).replace("\r\n", "\n"), "2\nfirst\n");
    // Only `run` starts a program, so passing arguments to another command is
    // an error rather than silently ignored.
    let rejected = Command::new(env!("CARGO_BIN_EXE_ae"))
        .arg("check").arg(&source).arg("--").arg("extra")
        .output().unwrap();
    assert!(!rejected.status.success());
    assert!(String::from_utf8_lossy(&rejected.stderr).contains("does not take program arguments"));
    fs::remove_dir_all(dir).unwrap();
}
#[test]
fn error_propagation_returns_the_first_failure() {
    let dir = std::env::temp_dir().join(format!("ae-propagate-{}-{}", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed)));
    fs::create_dir(&dir).unwrap();
    fs::write(dir.join("a.txt"), "alpha").unwrap();
    fs::write(dir.join("b.txt"), "beta").unwrap();
    let literal = |p: &std::path::Path| p.display().to_string().replace('\\', "\\\\");
    let source = format!(r#"fn merge(first: &string, second: &string) -> ReadResult {{
    let a = try_read_file(first)?;
    let b = try_read_file(second)?;
    return ReadResult::Ok(a + b);
}}

fn double(text: &string) -> IntResult {{
    let n = parse_int(text)?;
    return IntResult::Ok(n * 2);
}}

fn remove_missing(path: &string) -> StatusResult {{
    try_remove_file(path)?;
    return StatusResult::Ok;
}}

fn main() {{
    let both = merge("{a}", "{b}");
    match both {{
        ReadResult::Ok(text) => {{ print(text); }}
        ReadResult::Err(reason) => {{ print("unexpected failure"); }}
    }}
    let half = merge("{a}", "{missing}");
    match half {{
        ReadResult::Ok(text) => {{ print("unexpected success"); }}
        ReadResult::Err(reason) => {{ print(len(reason) > 0); }}
    }}
    let doubled = double("21");
    match doubled {{
        IntResult::Ok(n) => {{ print(n); }}
        IntResult::Err(reason) => {{ print("unexpected failure"); }}
    }}
    let rejected = double("21x");
    match rejected {{
        IntResult::Ok(n) => {{ print("unexpected success"); }}
        IntResult::Err(reason) => {{ print(reason); }}
    }}
    let absent = remove_missing("{missing2}");
    match absent {{
        StatusResult::Ok => {{ print("unexpected success"); }}
        StatusResult::Err(reason) => {{ print(len(reason) > 0); }}
    }}
}}"#, a = literal(&dir.join("a.txt")), b = literal(&dir.join("b.txt")),
        missing = literal(&dir.join("missing.txt")), missing2 = literal(&dir.join("gone.txt")));
    let output = execute(&source);
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).replace("\r\n", "\n"),
        "alphabeta\ntrue\n42\nnot a base-10 integer\ntrue\n"
    );
    fs::remove_dir_all(dir).unwrap();
}
#[test]
fn invalid_error_propagation_is_rejected() {
    for source in [
        // '?' needs a function that returns the same result type
        "fn main() { let a = try_read_file(\"x\")?; print(1); }",
        "fn f(p: &string) -> StatusResult { let names = try_list_dir(p)?; return StatusResult::Ok; } fn main() { print(1); }",
        // the value has to be a result at all
        "fn f() -> ReadResult { let x = 1?; return ReadResult::Ok(\"a\"); } fn main() { print(1); }",
        // only the prelude-shaped Ok/Err pairs can be propagated
        "enum Pair { Ok(string), Bad(string) } fn f() -> Pair { let x = try_read_file(\"a\")?; return Pair::Ok(x); } fn main() { print(1); }",
        // a three-value Ok needs an explicit match, a binding cannot pick one
        "fn f(p: &string) -> FileResult { let size = try_file_info(p)?; return FileResult::Ok(size, 0, false); } fn main() { print(1); }",
        // a bare call statement must state what happens to the result
        "fn f(p: &string) -> StatusResult { try_remove_file(p); return StatusResult::Ok; } fn main() { print(1); }",
    ] {
        let path = std::env::temp_dir().join(format!("ae-propagate-invalid-{}-{}.ae", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed)));
        fs::write(&path, source).unwrap();
        let output = Command::new(env!("CARGO_BIN_EXE_ae")).arg("check").arg(&path).output().unwrap();
        fs::remove_file(path).unwrap();
        assert!(!output.status.success(), "accepted invalid program: {source}");
    }
}
#[test]
fn float_maps_store_get_and_report_keys_like_int_maps() {
    // ADR-65: map[string, f64] mirrors map[string, i64] — put/index-write,
    // map_get into FloatResult (missing is the shared "key not found" text),
    // has/remove/keys/len and map_inc with a float delta.
    let source = "fn main() { var m = f64map_new(); m[\"pi\"] = 3.5; print(len(m)); let got = map_get(m, \"pi\"); match got { FloatResult::Ok(v) => { print(v); } FloatResult::Err(reason) => { print(reason); } } print(map_has(m, \"pi\")); print(map_has(m, \"nope\")); print(map_inc(m, \"pi\", 0.5)); print(map_inc(m, \"fresh\", 1.0)); m[\"zed\"] = 2.0; let ks = keys(m); for k in ks { print(k); } let removed = map_remove(m, \"fresh\"); print(removed); print(len(m)); }";
    let output = execute(source);
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    assert_eq!(String::from_utf8_lossy(&output.stdout).replace("\r\n", "\n"), "1\n3.5\ntrue\nfalse\n4.0\n1.0\nfresh\npi\nzed\ntrue\n2\n");
}
#[test]
fn float_maps_reject_wrong_value_types_and_move_like_int_maps() {
    for (source, expected) in [
        ("fn main() { var m = f64map_new(); m[\"a\"] = 1; }", "map values must be f64"),
        ("fn main() { var m = map_new(); m[\"a\"] = 1.0; }", "map values must be i64"),
        ("fn main() { var m = f64map_new(); print(map_inc(m, \"a\", 1)); }", "map_inc expects a f64 delta"),
        ("fn main() { var m = f64map_new(); let g = map_get(m, \"a\"); match g { IntResult::Ok(v) => { print(v); } IntResult::Err(r) => { print(r); } } }", "pattern must name the matched enum"),
        ("fn main() { var m = f64map_new(); let w = m; print(map_has(m, \"a\")); }", "moved"),
        ("fn main() { var m = map[string, f64]; }", "unknown variable"),
    ] {
        let message = check_rejects(source);
        assert!(message.contains(expected), "{source}: {message}");
    }
}
#[test]
fn float_maps_live_in_structs_and_release_exactly_once() {
    let source = "struct Prices { table: map[string, f64], name: string } fn main() { var p = Prices { table: f64map_new(), name: \"menu\" }; p.table[\"tea\"] = 2.5; p.table[\"cake\"] = 7.0; print(p.name); print(len(p.table)); let t = map_get(p.table, \"tea\"); match t { FloatResult::Ok(v) => { print(v); } FloatResult::Err(r) => { print(r); } } print(\"done\"); }";
    let output = execute(source);
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    assert_eq!(String::from_utf8_lossy(&output.stdout).replace("\r\n", "\n"), "menu\n2\n2.5\ndone\n");
}
#[test]
fn maps_store_count_replace_and_report_keys_in_order() {
    let output = execute(r#"fn main() {
    var counts = map_new();
    counts["b"] = 2;
    counts["a"] = 1;
    counts["b"] = 5;
    print(len(counts));
    print(map_has(counts, "a"));
    print(map_has(counts, "zz"));
    let got = map_get(counts, "b");
    match got { IntResult::Ok(n) => { print(n); } IntResult::Err(reason) => { print("unexpected failure"); } }
    let absent = map_get(counts, "zz");
    match absent { IntResult::Ok(n) => { print("unexpected success"); } IntResult::Err(reason) => { print(reason); } }
    let names = keys(counts);
    for name in names { print(name); }
    print(map_remove(counts, "a"));
    print(map_remove(counts, "a"));
    print(len(counts));
}"#);
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).replace("\r\n", "\n"),
        "2\ntrue\nfalse\n5\nkey not found\na\nb\ntrue\nfalse\n1\n"
    );
}
#[test]
fn missing_map_key_text_survives_every_owned_string_path() {
    // ADR-58: a miss hands out fixed text in read-only data instead of a fresh
    // allocation. Match binds the payload as a view, so the owned holder is
    // the result value itself: this exercises reads, concatenation on either
    // side, copying into a string vector, moving the whole result into a
    // function, and `?` propagation across a function boundary.
    let output = execute(r#"fn consume(r: IntResult) -> i64 {
    match r { IntResult::Ok(n) => { return n; } IntResult::Err(e) => { return len(e); } }
}
fn propagated() -> IntResult {
    var m = map_new();
    let n = map_get(m, "absent")?;
    return IntResult::Ok(n);
}
fn main() {
    var m = map_new();
    m["present"] = 7;
    for i in 0..2000 {
        let r = map_get(m, "absent");
        match r {
            IntResult::Ok(n) => { print("unexpected"); }
            IntResult::Err(e) => {
                if i == 0 { print(e); print(len(e)); print(e == "key not found"); print(e); }
            }
        }
    }
    let left = map_get(m, "absent");
    match left { IntResult::Ok(n) => { print("unexpected"); } IntResult::Err(e) => { print("prefix: " + e); } }
    let right = map_get(m, "absent");
    match right { IntResult::Ok(n) => { print("unexpected"); } IntResult::Err(e) => { print(e + " suffix"); } }
    let moved = map_get(m, "absent");
    print(consume(moved));
    let collected = map_get(m, "absent");
    match collected {
        IntResult::Ok(n) => { print("unexpected"); }
        IntResult::Err(e) => { var notes = strvec_new(); push(notes, e); print(notes[0]); print(len(notes)); }
    }
    let p = propagated();
    match p { IntResult::Ok(n) => { print("unexpected"); } IntResult::Err(e) => { print(e); } }
    let hit = map_get(m, "present");
    match hit { IntResult::Ok(n) => { print(n); } IntResult::Err(e) => { print("unexpected"); } }
}"#);
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).replace("\r\n", "\n"),
        "key not found\n13\ntrue\nkey not found\nprefix: key not found\nkey not found suffix\n13\nkey not found\n1\nkey not found\n7\n"
    );
}
#[test]
fn keys_after_removal_skips_tombstones_and_sees_reused_slots() {
    // The state byte decides what keys() reports, so a tombstone must be invisible
    // and a slot reused by a later insert must show up exactly once (ADR-57 moved
    // the state out of the slot record; this pins the observable consequence).
    let output = execute(r#"fn main() {
    var m = map_new();
    m["a"] = 1;
    m["b"] = 2;
    m["c"] = 3;
    print(map_remove(m, "b"));
    print(len(m));
    let first = keys(m);
    for k in first { print(k); }
    m["d"] = 4;
    m["b"] = 5;
    let second = keys(m);
    for k in second { print(k); }
    print(len(m));
}"#);
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).replace("\r\n", "\n"),
        "true\n2\na\nc\na\nb\nc\nd\n4\n"
    );
}

#[test]
fn maps_survive_storage_in_struct_fields() {
    let output = execute(r#"struct Tally { label: string, counts: map[string, i64] }
fn record(label: &string) -> Tally {
    var counts = map_new();
    counts["seen"] = 1;
    counts["seen"] = 2;
    return Tally { label: slice(label, 0, len(label)), counts: counts };
}
fn main() {
    let t = record("day");
    print(t.label);
    print(len(t.counts));
    print(map_has(t.counts, "seen"));
    let got = map_get(t.counts, "seen");
    match got { IntResult::Ok(n) => { print(n); } IntResult::Err(reason) => { print("unexpected failure"); } }
    let names = keys(t.counts);
    print(names[0]);
}"#);
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    assert_eq!(String::from_utf8_lossy(&output.stdout).replace("\r\n", "\n"), "day\n1\ntrue\n2\nseen\n");
}
#[test]
fn maps_reject_wrong_keys_values_and_use_after_move() {
    for source in [
        "fn main() { var m = map_new(); m[1] = 2; }",
        "fn main() { var m = map_new(); m[\"a\"] = \"b\"; }",
        "fn main() { let m = map_new(); m[\"a\"] = 1; }",
        "fn main() { var m = map_new(); print(map_get(1, \"a\")); }",
        "fn main() { var m = map_new(); print(map_has(m, 2)); }",
        "fn main() { var m = map_new(); m[\"a\"] = 1; print(m[\"a\"]); }",
        "fn take(m: map[string, i64]) -> i64 { return len(m); } fn main() { var m = map_new(); print(take(m)); print(len(m)); }",
        "fn main() { var a: map[string, bool] = map_new(); print(len(a)); }",
    ] {
        let path = std::env::temp_dir().join(format!("ae-map-invalid-{}-{}.ae", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed)));
        fs::write(&path, source).unwrap();
        let output = Command::new(env!("CARGO_BIN_EXE_ae")).arg("check").arg(&path).output().unwrap();
        fs::remove_file(path).unwrap();
        assert!(!output.status.success(), "accepted invalid program: {source}");
    }
}
fn execute_with_env(source: &str, env: &[(&str, &str)]) -> std::process::Output {
    let path = std::env::temp_dir().join(format!("ae-integration-{}-{}.ae", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed)));
    fs::write(&path, source).unwrap();
    let mut command = Command::new(env!("CARGO_BIN_EXE_ae"));
    command.arg("run").arg(&path);
    for (key, value) in env { command.env(key, value); }
    let output = command.output().unwrap();
    fs::remove_file(path).unwrap();
    output
}
#[test]
fn utf8_length_and_prefix_count_characters_not_bytes() {
    let output = execute(r#"fn main() {
    let text = "中文 😀 abc";
    print(len(text));
    print(utf8_len(text));
    print(utf8_prefix(text, 2));
    print(utf8_prefix(text, 3));
    print(utf8_prefix(text, 100) == text);
    print(utf8_prefix("", 3) == "");
    print(utf8_prefix("abc", 0) == "");
}"#);
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).replace("\r\n", "\n"),
        "15\n8\n中文\n中文 \ntrue\ntrue\ntrue\n"
    );
}
#[test]
fn slicing_inside_a_character_is_rejected() {
    for source in [
        "fn main() { let text = \"中文\"; print(slice(text, 0, 1)); }",
        "fn main() { let text = \"中文\"; print(slice(text, 3, 5)); }",
        "fn main() { let text = \"中文\"; print(slice_view(text, 1, 3)); }",
        // A whole-character range is fine, and so is the empty range at a boundary.
        "fn main() { let text = \"中文\"; print(slice(text, 3, 6)); print(utf8_len(slice(text, 0, 0))); }",
    ] {
        let path = std::env::temp_dir().join(format!("ae-utf8-slice-{}-{}.ae", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed)));
        fs::write(&path, source).unwrap();
        let output = Command::new(env!("CARGO_BIN_EXE_ae")).arg("run").arg(&path).output().unwrap();
        fs::remove_file(path).unwrap();
        let allowed = source.contains("utf8_len(slice(text, 0, 0))");
        if allowed {
            assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
            assert_eq!(String::from_utf8_lossy(&output.stdout).replace("\r\n", "\n"), "文\n0\n");
        } else {
            assert!(!output.status.success(), "accepted a slice that splits a character: {source}");
            assert!(String::from_utf8_lossy(&output.stderr).contains("string index splits a UTF-8 character"), "{}", String::from_utf8_lossy(&output.stderr));
        }
    }
}
#[test]
fn split_accepts_borrowed_separators() {
    let output = execute(r#"fn fields(text: &string, sep: &string) -> i64 {
    let parts = split(text, sep);
    return len(parts);
}
fn main() {
    print(fields("a,b,c", ","));
    print(fields("no separator", ","));
}"#);
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    assert_eq!(String::from_utf8_lossy(&output.stdout).replace("\r\n", "\n"), "3\n1\n");
}
#[test]
fn allocation_failure_reports_the_operation_location() {
    let source = "fn main() {\n    let text = \"hello\";\n    print(text);\n}";
    let exhausted = execute_with_env(source, &[("AE_ALLOC_LIMIT", "0")]);
    assert!(!exhausted.status.success(), "an exhausted allocation budget must fail");
    let stderr = String::from_utf8_lossy(&exhausted.stderr);
    assert!(stderr.contains("out of memory"), "{stderr}");
    assert!(stderr.contains("ae:2:"), "the location of the string literal is missing: {stderr}");
    let generous = execute_with_env(source, &[("AE_ALLOC_LIMIT", "4096")]);
    assert!(generous.status.success(), "{}", String::from_utf8_lossy(&generous.stderr));
    assert_eq!(String::from_utf8_lossy(&generous.stdout).trim(), "hello");
    // A malformed limit is ignored rather than silently disabling the program.
    let malformed = execute_with_env(source, &[("AE_ALLOC_LIMIT", "lots")]);
    assert!(malformed.status.success(), "{}", String::from_utf8_lossy(&malformed.stderr));
}
#[test]
fn private_items_stay_inside_their_file() {
    let dir = std::env::temp_dir().join(format!("ae-priv-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("lib.ae"), "struct Hidden { n: i64 }\nenum Mood { Calm, Loud }\nfn secret() -> i64 { return 7; }\npub fn open() -> i64 { return 8; }\npub struct Shown { v: i64 }\n").unwrap();
    let main_file = dir.join("main.ae");
    let check = |body: &str| {
        fs::write(&main_file, format!("use \"lib.ae\";\nfn main() {{ {body} }}")).unwrap();
        Command::new(env!("CARGO_BIN_EXE_ae")).arg("check").arg(&main_file).output().unwrap()
    };
    let ok = check("let s = Shown { v: 1 }; print(open() + s.v);");
    assert!(ok.status.success(), "{}", String::from_utf8_lossy(&ok.stderr));
    for (body, expected) in [
        ("print(secret());", "function 'secret' is private to"),
        ("let h = Hidden { n: 1 }; print(h.n);", "struct 'Hidden' is private to"),
        ("let m = Mood::Calm; print(open());", "enum 'Mood' is private to"),
    ] {
        let output = check(body);
        assert!(!output.status.success(), "accepted: {body}");
        let err = String::from_utf8_lossy(&output.stderr);
        assert!(err.contains(expected), "{body}: {err}");
    }
    // A file sees its own private items, so the library itself compiles.
    fs::write(dir.join("alone.ae"), "struct Hidden { n: i64 }\nfn secret() -> i64 { return 7; }\nfn main() { let h = Hidden { n: secret() }; print(h.n); }").unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_ae")).arg("run").arg(dir.join("alone.ae")).output().unwrap();
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), "7");
    fs::remove_dir_all(dir).unwrap();
}
#[test]
fn trim_and_prefix_checks_work_on_bytes() {
    let output = execute(r#"fn main() {
    print("[" + trim("  padded \n") + "]");
    print(trim("   ") == "");
    print(trim("中文  "));
    print(starts_with("notes.txt", "note"));
    print(starts_with("notes.txt", "x"));
    print(ends_with("notes.txt", ".txt"));
    print(ends_with("a.txt.bak", ".txt"));
    print(starts_with("x", "") && ends_with("x", ""));
    print(ends_with("abc", "abcd"));
}"#);
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).replace("\r\n", "\n"),
        "[padded]\ntrue\n中文\ntrue\nfalse\ntrue\nfalse\ntrue\nfalse\n"
    );
}
#[test]
fn trim_and_prefix_checks_reject_wrong_argument_types() {
    for source in [
        "fn main() { print(trim(1)); }",
        "fn main() { print(starts_with(1, \"a\")); }",
        "fn main() { print(ends_with(\"a\", 2)); }",
        "fn main() { print(starts_with(\"a\")); }",
    ] {
        let path = std::env::temp_dir().join(format!("ae-trim-invalid-{}-{}.ae", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed)));
        fs::write(&path, source).unwrap();
        let output = Command::new(env!("CARGO_BIN_EXE_ae")).arg("check").arg(&path).output().unwrap();
        fs::remove_file(path).unwrap();
        assert!(!output.status.success(), "accepted invalid program: {source}");
    }
}
#[test]
fn generic_enums_instantiate_per_argument() {
    let output = execute(r#"enum Either[T, U] { Left(T), Right(U), Both(T, U), Neither }
fn locate(text: &string, needle: &string) -> Option[i64] {
    let parts = split(text, needle);
    if len(parts) < 2 { return Option[i64]::None; }
    return Option[i64]::Some(len(parts[0]));
}
fn label(opt: Option[i64]) -> string {
    match opt {
        Option::Some(v) => { return "at " + to_string(v); }
        Option::None => { return "absent"; }
    }
}
fn main() {
    let hit = locate("a,b,c", ",");
    match hit {
        Option::Some(v) => { print("hit " + to_string(v)); }
        Option::None => { print("miss"); }
    }
    print(label(locate("abc", ",")));
    print(label(locate("xy;z", ";")));
    let e = Either[string, i64]::Both("n", 3);
    match e {
        Either::Left(text) => { print("left " + text); }
        Either::Right(n) => { print(n); }
        Either::Both(text, n) => { print(text + "=" + to_string(n)); }
        Either::Neither => { print("neither"); }
    }
    let many = Either[i64, vec[string]]::Right(split("p q", " "));
    match many {
        Either::Left(n) => { print(n); }
        Either::Right(parts) => { print(len(parts)); }
        Either::Both(n, parts) => { print(n); }
        Either::Neither => { print("neither"); }
    }
    let absent = Option[vec[string]]::None;
    match absent {
        Option::Some(parts) => { print(len(parts)); }
        Option::None => { print("no vector"); }
    }
}"#);
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).replace("\r\n", "\n"),
        "hit 1\nabsent\nat 2\nn=3\n2\nno vector\n"
    );
}
#[test]
fn generic_enums_reuse_one_layout_per_argument_list() {
    // Two mentions of Option[string] must be the same type: the second match
    // would be rejected as a type mismatch otherwise.
    let output = execute(r#"fn pick(flag: bool) -> Option[string] {
    if flag { return Option[string]::Some("yes"); }
    return Option[string]::None;
}
fn main() {
    let a = pick(true);
    let b = pick(false);
    match a {
        Option::Some(v) => { print(v); }
        Option::None => { print("none"); }
    }
    match b {
        Option::Some(v) => { print(v); }
        Option::None => { print("none"); }
    }
}"#);
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    assert_eq!(String::from_utf8_lossy(&output.stdout).replace("\r\n", "\n"), "yes\nnone\n");
}
#[test]
fn invalid_generic_enums_are_rejected() {
    for (source, expected) in [
        ("fn main() { let a = Option; }", "'Option' is a generic enum"),
        ("fn main() { let a = Option[i64, i64]::Some(1); }", "takes 1 type argument(s), found 2"),
        ("struct P { x: i64 }\nfn main() { let a = Option[P]::None; }", "type arguments must be i64"),
        ("struct B[T, T] { v: i64 }\nfn main() { print(1); }", "duplicate type parameter"),
        ("enum P[t] { A(t) }\nfn main() { print(1); }", "type parameters are single uppercase letters"),
        ("enum Option { Here, Gone }\nfn main() { print(1); }", "duplicate enum 'Option'"),
        ("enum P[T] { A(T), B(T) }\nfn main() { let p = P[i64]::A(1); match p { P::A(v) => { print(v); } } }", "match is not exhaustive"),
        ("fn main() { let a = Option[i64]::Some(\"x\"); }", "payload 1 of 'Option[i64]::Some' must be i64"),
        ("fn main() { let a = Option[string]::Some(\"x\"); let b = a; print(len(a)); }", "because it was moved"),
        ("fn t(o: Option) -> i64 { return 1; }\nfn main() { print(t()); }", "is a generic enum"),
    ] {
        let path = std::env::temp_dir().join(format!("ae-generic-invalid-{}-{}.ae", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed)));
        fs::write(&path, source).unwrap();
        let output = Command::new(env!("CARGO_BIN_EXE_ae")).arg("check").arg(&path).output().unwrap();
        fs::remove_file(path).unwrap();
        assert!(!output.status.success(), "accepted invalid program: {source}");
        let err = String::from_utf8_lossy(&output.stderr);
        assert!(err.contains(expected), "{source}: {err}");
    }
}
#[test]
fn generic_instantiation_budget_is_enforced() {
    // Four parameters and four argument types give 256 combinations, which is
    // past the budget: the point is a diagnostic, not a compiler OOM.
    let mut source = String::from("enum Tag[T, U, V, W] { Here, There }\nfn main() {\n");
    let types = ["i64", "bool", "string", "vec"];
    let mut index = 0;
    for a in types { for b in types { for c in types { for d in types {
        source.push_str(&format!("    let t{index} = Tag[{a}, {b}, {c}, {d}]::Here;\n"));
        index += 1;
    } } } }
    source.push_str("    print(1);\n}\n");
    let path = std::env::temp_dir().join(format!("ae-generic-budget-{}-{}.ae", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed)));
    fs::write(&path, &source).unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_ae")).arg("check").arg(&path).output().unwrap();
    fs::remove_file(path).unwrap();
    assert!(!output.status.success(), "a program past the instantiation budget must be rejected");
    let err = String::from_utf8_lossy(&output.stderr);
    assert!(err.contains("generic instantiation budget exceeded"), "{err}");
}
#[test]
fn generic_enums_respect_file_visibility() {
    let dir = std::env::temp_dir().join(format!("ae-generic-vis-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("lib.ae"), "pub enum Found[T] { Here(T), Gone }\nfn hidden(flag: bool) -> Found[i64] { if flag { return Found[i64]::Here(1); } return Found[i64]::Gone; }\npub fn open(flag: bool) -> Found[i64] { return hidden(flag); }\n").unwrap();
    let main_file = dir.join("main.ae");
    let check = |body: &str| {
        fs::write(&main_file, format!("use \"lib.ae\";\nfn main() {{ {body} }}")).unwrap();
        Command::new(env!("CARGO_BIN_EXE_ae")).arg("check").arg(&main_file).output().unwrap()
    };
    // The exported enum is usable, and so is the exported function returning it.
    let ok = check("let f = open(true); match f { Found::Here(v) => { print(v); } Found::Gone => { print(0); } }");
    assert!(ok.status.success(), "{}", String::from_utf8_lossy(&ok.stderr));
    // A private function is still private, whatever type it returns.
    let bad = check("let f = hidden(true); print(1);");
    assert!(!bad.status.success());
    let err = String::from_utf8_lossy(&bad.stderr);
    assert!(err.contains("'hidden' is private to"), "{err}");
    // A private generic enum is not instantiable from another file either.
    fs::write(dir.join("secret.ae"), "enum Quiet[T] { Here(T), Gone }\npub fn one() -> i64 { return 1; }\n").unwrap();
    fs::write(&main_file, "use \"secret.ae\";\nfn main() { let q = Quiet[i64]::Gone; print(1); }").unwrap();
    let bad = Command::new(env!("CARGO_BIN_EXE_ae")).arg("check").arg(&main_file).output().unwrap();
    assert!(!bad.status.success());
    let err = String::from_utf8_lossy(&bad.stderr);
    assert!(err.contains("enum 'Quiet[i64]' is private to"), "{err}");
    fs::remove_dir_all(dir).unwrap();
}
#[test]
fn sort_and_reverse_order_vectors_in_place() {
    let output = execute(r#"fn main() {
    var numbers = vec[3, 1, 2];
    sort(numbers);
    print(numbers[0]);
    print(numbers[1]);
    print(numbers[2]);
    reverse(numbers);
    print(numbers[0]);
    var names = split("pear apple fig", " ");
    sort(names);
    print(names[0]);
    print(names[2]);
    reverse(names);
    print(names[0]);
    var empty = vec[];
    sort(empty);
    reverse(empty);
    print(len(empty));
    var one = vec[7];
    sort(one);
    print(one[0]);
}"#);
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).replace("\r\n", "\n"),
        "1\n2\n3\n3\napple\npear\npear\n0\n7\n"
    );
}
#[test]
fn sort_requires_a_mutable_vector_and_keeps_borrows_out() {
    for source in [
        "fn main() { sort(1); }",
        "fn main() { let v = vec[1]; sort(v); }",
        "fn main() { var s = \"text\"; sort(s); }",
        "fn main() { var v = vec[1]; for x in v { sort(v); } }",
    ] {
        let path = std::env::temp_dir().join(format!("ae-sort-invalid-{}-{}.ae", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed)));
        fs::write(&path, source).unwrap();
        let output = Command::new(env!("CARGO_BIN_EXE_ae")).arg("check").arg(&path).output().unwrap();
        fs::remove_file(path).unwrap();
        assert!(!output.status.success(), "accepted invalid program: {source}");
    }
}
#[test]
fn clock_reports_a_plausible_epoch() {
    let output = execute("fn main() { let seconds = now_s(); print(seconds > 1600000000); print(now_ms() >= seconds * 1000); print(now_ms() < (seconds + 2) * 1000); }");
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    assert_eq!(String::from_utf8_lossy(&output.stdout).replace("\r\n", "\n"), "true\ntrue\ntrue\n");
}
#[test]
fn environment_lookup_separates_missing_from_empty() {
    let source = r#"fn show(name: &string) -> string {
    let found = env(name);
    match found {
        ReadResult::Ok(value) => { return "[" + value + "]"; }
        ReadResult::Err(reason) => { return "missing"; }
    }
}
fn main() {
    print(show("AE_TEST_PRESENT"));
    print(show("AE_TEST_ABSENT"));
}"#;
    let output = execute_with_env(source, &[("AE_TEST_PRESENT", "value")]);
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    assert_eq!(String::from_utf8_lossy(&output.stdout).replace("\r\n", "\n"), "[value]\nmissing\n");
    // A variable that exists but holds nothing is still a value, not an error.
    let output = execute_with_env(source, &[("AE_TEST_PRESENT", "")]);
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    assert_eq!(String::from_utf8_lossy(&output.stdout).replace("\r\n", "\n"), "[]\nmissing\n");
    // An empty name is a caller mistake, reported rather than looked up.
    let output = execute_with_env("fn main() { let r = env(\"\"); match r { ReadResult::Ok(v) => { print(\"found\"); } ReadResult::Err(e) => { print(e); } } }", &[]);
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), "variable name is empty");
}
#[test]
fn append_adds_to_a_file_that_write_truncates() {
    let path = std::env::temp_dir().join(format!("ae-append-{}-{}.txt", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed)));
    let _ = fs::remove_file(&path);
    let literal = path.display().to_string().replace('\\', "/");
    let source = format!(r#"fn main() {{
    let first = try_append_file("{literal}", "one\n");
    match first {{ WriteResult::Ok(n) => {{ print(n); }} WriteResult::Err(r) => {{ print(r); }} }}
    let second = try_append_file("{literal}", "two\n");
    match second {{ WriteResult::Ok(n) => {{ print(n); }} WriteResult::Err(r) => {{ print(r); }} }}
    let text = read_file("{literal}");
    print(len(text));
    let replaced = try_write_file("{literal}", "gone");
    match replaced {{ WriteResult::Ok(n) => {{ print(n); }} WriteResult::Err(r) => {{ print(r); }} }}
    print(read_file("{literal}"));
    let missing = try_append_file("{literal}/nested/x.txt", "x");
    match missing {{ WriteResult::Ok(n) => {{ print("unexpected"); }} WriteResult::Err(r) => {{ print("failed"); }} }}
}}"#, literal = literal);
    let output = execute(&source);
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).replace("\r\n", "\n"),
        "4\n4\n8\n4\ngone\nfailed\n"
    );
    let _ = fs::remove_file(&path);
}
#[test]
fn exit_sets_the_status_and_flushes_what_was_printed() {
    let output = execute(r#"fn main() {
    let note = "about to stop";
    print(note);
    exit(3);
}"#);
    assert_eq!(output.status.code(), Some(3), "{}", String::from_utf8_lossy(&output.stderr));
    assert_eq!(String::from_utf8_lossy(&output.stdout).replace("\r\n", "\n"), "about to stop\n");
    // Code after a terminating statement is never reached.
    let output = execute("fn main() { print(\"before\"); exit(0); print(\"after\"); }");
    assert_eq!(output.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&output.stdout).replace("\r\n", "\n"), "before\n");
}
#[test]
fn exit_can_end_a_function_that_never_returns_a_value() {
    let output = execute(r#"fn stop(code: i64) -> i64 {
    let note = "stopping";
    print(note);
    exit(code);
}
fn main() {
    let reached = stop(4);
    print(reached);
}"#);
    assert_eq!(output.status.code(), Some(4), "{}", String::from_utf8_lossy(&output.stderr));
    assert_eq!(String::from_utf8_lossy(&output.stdout).replace("\r\n", "\n"), "stopping\n");
}
#[test]
fn exit_rejects_non_integer_status_codes() {
    for source in ["fn main() { exit(\"no\"); }", "fn main() { exit(true); }", "fn main() { exit(); }"] {
        let path = std::env::temp_dir().join(format!("ae-exit-invalid-{}-{}.ae", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed)));
        fs::write(&path, source).unwrap();
        let output = Command::new(env!("CARGO_BIN_EXE_ae")).arg("check").arg(&path).output().unwrap();
        fs::remove_file(path).unwrap();
        assert!(!output.status.success(), "accepted invalid program: {source}");
    }
}
fn execute_with_stdin(source: &str, input: &[u8]) -> std::process::Output {
    use std::io::Write;
    let path = std::env::temp_dir().join(format!("ae-stdin-{}-{}.ae", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed)));
    fs::write(&path, source).unwrap();
    let mut child = Command::new(env!("CARGO_BIN_EXE_ae"))
        .arg("run")
        .arg(&path)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.as_mut().unwrap().write_all(input).unwrap();
    let output = child.wait_with_output().unwrap();
    fs::remove_file(path).unwrap();
    output
}
#[test]
fn standard_input_arrives_as_bytes_without_newline_translation() {
    let source = r#"fn main() {
    let input = read_stdin();
    match input {
        ReadResult::Ok(text) => { print(len(text)); print(contains(text, "\r")); }
        ReadResult::Err(reason) => { print("cannot read: " + reason); }
    }
}"#;
    let output = execute_with_stdin(source, b"a\r\nb");
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    assert_eq!(String::from_utf8_lossy(&output.stdout).replace("\r\n", "\n"), "4\ntrue\n");
    // Nothing on the other end is an empty read, not a failure.
    let output = execute_with_stdin(source, b"");
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    assert_eq!(String::from_utf8_lossy(&output.stdout).replace("\r\n", "\n"), "0\nfalse\n");
}
#[test]
fn working_directory_is_a_usable_path() {
    let output = execute(r#"fn main() {
    let here = cwd();
    match here {
        ReadResult::Ok(path) => { print(path_is_dir(path)); print(len(path) > 1); }
        ReadResult::Err(reason) => { print("cannot tell: " + reason); }
    }
}"#);
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    assert_eq!(String::from_utf8_lossy(&output.stdout).replace("\r\n", "\n"), "true\ntrue\n");
}
#[test]
fn the_pipe_stats_example_counts_a_piped_stream() {
    let source = fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/examples/pipe_stats.ae")).unwrap();
    let output = execute_with_stdin(&source, "中文行\nshort\n".as_bytes());
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).replace("\r\n", "\n"),
        "bytes 16\ncharacters 10\nlines 2\nlongest line 5\n"
    );
}
#[test]
fn json_paths_select_members_elements_and_escapes() {
    let source = r#"fn main() {
    let doc = "{ \"name\": \"ae\", \"tags\": [\"lang\", \"fast\"], \"meta\": { \"stars\": 12, \"flag\": true }, \"note\": null, \"text\": \"caf\\u00e9\\n\" }";
    let a = json_get(doc, "name");
    match a { ReadResult::Ok(v) => { print(v); } ReadResult::Err(e) => { print(e); } }
    let b = json_get(doc, "tags[1]");
    match b { ReadResult::Ok(v) => { print(v); } ReadResult::Err(e) => { print(e); } }
    let c = json_int(doc, "meta.stars");
    match c { IntResult::Ok(n) => { print(to_string(n)); } IntResult::Err(e) => { print(e); } }
    let d = json_bool(doc, "meta.flag");
    match d { BoolResult::Ok(f) => { print(f); } BoolResult::Err(e) => { print(e); } }
    let e = json_get(doc, "note");
    match e { ReadResult::Ok(v) => { print(v); } ReadResult::Err(x) => { print(x); } }
    let f = json_get(doc, "text");
    match f { ReadResult::Ok(v) => { print(v); print(len(v)); } ReadResult::Err(x) => { print(x); } }
    let g = json_len(doc, "tags");
    match g { IntResult::Ok(n) => { print(to_string(n)); } IntResult::Err(x) => { print(x); } }
    print(json_has(doc, "meta.stars"));
    print(json_has(doc, "missing"));
}"#;
    let output = execute(source);
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).replace("\r\n", "\n"),
        "ae\nfast\n12\ntrue\nnull\ncafé\n\n6\n2\ntrue\nfalse\n"
    );
}

#[test]
fn json_errors_separate_missing_from_wrong_type() {
    let source = r#"fn main() {
    let doc = "{ \"s\": \"x\", \"n\": 3, \"arr\": [1], \"big\": 99999999999999999999, \"f\": 1.5 }";
    let a = json_get(doc, "missing");
    match a { ReadResult::Ok(v) => { print(v); } ReadResult::Err(e) => { print(e); } }
    let b = json_int(doc, "s");
    match b { IntResult::Ok(n) => { print(to_string(n)); } IntResult::Err(e) => { print(e); } }
    let c = json_bool(doc, "n");
    match c { BoolResult::Ok(f) => { print(f); } BoolResult::Err(e) => { print(e); } }
    let d = json_len(doc, "s");
    match d { IntResult::Ok(n) => { print(to_string(n)); } IntResult::Err(e) => { print(e); } }
    let e = json_get(doc, "arr[3]");
    match e { ReadResult::Ok(v) => { print(v); } ReadResult::Err(x) => { print(x); } }
    let f = json_get(doc, "s.x");
    match f { ReadResult::Ok(v) => { print(v); } ReadResult::Err(x) => { print(x); } }
    let g = json_int(doc, "big");
    match g { IntResult::Ok(n) => { print(to_string(n)); } IntResult::Err(x) => { print(x); } }
    let h = json_int(doc, "f");
    match h { IntResult::Ok(n) => { print(to_string(n)); } IntResult::Err(x) => { print(x); } }
    let i = json_get(doc, "a.");
    match i { ReadResult::Ok(v) => { print(v); } ReadResult::Err(x) => { print(x); } }
    let j = json_get("{ \"a\": }", "a");
    match j { ReadResult::Ok(v) => { print(v); } ReadResult::Err(x) => { print(x); } }
}"#;
    let output = execute(source);
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).replace("\r\n", "\n"),
        concat!(
            "no member named 'missing'\n",
            "'s' is a string, not a number\n",
            "'n' is a number, not a boolean\n",
            "'s' is a string; only arrays and objects have a member count\n",
            "path segment [3] is past the end of the array\n",
            "path segment 'x' needs an object, found a string\n",
            "'big' does not fit in i64\n",
            "'f' is not an integer\n",
            "path ends with '.'\n",
            "JSON syntax error near byte 0\n",
        )
    );
}
#[test]
fn cache_prunes_entries_beyond_the_configured_limit() {
    let dir = std::env::temp_dir().join(format!("ae-cache-limit-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    let cache = dir.join("cache");
    let source = dir.join("prog.ae");
    fs::create_dir(&dir).unwrap();
    fs::write(&source, "fn main() { print(\"limited\"); }").unwrap();
    let stats = || {
        let out = Command::new(env!("CARGO_BIN_EXE_ae")).arg("cache").arg("stats")
            .env("AE_CACHE", &cache).env("AE_CACHE_LIMIT", "1").output().unwrap();
        String::from_utf8_lossy(&out.stdout).into_owned()
    };
    let build = |out: &std::path::Path, limit: Option<&str>| {
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_ae"));
        cmd.arg("build").arg(&source).arg("-o").arg(out).env("AE_CACHE", &cache).env("AE_TIMINGS", "1");
        match limit {
            Some(value) => { cmd.env("AE_CACHE_LIMIT", value); }
            None => { cmd.env_remove("AE_CACHE_LIMIT"); }
        }
        cmd.output().unwrap()
    };
    // One byte is below any entry, so publishing is followed by an immediate
    // prune: the build succeeds and the cache ends up empty.
    let first = dir.join(if cfg!(windows) { "one.exe" } else { "one" });
    let out = build(&first, Some("1"));
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    // `frontend entries:` also ends in `entries: 0`, so compare the first line
    // instead of searching the whole report.
    assert_eq!(stats().lines().next().unwrap_or(""), "entries: 0", "{}", stats());
    assert!(stats().contains("limit: 1"), "{}", stats());
    let output = Command::new(&first).output().unwrap();
    assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), "limited");
    // Without the cap the entry survives, so the cache is usable at all.
    let second = dir.join(if cfg!(windows) { "two.exe" } else { "two" });
    let out = build(&second, None);
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    let after = String::from_utf8_lossy(&Command::new(env!("CARGO_BIN_EXE_ae"))
        .arg("cache").arg("stats").env("AE_CACHE", &cache).env_remove("AE_CACHE_LIMIT").output().unwrap().stdout).into_owned();
    assert_eq!(after.lines().next().unwrap_or(""), "entries: 1", "{after}");
    assert!(after.contains("limit: none"), "{after}");
    let _ = fs::remove_dir_all(&dir);
}
#[test]
fn string_vectors_start_empty_and_copy_borrowed_elements() {
    // `args()` hands back owned strings, but indexing it gives a borrow. Pushing
    // a borrow must copy it: moving the element would empty the slot it was read
    // from, and the vector would free the same buffer twice on the way out.
    let source = r#"fn main() {
    let argv = args();
    var copy = strvec_new();
    for i in 0..len(argv) { push(copy, argv[i]); }
    print(len(copy));
    print(len(argv));
    print(join(copy, "|") == join(argv, "|"));
}"#;
    let path = std::env::temp_dir().join(format!("ae-strvec-copy-{}-{}.ae", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed)));
    fs::write(&path, source).unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_ae"))
        .arg("run").arg(&path).arg("--").arg("one").arg("two").output().unwrap();
    fs::remove_file(path).unwrap();
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    assert_eq!(String::from_utf8_lossy(&output.stdout).replace("\r\n", "\n"), "3\n3\ntrue\n");
}

#[test]
fn run_command_reports_status_and_output() {
    // One child invocation that must exist on both platforms, plus one program
    // name that cannot exist: a failed start is an Err, a non-zero exit is not.
    let (program, arguments) = if cfg!(windows) {
        ("cmd", vec!["/c", "echo", "child-ok"])
    } else {
        ("sh", vec!["-c", "echo child-ok"])
    };
    let pushes = arguments.iter().map(|a| format!("    push(argv, \"{a}\");\n")).collect::<String>();
    let source = format!(r#"fn main() {{
    var argv = strvec_new();
{pushes}    let result = run_command("{program}", argv);
    match result {{
        CommandResult::Ok(code, out) => {{ print(code); print(contains(out, "child-ok")); }}
        CommandResult::Err(reason) => {{ print("failed: " + reason); }}
    }}
    var empty = strvec_new();
    let missing = run_command("ae-no-such-program-on-path", empty);
    match missing {{
        CommandResult::Ok(code, out) => {{ print("unexpected " + to_string(code)); }}
        CommandResult::Err(reason) => {{ print(contains(reason, "cannot start")); }}
    }}
}}"#);
    let output = execute(&source);
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    assert_eq!(String::from_utf8_lossy(&output.stdout).replace("\r\n", "\n"), "0\ntrue\ntrue\n");
}

#[test]
fn generic_borrow_parameters_resolve_at_each_call_site() {
    // `&T` is not a new borrow kind: after substitution it is one of the three
    // read-only borrows the language already has, with the same call syntax.
    let source = r#"fn size_of[T](value: &T) -> i64 { return len(value); }
fn total(values: &vec) -> i64 {
    var sum = 0;
    for i in 0..len(values) { sum = sum + values[i]; }
    return sum;
}
fn forward(values: &vec) -> i64 { return total(&values); }
fn count(parts: &vec[string]) -> i64 { return len(parts); }
fn forward_parts(parts: &vec[string]) -> i64 { return count(&parts); }
fn main() {
    let text = "AE";
    let numbers = vec[10, 20, 30];
    let words = split("alpha,beta,gamma", ",");
    print(size_of[string](text));
    print(size_of[vec](&numbers));
    print(size_of[vec[string]](&words));
    print(forward(&numbers));
    print(forward_parts(&words));
    print(text);
    print(numbers[0]);
    print(words[1]);
}"#;
    let output = execute(source);
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    assert_eq!(String::from_utf8_lossy(&output.stdout).replace("\r\n", "\n"), "6\n3\n3\n60\n3\nAE\n10\nbeta\n");

    for (source, expected) in [
        ("fn f[T](x: &T) -> i64 { return len(x); } fn main() { print(f[i64](1)); }", "has no read-only borrow"),
        ("fn f[T](x: &T) -> i64 { return len(x); } fn main() { print(f[bool](true)); }", "has no read-only borrow"),
        ("fn f(x: &T) -> i64 { return len(x); } fn main() { print(1); }", "unsupported borrow type"),
        ("fn f[T](x: T) -> &T { return x; } fn main() { print(f[i64](1)); }", "expected identifier"),
        ("struct B[T] { v: &T } fn main() { print(1); }", "expected identifier"),
        // A borrowed string vector is not a borrowed i64 vector.
        ("fn inner(v: &vec) -> i64 { return len(v); } fn outer(v: &vec[string]) -> i64 { return inner(&v); } fn main() { print(1); }", "argument must be vec"),
        // Forwarding must not relax the read-only rule.
        ("fn inner(v: &vec) -> i64 { push(v, 1); return 0; } fn outer(v: &vec) -> i64 { return inner(&v); } fn main() { print(1); }", "push expects a vec"),
        // A view cannot escape the callee, generic or not.
        ("fn f[T](x: &T) -> string { return x; } fn main() { print(f[string](\"a\")); }", "return type must be string"),
    ] {
        let path = std::env::temp_dir().join(format!("ae-generic-borrow-{}-{}.ae", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed)));
        fs::write(&path, source).unwrap();
        let output = Command::new(env!("CARGO_BIN_EXE_ae")).arg("check").arg(&path).output().unwrap();
        fs::remove_file(path).unwrap();
        assert!(!output.status.success(), "accepted invalid program: {source}");
        let err = String::from_utf8_lossy(&output.stderr);
        assert!(err.contains(expected), "{source}: {err}");
    }
}

#[test]
fn find_reports_a_byte_offset_or_an_error_value() {
    let source = r#"fn main() {
    let line = "name=AE; mode=fast";
    let eq = find(line, "=");
    match eq { IntResult::Ok(at) => { print(at); } IntResult::Err(reason) => { print(reason); } }
    let mode = find(line, "mode=");
    match mode { IntResult::Ok(at) => { print(at); } IntResult::Err(reason) => { print(reason); } }
    let absent = find(line, "slow");
    match absent { IntResult::Ok(at) => { print(at); } IntResult::Err(reason) => { print(reason); } }
    let empty = find(line, "");
    match empty { IntResult::Ok(at) => { print(at); } IntResult::Err(reason) => { print(reason); } }
    let utf8 = find("中文 abc", "abc");
    match utf8 { IntResult::Ok(at) => { print(at); } IntResult::Err(reason) => { print(reason); } }
}"#;
    let output = execute(source);
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    assert_eq!(String::from_utf8_lossy(&output.stdout).replace("\r\n", "\n"), "4\n13\nsubstring not found\n0\n7\n");

    for (source, expected) in [
        ("fn main() { let x = find(1, \"a\"); print(1); }", "expects a string to search in"),
        ("fn main() { let x = find(\"a\", 1); print(1); }", "expects a string to search for"),
        ("fn main() { let find = 1; print(find); }", "built-in name"),
        ("fn main() { find(\"a\", \"b\"); }", "must end with"),
    ] {
        let path = std::env::temp_dir().join(format!("ae-find-invalid-{}-{}.ae", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed)));
        fs::write(&path, source).unwrap();
        let output = Command::new(env!("CARGO_BIN_EXE_ae")).arg("check").arg(&path).output().unwrap();
        fs::remove_file(path).unwrap();
        assert!(!output.status.success(), "accepted invalid program: {source}");
        let err = String::from_utf8_lossy(&output.stderr);
        assert!(err.contains(expected), "{source}: {err}");
    }
}

#[test]
fn float_arithmetic_prints_a_canonical_form() {
    let source = r#"fn mean(values: &vec) -> f64 {
    var total = 0.0;
    for i in 0..len(values) { total = total + to_f64(values[i]); }
    return total / to_f64(len(values));
}
fn main() {
    let data = vec[2, 4, 4, 4, 5, 5, 7, 9];
    print(mean(&data));
    print(1.0 / 3.0);
    print(0.1 + 0.2);
    print(0.1 + 0.2 == 0.3);
    print(1000.0);
    print(-0.0);
    print(1e16);
    print(0.0001);
    let huge = 1e308 * 10.0;
    print(huge);
    print(huge - huge);
    print(to_string(3.14));
    let whole = to_i64(5.0);
    match whole { IntResult::Ok(n) => { print(n); } IntResult::Err(reason) => { print(reason); } }
    let fraction = to_i64(0.1 + 0.2);
    match fraction { IntResult::Ok(n) => { print(n); } IntResult::Err(reason) => { print(reason); } }
    let not_finite = to_i64(huge);
    match not_finite { IntResult::Ok(n) => { print(n); } IntResult::Err(reason) => { print(reason); } }
    let too_large = to_i64(1e30);
    match too_large { IntResult::Ok(n) => { print(n); } IntResult::Err(reason) => { print(reason); } }
}"#;
    let output = execute(source);
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).replace("\r\n", "\n"),
        "5.0\n0.3333333333333333\n0.30000000000000004\nfalse\n1000.0\n-0.0\n1e+16\n1e-4\ninf\nnan\n3.14\n5\nf64 value is not an exact integer\nf64 value is not finite\nf64 value is outside the i64 range\n"
    );
}

#[test]
fn float_exponents_and_fields_resolve_without_mixing() {
    let source = r#"struct Point { x: f64, y: f64 }
enum Reading { Value(f64), Missing }
fn add[T: Add](a: T, b: T) -> T { return a + b; }
fn main() {
    let p = Point { x: 1.5, y: -2.5 };
    print(p.x + p.y);
    let r = Reading::Value(0.25);
    match r { Reading::Value(v) => { print(v * 4.0); } Reading::Missing => { print(0.0); } }
    print(add[f64](1.25, 2.75));
    let maybe = Option[f64]::Some(0.5);
    match maybe { Option::Some(v) => { print(v); } Option::None => { print(0.0); } }
    print(2.5e-3);
}"#;
    let output = execute(source);
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    assert_eq!(String::from_utf8_lossy(&output.stdout).replace("\r\n", "\n"), "-1.0\n1.0\n4.0\n0.5\n0.0025\n");
}

#[test]
fn float_rejections_keep_conversions_explicit() {
    for (source, expected) in [
        ("fn main() { print(1 + 2.0); }", "mixed i64/f64 arithmetic"),
        ("fn main() { print(5.0 % 2.0); }", "operands only"),
        ("fn main() { if 1.0 { print(1); } }", "boolean"),
        ("fn main() { print(1.0e400); }", "not finite"),
        ("fn main() { print(1.0e-400); }", "underflows"),
        // vec[1.0] became a valid vec[f64] literal (ADR-61); what stays
        // rejected is mixing element types inside one literal.
        ("fn main() { let v = vec[1, 2.0]; }", "does not convert implicitly"),
        ("fn main() { print(to_f64(1.0)); }", "expects an i64"),
        ("fn main() { print(to_i64(1)); }", "expects an f64"),
        ("fn main() { print(to_string(true)); }", "expects an i64 or f64"),
        ("fn main() { let x = vec[1]; print(x[0.5]); }", "index"),
        ("fn main() { let x = 1.0; x = 2; }", "cannot assign"),
    ] {
        let path = std::env::temp_dir().join(format!("ae-float-invalid-{}-{}.ae", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed)));
        fs::write(&path, source).unwrap();
        let output = Command::new(env!("CARGO_BIN_EXE_ae")).arg("check").arg(&path).output().unwrap();
        fs::remove_file(path).unwrap();
        assert!(!output.status.success(), "accepted invalid program: {source}");
        let err = String::from_utf8_lossy(&output.stderr);
        assert!(err.contains(expected), "{source}: {err}");
    }
}

#[test]
fn float_runtime_guards_report_instead_of_rounding() {
    for (source, expected) in [
        ("fn main() { print(to_f64(9007199254740993)); }", "would lose precision"),
        ("fn main() { let z = 0.0; print(1.0 / z); }", "f64 division by zero"),
    ] {
        let output = execute(source);
        assert!(!output.status.success(), "{source}");
        assert!(String::from_utf8_lossy(&output.stderr).contains(expected), "{source}: {}", String::from_utf8_lossy(&output.stderr));
    }
}

#[test]
fn interface_summaries_ignore_function_bodies_but_track_public_signatures() {
    let dir = std::env::temp_dir().join(format!("ae-deps-test-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir(&dir).unwrap();
    let lib = dir.join("lib.ae");
    let main = dir.join("main.ae");
    let report = || {
        let out = Command::new(env!("CARGO_BIN_EXE_ae")).arg("deps").arg(&main).output().unwrap();
        assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
        String::from_utf8_lossy(&out.stdout).replace("\r\n", "\n")
    };

    fs::write(&main, "use \"lib.ae\";\nfn main() { print(twice(3)); }\n").unwrap();
    fs::write(&lib, "pub fn twice(x: i64) -> i64 { return x + x; }\n").unwrap();
    let first = report();
    assert!(first.contains("uses lib.ae"), "{first}");
    assert!(first.contains("exports 1"), "{first}");
    assert!(first.contains("fingerprint "), "{first}");

    // A body-only edit (including a private helper) must not change any hash.
    fs::write(&lib, "fn helper(x: i64) -> i64 { return x + 1; }\npub fn twice(x: i64) -> i64 { let y = x + x; return helper(y) - 1; }\n").unwrap();
    assert_eq!(report(), first, "a body edit changed the interface fingerprint");

    // Changing a public signature changes that module's interface and the
    // program fingerprint, while the dependency graph stays the same.
    fs::write(&main, "use \"lib.ae\";\nfn main() { print(twice(3.0)); }\n").unwrap();
    fs::write(&lib, "pub fn twice(x: f64) -> f64 { return x + x; }\n").unwrap();
    let third = report();
    assert_ne!(third, first, "a signature edit left the fingerprint unchanged");
    assert!(third.contains("uses lib.ae"), "{third}");
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn deps_reports_the_import_graph_in_load_order() {
    let out = Command::new(env!("CARGO_BIN_EXE_ae")).arg("deps")
        .arg("examples/modules/main.ae").output().unwrap();
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    let text = String::from_utf8_lossy(&out.stdout).replace("\r\n", "\n");
    assert!(text.contains("module words.ae"), "{text}");
    assert!(text.contains("module report.ae"), "{text}");
    assert!(text.contains("module main.ae"), "{text}");
    assert!(text.contains("  uses words.ae\n  uses report.ae\n"), "{text}");
}

fn unique_temp_dir(label: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("ae-fmt-test-{}-{}-{}", label, std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed)));
    fs::create_dir(&dir).unwrap();
    dir
}

#[test]
fn fmt_normalizes_keeps_semantics_and_is_idempotent() {
    let dir = unique_temp_dir("sem");
    let input = dir.join("fmt_target.ae");
    let messy = "fn main(){\n// count\nvar total=0;\nfor i in 0..3 {\ntotal=total+i;\n}\nprint(total); // done\n}\n";
    fs::write(&input, messy).unwrap();
    let ae = || Command::new(env!("CARGO_BIN_EXE_ae"));
    let before = ae().arg("emit-c").arg(&input).output().unwrap();
    assert!(before.status.success(), "{}", String::from_utf8_lossy(&before.stderr));

    let first = ae().arg("fmt").arg(&input).output().unwrap();
    assert!(first.status.success(), "{}", String::from_utf8_lossy(&first.stderr));
    let formatted = fs::read_to_string(&input).unwrap();
    assert_eq!(formatted, "fn main() {\n    // count\n    var total = 0;\n    for i in 0..3 {\n        total = total + i;\n    }\n    print(total); // done\n}\n");

    // A second pass must be a no-op: formatting is idempotent.
    let second = ae().arg("fmt").arg(&input).output().unwrap();
    assert!(second.status.success(), "{}", String::from_utf8_lossy(&second.stderr));
    assert_eq!(fs::read_to_string(&input).unwrap(), formatted);

    // Semantics are unchanged. Byte-equality of the generated C is not the
    // contract: runtime checks embed the source column, and spacing moves
    // columns. What must hold here is that the generated C changes only in
    // embedded numbers, so every differing line matches once digits are
    // ignored. (Token stream and line numbers are covered by the formatter's
    // own unit tests against the compiler lexer.)
    let after = ae().arg("emit-c").arg(&input).output().unwrap();
    let before_c = String::from_utf8_lossy(&before.stdout).replace("\r\n", "\n");
    let after_c = String::from_utf8_lossy(&after.stdout).replace("\r\n", "\n");
    assert_eq!(after_c.lines().count(), before_c.lines().count(), "generated C line count changed");
    let ignore_digits = |line: &str| -> String { line.chars().filter(|c| !c.is_ascii_digit()).collect() };
    for (index, (was, now)) in before_c.lines().zip(after_c.lines()).enumerate() {
        assert_eq!(ignore_digits(was), ignore_digits(now),
            "generated C line {index} changed beyond embedded numbers:\n  before: {was}\n  after:  {now}");
    }

    // And the program still computes the same answer.
    let run = execute(&formatted);
    assert!(run.status.success(), "{}", String::from_utf8_lossy(&run.stderr));
    assert_eq!(String::from_utf8_lossy(&run.stdout).replace("\r\n", "\n"), "3\n");
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn fmt_adds_the_final_newline_and_reports_no_change_when_clean() {
    let dir = unique_temp_dir("nl");
    let input = dir.join("tidy.ae");
    fs::write(&input, "fn main() { print(1); }").unwrap();
    let ae = || Command::new(env!("CARGO_BIN_EXE_ae"));
    let result = ae().arg("fmt").arg(&input).output().unwrap();
    assert!(result.status.success(), "{}", String::from_utf8_lossy(&result.stderr));
    assert_eq!(fs::read_to_string(&input).unwrap(), "fn main() { print(1); }\n");
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn fmt_refuses_unlexable_files_and_leaves_them_untouched() {
    let dir = unique_temp_dir("bad");
    let ae = || Command::new(env!("CARGO_BIN_EXE_ae"));
    for broken in [
        // Unterminated string.
        "fn main() { print(\"open); }\n",
        // A character the language has no token for.
        "fn main() { print(1 @ 2); }\n",
    ] {
        let input = dir.join("broken.ae");
        fs::write(&input, broken).unwrap();
        let result = ae().arg("fmt").arg(&input).output().unwrap();
        assert!(!result.status.success(), "fmt accepted {broken:?}");
        assert!(!String::from_utf8_lossy(&result.stderr).is_empty());
        assert_eq!(fs::read_to_string(&input).unwrap(), broken, "fmt modified a file it refused to format");
    }
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn fmt_formats_files_that_fail_full_check() {
    // A semantic error does not make whitespace normalization unsafe, and
    // module files have no main of their own, so fmt only requires
    // lexability. A file with an unknown name is still reformatted.
    let dir = unique_temp_dir("semantic");
    let input = dir.join("broken.ae");
    fs::write(&input, "fn main() {\nprint(who_am_i);\n}\n").unwrap();
    let result = Command::new(env!("CARGO_BIN_EXE_ae")).arg("fmt").arg(&input).output().unwrap();
    assert!(result.status.success(), "{}", String::from_utf8_lossy(&result.stderr));
    assert_eq!(fs::read_to_string(&input).unwrap(), "fn main() {\n    print(who_am_i);\n}\n");
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn fmt_formats_a_module_file_without_a_main() {
    let dir = unique_temp_dir("module");
    let modules = dir.join("modules");
    fs::create_dir(&modules).unwrap();
    for name in ["main.ae", "words.ae", "report.ae"] {
        fs::copy(format!("examples/modules/{name}"), modules.join(name)).unwrap();
    }
    let before = Command::new(env!("CARGO_BIN_EXE_ae")).arg("emit-c").arg(modules.join("main.ae")).output().unwrap();
    assert!(before.status.success(), "{}", String::from_utf8_lossy(&before.stderr));

    let result = Command::new(env!("CARGO_BIN_EXE_ae")).arg("fmt").arg(modules.join("report.ae")).output().unwrap();
    assert!(result.status.success(), "{}", String::from_utf8_lossy(&result.stderr));

    // The whole program still emits the same C up to embedded columns.
    let after = Command::new(env!("CARGO_BIN_EXE_ae")).arg("emit-c").arg(modules.join("main.ae")).output().unwrap();
    assert!(after.status.success(), "{}", String::from_utf8_lossy(&after.stderr));
    let ignore_digits = |line: &str| -> String { line.chars().filter(|c| !c.is_ascii_digit()).collect() };
    let strip = |bytes: &[u8]| -> Vec<String> {
        String::from_utf8_lossy(bytes).replace("\r\n", "\n").split('\n')
            .map(|l| ignore_digits(l)).collect()
    };
    assert_eq!(strip(&before.stdout), strip(&after.stdout));
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn fmt_preserves_comment_and_string_bytes_end_to_end() {
    let dir = unique_temp_dir("bytes");
    let input = dir.join("keep.ae");
    let source = "// header comment\nfn main() {\n    // indented comment\n    print(\"tab\\there quote\\\" done\"); // tail\n    print(1e3);\n    print(007);\n}\n";
    fs::write(&input, source).unwrap();
    let ae = || Command::new(env!("CARGO_BIN_EXE_ae"));
    let result = ae().arg("fmt").arg(&input).output().unwrap();
    assert!(result.status.success(), "{}", String::from_utf8_lossy(&result.stderr));
    let formatted = fs::read_to_string(&input).unwrap();
    // Everything but indentation is byte-identical: comments, escapes and
    // literal spellings are re-emitted from the source, not re-created.
    for needle in ["// header comment", "// indented comment", "\"tab\\there quote\\\" done\"); // tail", "1e3", "007"] {
        assert!(formatted.contains(needle), "lost {needle:?} in:\n{formatted}");
    }
    let run = execute(&formatted);
    assert!(run.status.success());
    // The integer literal keeps its value; the float's canonical text is
    // ADR-37's domain and asserted there, not here.
    let out = String::from_utf8_lossy(&run.stdout).replace("\r\n", "\n");
    assert!(out.lines().any(|l| l == "7"), "{out}");
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn test_runner_reports_pass_fail_and_panic_with_exit_codes() {
    let dir = unique_temp_dir("runner");
    let ae = || Command::new(env!("CARGO_BIN_EXE_ae"));

    let passing = dir.join("pass.ae");
    fs::write(&passing, "fn test_add() -> i64 {\n    assert(2 + 2 == 4);\n    return 0;\n}\n\nfn test_len() -> i64 {\n    if len(\"abc\") != 3 { return 1; }\n    return 0;\n}\n").unwrap();
    let output = ae().arg("test").arg(&passing).output().unwrap();
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    let stdout = String::from_utf8_lossy(&output.stdout).replace("\r\n", "\n");
    assert_eq!(stdout, "RUN test_add\nPASS test_add\nRUN test_len\nPASS test_len\nok: 2 passed\n");

    // A test returning a non-zero value fails the suite with that code, and
    // tests after it do not run.
    let failing = dir.join("fail.ae");
    fs::write(&failing, "fn test_first() -> i64 {\n    return 0;\n}\n\nfn test_second() -> i64 {\n    return 3;\n}\n\nfn test_third() -> i64 {\n    return 0;\n}\n").unwrap();
    let output = ae().arg("test").arg(&failing).output().unwrap();
    assert_eq!(output.status.code(), Some(3));
    let stdout = String::from_utf8_lossy(&output.stdout).replace("\r\n", "\n");
    assert!(stdout.contains("PASS test_first"), "{stdout}");
    assert!(stdout.contains("FAIL test_second"), "{stdout}");
    assert!(!stdout.contains("RUN test_third"), "{stdout}");
    assert!(!stdout.contains("ok:"), "{stdout}");

    // A filter is a substring on the test name; only matching tests run and
    // the ok line counts the filtered set. An empty match is an error, not a
    // green run (ADR-62).
    let filtered = dir.join("filter.ae");
    fs::write(&filtered, "fn test_trim_start() -> i64 {\n    return 0;\n}\n\nfn test_trim_end() -> i64 {\n    return 0;\n}\n\nfn test_join() -> i64 {\n    return 0;\n}\n").unwrap();
    let output = ae().arg("test").arg(&filtered).arg("trim").output().unwrap();
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    let stdout = String::from_utf8_lossy(&output.stdout).replace("\r\n", "\n");
    assert_eq!(stdout, "RUN test_trim_start\nPASS test_trim_start\nRUN test_trim_end\nPASS test_trim_end\nok: 2 passed\n");
    let output = ae().arg("test").arg(&filtered).output().unwrap();
    assert!(output.status.success());
    assert!(String::from_utf8_lossy(&output.stdout).contains("ok: 3 passed"));
    let output = ae().arg("test").arg(&filtered).arg("unicode").output().unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("no test functions match filter 'unicode'"), "{}", String::from_utf8_lossy(&output.stderr));

    // A panicking test keeps the existing failure behaviour: location on
    // stderr, exit status 1.
    let panicking = dir.join("panic.ae");
    fs::write(&panicking, "fn test_boom() -> i64 {\n    assert(1 == 2);\n    return 0;\n}\n").unwrap();
    let output = ae().arg("test").arg(&panicking).output().unwrap();
    assert_eq!(output.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&output.stderr).contains("assertion failed"), "{}", String::from_utf8_lossy(&output.stderr));
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn test_runner_refuses_malformed_suites_and_cleans_up() {
    let dir = unique_temp_dir("refuse");
    let ae = || Command::new(env!("CARGO_BIN_EXE_ae"));
    for broken in [
        // A main is a program, not a suite.
        "fn main() { print(1); }\nfn test_a() -> i64 { return 0; }\n",
        // No test functions at all.
        "fn helper() -> i64 { return 1; }\n",
        // Wrong signature: params.
        "fn test_a(x: i64) -> i64 { return x; }\n",
        // Wrong signature: return type.
        "fn test_a() -> string { return \"s\"; }\n",
        // Generic tests are not supported in v1.
        "fn test_a[T]() -> i64 { return 0; }\n",
    ] {
        let input = dir.join("suite.ae");
        fs::write(&input, broken).unwrap();
        let output = ae().arg("test").arg(&input).output().unwrap();
        assert!(!output.status.success(), "ae test accepted {broken:?}");
        assert!(!String::from_utf8_lossy(&output.stderr).is_empty());
        // The staged copy next to the input must be gone on every path.
        let leftovers: Vec<_> = fs::read_dir(&dir).unwrap()
            .flatten()
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .filter(|n| n.starts_with(".ae-test-"))
            .collect();
        assert!(leftovers.is_empty(), "staging files left behind: {leftovers:?}");
    }
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn test_runner_works_with_modules_and_finds_failures_there() {
    // Tests in the entry file may call helpers from used modules; module
    // files stay plain modules (their tests are not collected in v1).
    let dir = unique_temp_dir("mods");
    let modules = dir.join("modules");
    fs::create_dir(&modules).unwrap();
    fs::write(modules.join("lib.ae"), "pub fn twice(x: i64) -> i64 { return x + x; }\n").unwrap();
    fs::write(modules.join("main_t.ae"),
        "use \"lib.ae\";\n\nfn test_twice() -> i64 {\n    if twice(21) != 42 { return 1; }\n    return 0;\n}\n").unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_ae")).arg("test").arg(modules.join("main_t.ae")).output().unwrap();
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    let stdout = String::from_utf8_lossy(&output.stdout).replace("\r\n", "\n");
    assert_eq!(stdout, "RUN test_twice\nPASS test_twice\nok: 1 passed\n");
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn map_grows_reuses_removed_slots_and_keeps_every_survivor() {
    // ADR-56 halved the load factor, so the table grows at a different point and
    // tombstones interact with growth differently than before. Insert enough keys
    // to grow several times, delete every other one, then insert a second family:
    // survivors must keep their values, removed keys must stay absent, and a slot
    // reused from a tombstone must not shadow a live key.
    let source = r#"
fn main() {
    var m = map_new();
    for i in 0..5000 { let k = "k" + to_string(i); let n = map_inc(m, k, i); }
    print(len(m));
    var removed = 0;
    for i in 0..5000 {
        if i % 2 == 0 {
            let k = "k" + to_string(i);
            if map_remove(m, k) { removed = removed + 1; }
        }
    }
    print(removed);
    print(len(m));
    for i in 0..5000 { let k = "n" + to_string(i); let n = map_inc(m, k, 1); }
    print(len(m));
    var bad = 0;
    for i in 0..5000 {
        let k = "k" + to_string(i);
        let c = map_get(m, k);
        match c {
            IntResult::Ok(n) => { if i % 2 == 0 || n != i { bad = bad + 1; } }
            IntResult::Err(reason) => { if i % 2 == 1 { bad = bad + 1; } }
        }
    }
    for i in 0..5000 {
        let k = "n" + to_string(i);
        if !map_has(m, k) { bad = bad + 1; }
    }
    print(bad);
}
"#;
    let output = execute(source);
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).replace("\r\n", "\n"),
        "5000\n2500\n2500\n7500\n0\n"
    );
}

#[test]
fn map_inc_counts_and_returns_the_new_value() {
    let output = execute(
        "fn main() {\n    var m = map_new();\n    let first = map_inc(m, \"k\", 1);\n    let second = map_inc(m, \"k\", 1);\n    let big = map_inc(m, \"k\", 40);\n    let negative = map_inc(m, \"other\", 0 - 3);\n    print(first);\n    print(second);\n    print(big);\n    print(negative);\n    print(len(m));\n    let seen = map_get(m, \"k\");\n    match seen {\n        IntResult::Ok(n) => { print(n); }\n        IntResult::Err(reason) => { print(\"missing\"); }\n    }\n}");
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    let stdout = String::from_utf8_lossy(&output.stdout).replace("\r\n", "\n");
    assert_eq!(stdout, "1\n2\n42\n-3\n2\n42\n");
}

#[test]
fn map_inc_rejects_bad_shapes_and_moved_maps() {
    let rejects = [
        ("fn main() { var m = map_new(); let n = map_inc(m, 5, 1); print(n); }", "string key"),
        ("fn main() { var m = map_new(); let n = map_inc(m, \"k\", \"x\"); print(n); }", "i64 delta"),
        ("fn main() { var m = map_new(); let b = m; let n = map_inc(m, \"k\", 1); print(n); }", "moved map"),
    ];
    for (source, needle) in rejects {
        let message = check_rejects(source);
        // A moved map is usually caught by the general move rule before the
        // builtin's own check runs; either rejection is correct.
        assert!(message.contains(needle) || message.contains("was moved"), "{source:?}: {message}");
    }
}

#[test]
fn map_inc_works_on_struct_fields_like_the_other_map_builtins() {
    let output = execute(
        "struct Bag { counts: map[string, i64], tag: i64 }\nfn main() {\n    var b = Bag { counts: map_new(), tag: 1 };\n    let first = map_inc(b.counts, \"a\", 2);\n    let now = map_inc(b.counts, \"a\", 2);\n    print(first);\n    print(now);\n    print(len(b.counts));\n}");
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    let stdout = String::from_utf8_lossy(&output.stdout).replace("\r\n", "\n");
    assert_eq!(stdout, "2\n4\n1\n");
}

#[test]
fn trait_dispatch_resolves_per_type_and_matches_the_hand_written_call() {
    let output = execute(
        "trait Area {\n    fn area(shape: T) -> i64;\n}\n\nimpl Area for i64 {\n    fn area(shape: i64) -> i64 { return shape * shape; }\n}\n\nstruct Rect { w: i64, h: i64 }\n\nimpl Area for Rect {\n    fn area(shape: Rect) -> i64 { return shape.w * shape.h; }\n}\n\nfn report[T: Area](shape: T) -> string {\n    return \"area=\" + to_string(area(shape));\n}\n\nfn rect_area(shape: Rect) -> i64 { return shape.w * shape.h; }\n\nfn main() {\n    print(report[i64](7));\n    print(report[Rect](Rect { w: 3, h: 4 }));\n    print(rect_area(Rect { w: 3, h: 4 }));\n}");
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    let stdout = String::from_utf8_lossy(&output.stdout).replace("\r\n", "\n");
    assert_eq!(stdout, "area=49\narea=12\n12\n");
}

#[test]
fn trait_rules_are_enforced() {
    let rejects = [
        // No impl for the argument the call site chose.
        ("trait Area {\n    fn area(shape: T) -> i64;\n}\nimpl Area for i64 {\n    fn area(shape: i64) -> i64 { return shape; }\n}\nfn report[T: Area](shape: T) -> i64 {\n    return area(shape);\n}\nfn main() {\n    print(report[string](\"x\"));\n}", "no impl of trait 'Area' for 'string'"),
        // The body calls the method but the parameter is not constrained.
        ("fn report[T](shape: T) -> i64 {\n    return area(shape);\n}\nfn main() {\n    print(report[i64](1));\n}", "unknown function 'area'"),
        // Two impls for the same (trait, type) pair.
        ("trait Area {\n    fn area(shape: T) -> i64;\n}\nimpl Area for i64 {\n    fn area(shape: i64) -> i64 { return shape; }\n}\nimpl Area for i64 {\n    fn area(shape: i64) -> i64 { return 0; }\n}\nfn main() {\n    print(1);\n}", "duplicate impl of 'Area' for 'i64'"),
        // The impl's signature disagrees with the trait.
        ("trait Area {\n    fn area(shape: T) -> i64;\n}\nimpl Area for i64 {\n    fn area(shape: bool) -> i64 { return 1; }\n}\nfn main() {\n    print(1);\n}", "impl method 'area' must take (i64)"),
        // The bound names something that is not a trait.
        ("fn report[T: Area](shape: T) -> i64 {\n    return 1;\n}\nfn main() {\n    print(report[i64](1));\n}", "unknown constraint"),
        // The impl names an unknown trait.
        ("impl Area for i64 {\n    fn area(shape: i64) -> i64 { return 1; }\n}\nfn main() {\n    print(1);\n}", "impl names unknown trait 'Area'"),
        // The impl invents a method the trait never declared.
        ("trait Area {\n    fn area(shape: T) -> i64;\n}\nimpl Area for i64 {\n    fn width(shape: i64) -> i64 { return 1; }\n}\nfn main() {\n    print(1);\n}", "trait 'Area' has no method 'width'"),
        // The impl's return type disagrees.
        ("trait Area {\n    fn area(shape: T) -> i64;\n}\nimpl Area for i64 {\n    fn area(shape: i64) -> bool { return true; }\n}\nfn main() {\n    print(1);\n}", "impl method 'area' must return i64"),
    ];
    for (source, needle) in rejects {
        let message = check_rejects(source);
        assert!(message.contains(needle), "{source:?}: {message}");
    }
}

#[test]
fn trait_impl_lives_with_its_trait_or_type_and_third_files_are_refused() {
    // Coherence, v1 shape: an impl belongs to the file that declares the
    // trait or the type; an unrelated third file cannot provide the impl.
    let dir = unique_temp_dir("traitmod");
    let modules = dir.join("modules");
    fs::create_dir(&modules).unwrap();
    fs::write(modules.join("shape.ae"),
        "pub trait Area {\n    fn area(shape: T) -> i64;\n}\n\npub struct Rect { w: i64, h: i64 }\n\nimpl Area for Rect {\n    fn area(shape: Rect) -> i64 { return shape.w * shape.h; }\n}\n").unwrap();
    fs::write(modules.join("main_t.ae"),
        "use \"shape.ae\";\n\nfn report[T: Area](shape: T) -> i64 {\n    return area(shape);\n}\n\nfn main() {\n    print(report[Rect](Rect { w: 3, h: 4 }));\n}\n").unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_ae")).arg("run").arg(modules.join("main_t.ae")).output().unwrap();
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    assert_eq!(String::from_utf8_lossy(&output.stdout).replace("\r\n", "\n"), "12\n");

    // A third file that declares neither the trait nor the type may not
    // write the impl, even though both are exported.
    fs::write(modules.join("third.ae"),
        "use \"shape.ae\";\n\nimpl Area for Rect {\n    fn area(shape: Rect) -> i64 { return 0; }\n}\n").unwrap();
    fs::write(modules.join("main_bad.ae"),
        "use \"third.ae\";\nfn main() {\n    print(1);\n}\n").unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_ae")).arg("run").arg(modules.join("main_bad.ae")).output().unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("impl of 'Area' for 'struct"), "{}", String::from_utf8_lossy(&output.stderr));
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn check_json_reports_structured_diagnostics_or_an_empty_array() {
    let ae = || Command::new(env!("CARGO_BIN_EXE_ae"));

    let dir = unique_temp_dir("checkjson");
    let good = dir.join("good.ae");
    fs::write(&good, "fn main() { print(1); }\n").unwrap();
    let output = ae().arg("check").arg("--json").arg(&good).output().unwrap();
    assert!(output.status.success());
    assert_eq!(String::from_utf8_lossy(&output.stdout).replace("\r\n", "\n").trim(), "[]");
    // The flag is also accepted after the path; both spellings are documented.
    let output = ae().arg("check").arg(&good).arg("--json").output().unwrap();
    assert!(output.status.success());
    assert_eq!(String::from_utf8_lossy(&output.stdout).replace("\r\n", "\n").trim(), "[]");

    let bad = dir.join("bad.ae");
    fs::write(&bad, "fn main() { print(who_am_i); }\n").unwrap();
    let output = ae().arg("check").arg("--json").arg(&bad).output().unwrap();
    assert!(!output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout).replace("\r\n", "\n");
    assert!(stdout.trim_start().starts_with('['), "{stdout}");
    assert!(stdout.contains("\"file\""), "{stdout}");
    assert!(stdout.contains("\"line\":"), "{stdout}");
    assert!(stdout.contains("\"column\":"), "{stdout}");
    assert!(stdout.contains("unknown variable"), "{stdout}");
    assert!(stdout.trim_end().ends_with(']'), "{stdout}");
    // Windows paths carry backslashes, so the escaping must survive them; on
    // Unix the separator is `/` and the same assertion would be vacuous, so
    // the path itself is the thing worth checking.
    #[cfg(windows)]
    assert!(stdout.contains("\\\\"), "separators not escaped: {stdout}");
    #[cfg(unix)]
    assert!(stdout.contains("\"file\":\"/"), "absolute unix path missing: {stdout}");
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn check_json_recovers_impl_methods_and_functions_in_source_order() {
    let dir = unique_temp_dir("json-impl-recovery");
    let path = dir.join("bad.ae");
    fs::write(&path, "trait Area { fn area(x: T) -> i64; fn width(x: T) -> i64; }\nfn before() -> i64 { return missing_before; }\nimpl Area for i64 {\n fn area(x: i64) -> i64 { return missing_area; }\n fn width(x: i64) -> i64 { return missing_width; }\n}\nfn main() { print(missing_main); }\n").unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_ae")).arg("check").arg("--json").arg(&path).output().unwrap();
    assert!(!output.status.success());
    let json = String::from_utf8(output.stdout).unwrap();
    let positions: Vec<_> = ["missing_before", "missing_area", "missing_width", "missing_main"].iter()
        .map(|name| json.find(name).unwrap_or_else(|| panic!("missing {name}: {json}"))).collect();
    assert!(positions.windows(2).all(|w| w[0] < w[1]), "{json}");
    assert_eq!(json.matches("\"message\":").count(), 4, "{json}");
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn check_json_recovers_independent_function_body_errors() {
    let dir = unique_temp_dir("json-multiple-bodies");
    let path = dir.join("bad.ae");
    fs::write(&path, "fn first() -> i64 { return missing_a; }\nfn second() -> i64 { return missing_b; }\nfn main() { print(missing_c); }\n").unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_ae")).arg("check").arg("--json").arg(&path).output().unwrap();
    assert!(!output.status.success());
    let json = String::from_utf8(output.stdout).unwrap();
    assert_eq!(json.matches("\"message\":").count(), 3, "{json}");
    for name in ["missing_a", "missing_b", "missing_c"] { assert!(json.contains(name), "{json}"); }
    let output = Command::new(env!("CARGO_BIN_EXE_ae")).arg("check").arg(&path).output().unwrap();
    assert!(!output.status.success());
    let text = String::from_utf8_lossy(&output.stderr);
    assert!(text.contains("missing_a") && !text.contains("missing_b"), "{text}");
    fs::write(&path, "fn identity[T](x: T) -> T { return x; }\nfn first() -> i64 { return identity[i64](true); }\nfn good() -> i64 { return identity[i64](7); }\nfn last() -> i64 { return missing_last; }\nfn main() {}\n").unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_ae")).arg("check").arg("--json").arg(&path).output().unwrap();
    assert!(!output.status.success());
    let json = String::from_utf8(output.stdout).unwrap();
    assert_eq!(json.matches("\"message\":").count(), 2, "{json}");
    assert!(json.contains("missing_last"), "{json}");
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn parser_diagnostic_retains_current_token_range() {
    let dir = unique_temp_dir("diagnostic-range");
    let path = dir.join("bad.ae");
    let source = "// 中文\nfn main() { let value = ; }\n";
    fs::write(&path, source).unwrap();
    let error = ae_compiler::frontend::check_path_diagnostic(&path).unwrap_err();
    let ae_compiler::frontend::FrontendError::Diagnostic(diagnostic) = error else { panic!("expected a parser diagnostic") };
    let span = diagnostic.primary.unwrap().span;
    let offset = source.find(';').unwrap();
    assert_eq!(span.range, offset..offset + 1);
    assert_eq!((span.line, span.column), (2, 25));
    let source = "fn main() { print(unknown_name); }\n";
    fs::write(&path, source).unwrap();
    let error = ae_compiler::frontend::check_path_diagnostic(&path).unwrap_err();
    let ae_compiler::frontend::FrontendError::Diagnostic(diagnostic) = error else { panic!("expected a parser diagnostic") };
    let span = diagnostic.primary.unwrap().span;
    let offset = source.find("unknown_name").unwrap();
    assert_eq!(&source[span.range], "unknown_name");
    assert_eq!((span.line, span.column), (1, offset + 1));
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn duplicate_binding_errors_label_the_earlier_declaration() {
    let dir = unique_temp_dir("duplicate-secondary");
    let path = dir.join("case.ae");
    for (source, marker) in [
        ("// 中文\nfn main() { let value = 1; let value = 2; }", "value"),
        ("fn f(value: i64) -> i64 { let value = 2; return value; } fn main() {}", "value"),
    ] {
        fs::write(&path, source).unwrap();
        let error = ae_compiler::frontend::check_path_diagnostic(&path).unwrap_err();
        let ae_compiler::frontend::FrontendError::Diagnostic(diagnostic) = error else { panic!("expected one diagnostic") };
        assert_eq!(diagnostic.code.as_deref(), Some("E0104"));
        assert_eq!(diagnostic.secondary.len(), 1);
        let start = source.find(marker).unwrap();
        assert_eq!(diagnostic.secondary[0].span.range, start..start + marker.len());
        assert_eq!(diagnostic.secondary[0].message.as_deref(), Some("binding declared here"));
    }
    fs::write(&path, "fn main() { if true { let value = 1; } if true { let value = 2; } }").unwrap();
    assert!(ae_compiler::frontend::check_path_diagnostic(&path).is_ok());
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn move_errors_label_known_declarations_without_claiming_move_site() {
    let dir = unique_temp_dir("move-secondary");
    let path = dir.join("case.ae");
    for (source, marker) in [
        ("fn main() { let original = \"x\"; let other = original; print(original); }", "original"),
        ("fn f(item: string) -> i64 { let other = item; print(item); return 0; } fn main() {}", "item"),
        ("fn main() { let original = \"x\"; if true { let other = original; } print(original); }", "original"),
        ("fn f[T](item: T) -> T { let other = item; return item; } fn main() { let result = f[string](\"x\"); print(result); }", "item"),
    ] {
        fs::write(&path, source).unwrap();
        let error = ae_compiler::frontend::check_path_diagnostic(&path).unwrap_err();
        let ae_compiler::frontend::FrontendError::Diagnostic(diagnostic) = error else { panic!("expected one diagnostic") };
        assert_eq!(diagnostic.code.as_deref(), Some("E0102"));
        assert_eq!(diagnostic.secondary.len(), 1);
        let label = &diagnostic.secondary[0];
        let start = source.find(marker).unwrap();
        assert_eq!(label.span.range, start..start + marker.len());
        assert_eq!(label.message.as_deref(), Some("binding declared here"));
        assert!(diagnostic.suggestions.is_empty());
    }
    fs::write(&path, "fn main() { var original = \"x\"; let other = original; original = \"y\"; print(original); }").unwrap();
    assert!(ae_compiler::frontend::check_path_diagnostic(&path).is_ok());
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn immutable_binding_diagnostics_include_only_known_declarations() {
    let dir = unique_temp_dir("diagnostic-secondary");
    let path = dir.join("case.ae");
    for (source, marker) in [
        ("// 中文\nfn main() { let value = 1; value = 2; }", "value"),
        ("fn f(parameter: i64) -> i64 { parameter = 2; return parameter; } fn main() {}", "parameter"),
        ("fn f[T](item: T) -> T { item = item; return item; } fn main() { print(f[i64](1)); }", "item"),
    ] {
        fs::write(&path, source).unwrap();
        let error = ae_compiler::frontend::check_path_diagnostic(&path).unwrap_err();
        let ae_compiler::frontend::FrontendError::Diagnostic(diagnostic) = error else { panic!("expected one diagnostic") };
        assert_eq!(diagnostic.code.as_deref(), Some("E0103"));
        assert_eq!(diagnostic.secondary.len(), 1);
        let label = &diagnostic.secondary[0];
        let start = source.find(marker).unwrap();
        assert_eq!(label.span.range, start..start + marker.len());
        assert_eq!(label.message.as_deref(), Some("binding declared here"));
        let output = Command::new(env!("CARGO_BIN_EXE_ae")).arg("check").arg("--json").arg(&path).output().unwrap();
        assert!(!output.status.success());
        let json = String::from_utf8(output.stdout).unwrap();
        assert!(json.contains(&format!("\"byte_start\":{start},\"byte_end\":{},\"message\":\"binding declared here\"", start + marker.len())), "{json}");
    }
    fs::write(&path, "fn main() { for i in 0..2 { i = 1; } }").unwrap();
    let error = ae_compiler::frontend::check_path_diagnostic(&path).unwrap_err();
    let ae_compiler::frontend::FrontendError::Diagnostic(diagnostic) = error else { panic!("expected one diagnostic") };
    assert_eq!(diagnostic.code.as_deref(), Some("E0103"));
    assert!(diagnostic.secondary.is_empty());
    assert!(diagnostic.suggestions.is_empty());
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn check_json_reports_only_applicable_suggestions() {
    let dir = unique_temp_dir("json-suggestions");
    let path = dir.join("case.ae");
    fs::write(&path, "fn main() { let n = 1; n = 2; }").unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_ae")).arg("check").arg("--json").arg(&path).output().unwrap();
    assert!(!output.status.success());
    let json = String::from_utf8(output.stdout).unwrap();
    assert!(json.contains("\"suggestions\":[\"If mutation is intended, declare the binding with `var`.\"]"), "{json}");
    let output = Command::new(env!("CARGO_BIN_EXE_ae")).arg("check").arg(&path).output().unwrap();
    assert!(!String::from_utf8_lossy(&output.stderr).contains("If mutation is intended"));
    fs::write(&path, "fn main() { print(\"bad\\q\"); }").unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_ae")).arg("check").arg("--json").arg(&path).output().unwrap();
    let json = String::from_utf8(output.stdout).unwrap();
    assert!(json.contains("\"code\":\"E0002\""), "{json}");
    assert!(json.contains("\"suggestions\":[\"Use a supported escape, or escape a literal backslash as \\\\\\\\.\"]"), "{json}");
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn check_json_assigns_stable_codes_to_common_semantic_errors() {
    let dir = unique_temp_dir("json-semantic-codes");
    let path = dir.join("case.ae");
    for (source, code) in [
        ("fn main() { print(missing); }", "E0101"),
        ("fn main() { let a = \"x\"; let b = a; print(a); }", "E0102"),
        ("fn main() { let n = 1; n = 2; }", "E0103"),
        ("fn main() { let n = 1; let n = 2; }", "E0104"),
    ] {
        fs::write(&path, source).unwrap();
        let output = Command::new(env!("CARGO_BIN_EXE_ae")).arg("check").arg("--json").arg(&path).output().unwrap();
        assert!(!output.status.success(), "{source}");
        let json = String::from_utf8(output.stdout).unwrap();
        assert!(json.contains(&format!("\"code\":\"{code}\"")), "{source}: {json}");
    }
    fs::write(&path, "fn main() { var n = 1; n = 2; print(n); }").unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_ae")).arg("check").arg("--json").arg(&path).output().unwrap();
    assert!(output.status.success());
    assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), "[]");
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn check_json_reports_eof_range_and_unknown_ranges_as_null() {
    let dir = unique_temp_dir("json-range-boundaries");
    let path = dir.join("eof.ae");
    let source = "// 中文\nfn main() {";
    fs::write(&path, source).unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_ae")).arg("check").arg("--json").arg(&path).output().unwrap();
    assert!(!output.status.success());
    let json = String::from_utf8(output.stdout).unwrap();
    assert!(json.contains(&format!("\"byte_start\":{},\"byte_end\":{}", source.len(), source.len())), "{json}");
    let missing = dir.join("missing.ae");
    let output = Command::new(env!("CARGO_BIN_EXE_ae")).arg("check").arg("--json").arg(&missing).output().unwrap();
    assert!(!output.status.success());
    let json = String::from_utf8(output.stdout).unwrap();
    assert!(json.contains("\"byte_start\":null,\"byte_end\":null"), "{json}");
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn check_json_collects_lexical_errors_across_sibling_imports() {
    let dir = unique_temp_dir("json-import-lex");
    fs::create_dir(dir.join("sub")).unwrap();
    let entry = dir.join("main.ae");
    fs::write(&entry, "use \"left.ae\";\nuse \"right.ae\";\nfn main() {}\n").unwrap();
    fs::write(dir.join("left.ae"), "use \"shared.ae\";\n").unwrap();
    fs::write(dir.join("right.ae"), "use \"sub/../shared.ae\";\nuse \"other.ae\";\n").unwrap();
    fs::write(dir.join("shared.ae"), "@\n").unwrap();
    fs::write(dir.join("other.ae"), "#\n").unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_ae")).arg("check").arg("--json").arg(&entry).output().unwrap();
    assert!(!output.status.success());
    let json = String::from_utf8(output.stdout).unwrap();
    assert_eq!(json.matches("\"message\":").count(), 2, "{json}");
    assert!(json.find("shared.ae").unwrap() < json.find("other.ae").unwrap(), "{json}");
    let output = Command::new(env!("CARGO_BIN_EXE_ae")).arg("check").arg(&entry).output().unwrap();
    let text = String::from_utf8_lossy(&output.stderr);
    assert!(!output.status.success());
    assert!(text.contains("shared.ae") && !text.contains("other.ae"), "{text}");
    // An unreadable sibling is reported without suppressing later branches.
    fs::write(dir.join("right.ae"), "use \"missing.ae\";\nuse \"other.ae\";\n").unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_ae")).arg("check").arg("--json").arg(&entry).output().unwrap();
    let json = String::from_utf8(output.stdout).unwrap();
    assert_eq!(json.matches("\"message\":").count(), 3, "{json}");
    assert!(json.find("shared.ae").unwrap() < json.find("missing.ae").unwrap(), "{json}");
    assert!(json.find("missing.ae").unwrap() < json.find("other.ae").unwrap(), "{json}");
    assert!(json.contains("cannot open"), "{json}");
    // A malformed import header and an import cycle are branch-local errors.
    fs::write(dir.join("malformed.ae"), "use bad;\n").unwrap();
    fs::write(dir.join("cycle.ae"), "use \"cycle.ae\";\n").unwrap();
    fs::write(dir.join("right.ae"), "use \"malformed.ae\";\nuse \"cycle.ae\";\nuse \"other.ae\";\n").unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_ae")).arg("check").arg("--json").arg(&entry).output().unwrap();
    let json = String::from_utf8(output.stdout).unwrap();
    assert_eq!(json.matches("\"message\":").count(), 4, "{json}");
    assert!(json.contains("expected a string path") && json.contains("circular import") && json.contains("other.ae"), "{json}");
    fs::write(dir.join("right.ae"), "use \"other.ae\";\n").unwrap();
    fs::write(dir.join("shared.ae"), "@\n".repeat(20)).unwrap();
    fs::write(dir.join("other.ae"), "#\n".repeat(20)).unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_ae")).arg("check").arg("--json").arg(&entry).output().unwrap();
    let json = String::from_utf8(output.stdout).unwrap();
    assert_eq!(json.matches("\"message\":").count(), 32, "{json}");
    // A semantic failure in the first branch must not be replaced by a
    // later branch's lexical failure discovered by supplementary scanning.
    fs::write(dir.join("shared.ae"), "fn bad() -> i64 { return unknown_name; }\n").unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_ae")).arg("check").arg("--json").arg(&entry).output().unwrap();
    let json = String::from_utf8(output.stdout).unwrap();
    assert!(json.contains("unknown_name") && !json.contains("unexpected character"), "{json}");
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn check_json_reports_multiple_lexical_errors_without_accepting_program() {
    let dir = unique_temp_dir("json-multiple-lex");
    let path = dir.join("bad.ae");
    fs::write(&path, "// 注释中的 @ 不报错\n@ 中 #\nfn main() {}\n").unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_ae"))
        .arg("check").arg("--json").arg(&path).output().unwrap();
    assert!(!output.status.success());
    assert!(output.stderr.is_empty());
    let json = String::from_utf8(output.stdout).unwrap();
    assert_eq!(json.matches("\"message\":").count(), 3, "{json}");
    for (column, character) in [(1, "@"), (3, "中"), (5, "#")] {
        assert!(json.contains(&format!("\"line\":2,\"column\":{column},\"message\":\"unexpected character '{character}'\"")), "{json}");
    }
    let output = Command::new(env!("CARGO_BIN_EXE_ae")).arg("check").arg(&path).output().unwrap();
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("2:1: unexpected character '@'"), "{stderr}");
    assert!(!stderr.contains("unexpected character '中'"), "{stderr}");
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn check_json_preserves_diagnostic_location_in_imported_file() {
    let dir = unique_temp_dir("json-lex");
    #[cfg(windows)]
    let name = "模块.ae";
    #[cfg(not(windows))]
    let name = "模块: part.ae";
    let imported = dir.join(name);
    let entry = dir.join("main.ae");
    let source = "// 中文\n  @\n";
    fs::write(&imported, source).unwrap();
    fs::write(&entry, format!("use \"{name}\";\nfn main() {{}}\n")).unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_ae"))
        .arg("check").arg("--json").arg(&entry).output().unwrap();
    assert!(!output.status.success());
    assert!(output.stderr.is_empty());
    let escaped = imported.display().to_string().replace('\\', "\\\\").replace('"', "\\\"");
    let start = source.find('@').unwrap();
    let end = start + 1;
    let expected = format!("[{{\"file\":\"{escaped}\",\"line\":2,\"column\":3,\"message\":\"unexpected character '@'\",\"byte_start\":{start},\"byte_end\":{end},\"code\":\"E0001\",\"suggestions\":[],\"secondary\":[]}}]");
    assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), expected);
    let source = "// 中文\nuse nope;\n";
    fs::write(&imported, source).unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_ae"))
        .arg("check").arg("--json").arg(&entry).output().unwrap();
    assert!(!output.status.success());
    assert!(output.stderr.is_empty());
    let start = source.find("nope").unwrap();
    let end = start + "nope".len();
    let expected = format!("[{{\"file\":\"{escaped}\",\"line\":2,\"column\":5,\"message\":\"expected a string path after 'use'\",\"byte_start\":{start},\"byte_end\":{end},\"code\":null,\"suggestions\":[],\"secondary\":[]}}]");
    assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), expected);
    let source = "pub fn broken() -> i64 { return nope; }\n";
    fs::write(&imported, source).unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_ae"))
        .arg("check").arg("--json").arg(&entry).output().unwrap();
    assert!(!output.status.success());
    assert!(output.stderr.is_empty());
    let column = source.find("nope").unwrap() + 1;
    let start = column - 1;
    let end = start + "nope".len();
    let expected = format!("[{{\"file\":\"{escaped}\",\"line\":1,\"column\":{column},\"message\":\"unknown variable 'nope'\",\"byte_start\":{start},\"byte_end\":{end},\"code\":\"E0101\",\"suggestions\":[],\"secondary\":[]}}]");
    assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), expected);
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn numeric_lexical_errors_recover_in_json_without_accepting_program() {
    let dir = unique_temp_dir("json-numeric-recovery");
    let path = dir.join("bad.ae");
    fs::write(&path, "// unicode: 中文\nfn main() { print(9223372036854775808); @ print(1e999); print(1e-999); }\n").unwrap();
    let json = Command::new(env!("CARGO_BIN_EXE_ae")).arg("check").arg("--json").arg(&path).output().unwrap();
    assert!(!json.status.success());
    assert!(json.stderr.is_empty());
    let text = String::from_utf8(json.stdout).unwrap();
    let positions: Vec<_> = ["E0005", "E0001", "E0007", "E0008"].iter().map(|code| text.find(code).unwrap()).collect();
    assert!(positions.windows(2).all(|pair| pair[0] < pair[1]), "{text}");
    assert_eq!(text.matches("\"code\":").count(), 4, "{text}");
    let plain = Command::new(env!("CARGO_BIN_EXE_ae")).arg("check").arg(&path).output().unwrap();
    assert!(!plain.status.success());
    let stderr = String::from_utf8_lossy(&plain.stderr);
    assert!(stderr.contains("integer literal exceeds i64 maximum"), "{stderr}");
    assert!(!stderr.contains("unexpected character"), "{stderr}");
    let generated = Command::new(env!("CARGO_BIN_EXE_ae")).arg("emit-c").arg(&path).output().unwrap();
    assert!(!generated.status.success());
    assert!(generated.stdout.is_empty());
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn invalid_escapes_in_closed_strings_allow_json_lexical_recovery() {
    let dir = unique_temp_dir("json-string-recovery");
    let path = dir.join("bad.ae");
    fs::write(&path, "fn main() { print(\"bad\\q and \\z\"); @ }\n").unwrap();
    let json = Command::new(env!("CARGO_BIN_EXE_ae")).arg("check").arg("--json").arg(&path).output().unwrap();
    assert!(!json.status.success());
    assert!(json.stderr.is_empty());
    let text = String::from_utf8(json.stdout).unwrap();
    assert_eq!(text.matches("\"code\":\"E0002\"").count(), 2, "{text}");
    assert_eq!(text.matches("\"code\":\"E0001\"").count(), 1, "{text}");
    let plain = Command::new(env!("CARGO_BIN_EXE_ae")).arg("check").arg(&path).output().unwrap();
    assert!(!plain.status.success());
    assert!(String::from_utf8_lossy(&plain.stderr).contains("unsupported string escape"));
    let generated = Command::new(env!("CARGO_BIN_EXE_ae")).arg("emit-c").arg(&path).output().unwrap();
    assert!(!generated.status.success());
    assert!(generated.stdout.is_empty());
    fs::remove_dir_all(dir).unwrap();
}


#[test]
fn check_json_recovers_independent_unknown_prints_in_one_body() {
    let dir = unique_temp_dir("json-multiple-prints");
    let path = dir.join("bad.ae");
    fs::write(&path, "fn main() { print(first_missing); print(second_missing); print(third_missing); }\n").unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_ae")).arg("check").arg("--json").arg(&path).output().unwrap();
    assert!(!output.status.success());
    let json = String::from_utf8(output.stdout).unwrap();
    let positions: Vec<_> = ["first_missing", "second_missing", "third_missing"].iter().map(|name| json.find(name).unwrap()).collect();
    assert!(positions.windows(2).all(|pair| pair[0] < pair[1]), "{json}");
    assert_eq!(json.matches("\"code\":\"E0101\"").count(), 3, "{json}");
    let plain = Command::new(env!("CARGO_BIN_EXE_ae")).arg("check").arg(&path).output().unwrap();
    assert!(!plain.status.success());
    let stderr = String::from_utf8_lossy(&plain.stderr);
    assert!(stderr.contains("first_missing") && !stderr.contains("second_missing"), "{stderr}");
    let generated = Command::new(env!("CARGO_BIN_EXE_ae")).arg("emit-c").arg(&path).output().unwrap();
    assert!(!generated.status.success() && generated.stdout.is_empty());
    fs::write(&path, "fn main() { print(first_missing + 1); print(second_missing); }\n").unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_ae")).arg("check").arg("--json").arg(&path).output().unwrap();
    let json = String::from_utf8(output.stdout).unwrap();
    assert!(json.contains("first_missing") && !json.contains("second_missing"), "{json}");
    let many = (0..40).map(|i| format!("print(missing_{i}); ")).collect::<String>();
    fs::write(&path, format!("fn main() {{ {many} }}\n")).unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_ae")).arg("check").arg("--json").arg(&path).output().unwrap();
    let json = String::from_utf8(output.stdout).unwrap();
    assert_eq!(json.matches("\"code\":\"E0101\"").count(), 32, "{json}");
    assert!(!json.contains("missing_32"), "{json}");
    fs::remove_dir_all(dir).unwrap();
}


#[test]
fn check_json_recovers_independent_unknown_conditions() {
    let dir = unique_temp_dir("json-multiple-conditions");
    let path = dir.join("bad.ae");
    fs::write(&path, "fn main() { assert(first_missing); if second_missing { print(third_missing); } while fourth_missing { assert(fifth_missing); } }\n").unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_ae")).arg("check").arg("--json").arg(&path).output().unwrap();
    assert!(!output.status.success());
    let json = String::from_utf8(output.stdout).unwrap();
    let positions: Vec<_> = ["first_missing", "second_missing", "third_missing", "fourth_missing", "fifth_missing"].iter().map(|name| json.find(name).unwrap()).collect();
    assert!(positions.windows(2).all(|pair| pair[0] < pair[1]), "{json}");
    assert_eq!(json.matches("\"code\":\"E0101\"").count(), 5, "{json}");
    fs::write(&path, "fn main() { assert(first_missing && true); print(second_missing); }\n").unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_ae")).arg("check").arg("--json").arg(&path).output().unwrap();
    let json = String::from_utf8(output.stdout).unwrap();
    assert!(json.contains("first_missing") && !json.contains("second_missing"), "{json}");
    fs::remove_dir_all(dir).unwrap();
}


#[test]
fn json_lexical_recovery_follows_verified_import_prefixes() {
    let dir = unique_temp_dir("json-lex-prefix");
    let entry = dir.join("main.ae");
    fs::write(&entry, "use \"child.ae\";\nfn main() { print(\"bad\\q\"); }\n").unwrap();
    fs::write(dir.join("child.ae"), "use \"nested.ae\";\n@\n").unwrap();
    fs::write(dir.join("nested.ae"), "#\n").unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_ae")).arg("check").arg("--json").arg(&entry).output().unwrap();
    assert!(!output.status.success());
    let json = String::from_utf8(output.stdout).unwrap();
    let positions: Vec<_> = ["E0002", "E0001", "nested.ae"].iter().map(|item| json.find(item).unwrap()).collect();
    assert!(positions.windows(2).all(|pair| pair[0] < pair[1]), "{json}");
    assert_eq!(json.matches("\"code\":").count(), 3, "{json}");
    // A semicolon inside the malformed string is not an import boundary.
    fs::write(&entry, "use \"child.ae\";\nfn main() { print(\"text; bad\\q\"); }\n").unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_ae")).arg("check").arg("--json").arg(&entry).output().unwrap();
    let json = String::from_utf8(output.stdout).unwrap();
    assert!(json.contains("child.ae") && json.contains("nested.ae"), "{json}");
    // A malformed import before the lexical error is not guessed or traversed.
    fs::write(&entry, "use nope;\n@\n").unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_ae")).arg("check").arg("--json").arg(&entry).output().unwrap();
    let json = String::from_utf8(output.stdout).unwrap();
    assert_eq!(json.matches("\"code\":").count(), 1, "{json}");
    assert!(!json.contains("child.ae"), "{json}");
    fs::remove_dir_all(dir).unwrap();
}
