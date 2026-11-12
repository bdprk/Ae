# Contributing

## Independent implementation

AE is an independently developed implementation. Do not copy implementation code, tests, documentation, or assets from other language projects, including Gojo. Public material may be studied for general ideas, but submitted code and text must be original to this project. Identify design sources in the change description or an ADR when applicable.

## Change requirements

1. Include tests for each feature or fix: positive cases, rejection cases for invalid programs, and regression tests when appropriate.
2. Pass `cargo test --workspace --locked` and `python scripts/verify_generated_c.py`. On Linux, also pass the latter with `--sanitize`.
3. Update `docs/current-language.md` when language behavior changes. Record decisions affecting the long-term design in a new `docs/adr/NNNN.md` using `docs/adr/TEMPLATE.md`.
4. Never trade safety semantics for performance. Checked arithmetic, bounds checks, and checks against use after move must remain in every build mode.
5. Generated C must not introduce undefined behavior. Verify new templates produce consistent results in both optimization modes.
6. Record and compare benchmark data without treating it as proof of universal superiority. Preserve raw data and commands.

## Code style

- The current transitional layout has source positions and diagnostic foundations in `crates/ae-span`, the lexer, tokens, and formatter in `crates/ae-syntax`, and parsing, checking, and C generation in `crates/ae-compiler/src/frontend/`. Test helpers remain in the compiler library for now; `src/main.rs` is the command-line driver. Split crates further as planned and follow the design charter in `docs/adr/0067.md` and the freeze list in `docs/adr/0068.md`.
- Errors should identify the line, column, and cause. Rejection paths require test coverage.
- Explain what changed and why in commit messages; keep each commit focused on one topic.
