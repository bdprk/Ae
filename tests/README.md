# Snapshot tests

Run `cargo test --test snapshots --locked` from the repository root.

- `ui/*.ae`: compiler checks. A missing or empty `.stderr` means the source must pass; a nonempty `.stderr` is the expected diagnostic.
- `ui-json/*.ae`: `check --json` failure cases. Each `.json` snapshot must contain at least two diagnostics.
- `run/*.ae`: each case runs in both development and release mode. `.stdout` is required; a missing `.stderr` means standard error must be empty. A missing `.exit` means exit code zero; write a nonzero exit code in `.exit` for an expected failure.

Snapshots normalize CRLF to LF. Paths in diagnostics under the test directory are replaced with `$DIR`; program stdout is otherwise kept intact.

Set `AE_BLESS=1` when running the test to update output snapshots. Review the resulting changes. Run-case exit codes are not updated automatically: an unexpected exit code fails before any output snapshot for that case is written. Release output is always compared with the development snapshot, including when blessing.
