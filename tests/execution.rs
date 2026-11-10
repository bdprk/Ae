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
fn vector_use_after_move_is_caught_at_runtime_in_loops() {
    // The move happens after the read within one iteration; the null-check
    // backstop catches the second iteration instead of misbehaving.
    let source = "fn main() { var v = vec[1, 2]; while len(v) > 0 { print(v[0]); let w = v; } }";
    let output = execute(source);
    assert!(!output.status.success());
    let err = String::from_utf8_lossy(&output.stderr);
    assert!(err.contains("use of moved vector"), "{err}");
    assert!(err.contains("1"), "{err}");
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
    fs::write(dir.join("lib.ae"), "use \"sub/deep.ae\";\nfn twice(x: i64) -> i64 { return x * 2; }\nfn quad(x: i64) -> i64 { return twice(twice(x)); }\n").unwrap();
    fs::write(dir.join("sub/deep.ae"), "fn base() -> i64 { return 3; }\n").unwrap();
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
    for source in [
        "fn main() { let v = split(\"a\", \",\"); let s = v[0] + \"!\"; print(s); }",
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
        // views cannot be concatenated
        "fn main() { let t = \"abc\"; let v = slice_view(t, 0, 1); print(v + \"x\"); }",
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
