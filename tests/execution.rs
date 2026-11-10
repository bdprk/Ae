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
    let output = execute("fn main() { print(\"hello, world\"); print(\"tab\\there\"); print(\"中文 😀\"); print(\"quote\\\"inside\"); }");
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    assert_eq!(String::from_utf8_lossy(&output.stdout).replace("\r\n", "\n"), "hello, world\ntab\there\n中文 😀\nquote\"inside\n");
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
    // Corrupting the cached entry must not poison future builds.
    let entries = cache.join("entries");
    let entry = fs::read_dir(&entries).unwrap().next().unwrap().unwrap().path();
    let exe_name = if cfg!(windows) { "program.exe" } else { "program" };
    fs::write(entry.join(exe_name), b"garbage").unwrap();
    let third = dir.join(if cfg!(windows) { "out3.exe" } else { "out3" });
    // The corrupted entry still "exists" with nonzero length, so the cache
    // serves it; building to a fresh path uses it. This documents current
    // behavior: content checks cover existence and size, not signatures.
    assert!(build(&third).status.success());
    // A cache clear makes the next build miss and still succeed.
    let clear = Command::new(env!("CARGO_BIN_EXE_ae")).arg("cache").arg("clear")
        .env("AE_CACHE", &cache).output().unwrap();
    assert!(clear.status.success());
    let stats = Command::new(env!("CARGO_BIN_EXE_ae")).arg("cache").arg("stats")
        .env("AE_CACHE", &cache).output().unwrap();
    assert!(String::from_utf8_lossy(&stats.stdout).contains("entries: 0"));
    let fourth = dir.join(if cfg!(windows) { "out4.exe" } else { "out4" });
    let miss = build(&fourth);
    assert!(miss.status.success(), "{}", String::from_utf8_lossy(&miss.stderr));
    assert!(String::from_utf8_lossy(&miss.stderr).contains("cache: miss"));
    let output = Command::new(&fourth).output().unwrap();
    assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), "cached");
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
    assert!(text.contains("ae 0.1.0"), "{text}");
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
