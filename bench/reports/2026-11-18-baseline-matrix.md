# Baseline Matrix — revision 15321cf

Date: 2026-11-18  ·  Windows 11 x86-64 (rustc 1.99.1, gcc 16.2.0 MinGW-W64 UCRT) and Debian 13 x86-64
(InstaBox evaluate-2, rustc 1.99.0, gcc 14.2.0). Commands run from the repository root; the binary is
`target/debug/ae` built by `cargo build`.

| # | Command | Target | Expected | Observed | Reproducer |
|---|---|---|---|---|---|
| B01 | `git rev-parse HEAD` / `git status -sb` | both | pinned revision, clean tree | `15321cf`, clean | any checkout |
| B02 | `rustc -Vv`, `cargo -V`, `gcc --version` | both | versions recorded per host | Windows: 1.99.1 / 1.99.1 / 16.2.0; Linux: 1.99.0 / 1.99.0 / 14.2.0 | same commands |
| B03 | `cargo test --workspace` | Windows | no failures | 10 + 280 + 3 + 43 + 2 + 23 passed, 0 failed | `cargo test --workspace` |
| B04 | `cargo test --workspace --locked` | Debian 13 | same suites, no host-specific skips | 280 execution + 43 JSON + 3 run snapshots passed | see `2026-11-18-linux-baseline.md` |
| B05 | `ae check examples/hello.ae`; `ae run examples/hello.ae` | both | exit 0, prints `42` | exit 0 / prints `42` / exit 0 | `cargo run -- run examples/hello.ae` |
| B06 | `ae emit-c examples/hello.ae` | both | entry point, runtime calls, linker inputs identified | see below | same command |
| B07 | read `examples/option_propagate.ae` + `ae emit-mir` | both | each owned local disposed on both paths | see below | `ae emit-mir examples/option_propagate.ae` |
| B08 | `ae check` on a missing semicolon; `check --json` | both | one diagnostic, code, exit 1 | text mode: `expected Semi, found RBrace` at 3:1, exit 1; JSON mode: one-object array with `"code":"E0010"` | `printf 'fn main() {\n    print(1)\n}\n' > /tmp/nosemi.ae` |
| B09 | `scripts/compare_emit_c.py`, `scripts/snapshots.py`, `scripts/fmt_acceptance.py` | both | invocation and fixture coverage recorded | see below | `python scripts/snapshots.py --help` |
| B10 | this document | — | matrix published | published | — |

## B06 — emit-c anatomy

- Entry point: `int main(int argc, char **argv)`; it calls `ae_args_init(argc, argv)`, stores the argument
  vector for `args()`, runs the program body in `block_0:`, then `block_tail:` flushes stdout (`fflush(stdout)`)
  and returns 0. A program `exit(n)` leaves through the runtime's exit path instead.
- Runtime calls in the same translation unit: allocator hooks (`ae_os_alloc`/`ae_os_resize`/
  `ae_os_release`, wrapped by `ae_alloc_try` with a budget counter), `ae_print_i64`,
  `ae_args_init`, and the console conversion used on Windows.
- Linker inputs: `gcc -std=c11 -O0|-O2 program.c -o program`, plus `-lm` on POSIX. There is no separate runtime
  object and no non-libc library: libc supplies `malloc`/`realloc`/`free`/`fflush` and the process entry.
- The generated C is one file; `ae build -o` copies the linked executable and never overwrites an existing file.

## B07 — drop paths in `option_propagate.ae`

`upper(true)`: `maybe_find` returns `Some("found")`; the `?` binds the payload to `text` (a move out of the
enum), `text + "!"` allocates a new string and releases the old one, and `Option[string]::Some(...)` moves the
new string into the return value. One owner at every step.

`upper(false)`: `?` observes `None`, releases nothing it does not own, and returns `None` immediately; the concat
and the wrap are never reached, so there is nothing to release on that path.

`discard(flag)`: the bare `maybe_find(flag)?;` statement releases a `Some` payload at the end of the statement
and propagates `None` unchanged.

`count_numbers` / `keep_numbers`: a `vec` payload is respected as owned. `count_numbers` discards it at the
statement end; `keep_numbers` moves it into a fresh `Some`, which is why the second release must not fire — the
`?` success path and the wrap share a single owner.

`ae emit-mir` shows one `releases(...)` in `main` covering the eight option bindings (a, b, c, d, e, f, g, h);
the drop-elaboration pass keeps that release because each binding owns a distinct enum value.

## B09 — script inventory

| Script | Invocation | Coverage |
|---|---|---|
| `scripts/snapshots.py` | `python scripts/snapshots.py [--suite all|ui|json|run] [--bless]`, wraps `cargo test --test snapshots` | `tests/ui` stderr, `tests/ui-json` JSON, `tests/run` stdout/exit |
| `scripts/compare_emit_c.py` | `python scripts/compare_emit_c.py --baseline <exe> --candidate <exe>` | generated C byte-for-byte over the example corpus; self-comparison validates the tool |
| `scripts/fmt_acceptance.py` | `python scripts/fmt_acceptance.py --json <out.json>` | formats a temporary copy of every example, checks idempotence and semantics preservation |

## Reproduction

```sh
cargo build
target/debug/ae check examples/hello.ae
target/debug/ae run examples/hello.ae
target/debug/ae emit-c examples/hello.ae | tail -20
target/debug/ae emit-mir examples/option_propagate.ae | grep releases
```

Host-specific differences observed: none in behaviour; only toolchain versions and wall-clock timings differ.
