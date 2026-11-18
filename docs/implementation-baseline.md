# Implementation baseline

Revision 31e6179, clean Windows checkout. Verified 2026-11-18.

| Ledger | Evidence | Result |
|---|---|---|
| B01–B02 | git status -sb, rustc -Vv, cargo -V and gcc --version | x86_64-pc-windows-msvc, Rust and Cargo 1.99.1, MinGW-W64 GCC 16.2.0 |
| B03 | cargo test --workspace | Exit 0, including all 249 execution tests and all three snapshot suites |
| B04 | GitHub Actions run 44293817056 | Both jobs skipped execution due to account billing. Linux evidence pending |
| B05 | cargo run --quiet -- check examples/hello.ae followed by run | Check silent, program printed 42 |
| B06 | cargo run --quiet -- emit-c examples/hello.ae | C entry point is int main(int argc, char **argv) |
| B07 | cargo run --quiet -- run examples/option_propagate.ae | Printed found!, none, always, none, 1, none, 3, none on separate lines. Ownership trace pending |
| B08 | Inspect CLI and loader calls | Lexer comes from ae-syntax. The production parser is constructed in the compiler loader |
| B09 | Inspect script argument parsers | Invocations below |

On a cache miss the CLI calls compile_path_tracked. This loads source units through loader::load_with_units and calls emit_program. The CLI invokes its selected C compiler to build the executable and launches it for run. Check takes its own route through check_path or check_path_diagnostic, using the same compiler frontend.

- python scripts/compare_emit_c.py --baseline OLD --candidate NEW compares output across compiler binaries.
- python scripts/snapshots.py --suite all checks the recorded suites.
- python scripts/fmt_acceptance.py --json REPORT records formatter results.

The Linux gate remains open. Each future source change should be compared with the pinned Windows baseline and rerun on Linux when a runner is available.
