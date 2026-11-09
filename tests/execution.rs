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
    for source in ["fn main() { print(\"bad \\q\"); }", "fn main() { print(\"open }", "fn main() { print(\"a\nb\"); }", "fn main() { let s = \"text\"; }"] {
        let path = std::env::temp_dir().join(format!("ae-str-invalid-{}-{}.ae", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed)));
        fs::write(&path, source).unwrap();
        let result = Command::new(env!("CARGO_BIN_EXE_ae")).arg("check").arg(&path).output().unwrap();
        fs::remove_file(path).unwrap();
        assert!(!result.status.success(), "{source}");
    }
}
