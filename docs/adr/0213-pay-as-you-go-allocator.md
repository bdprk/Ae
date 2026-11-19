# ADR-0213: Pay-as-you-go Allocator Linking

- Date: 2026-11-19
- Status: Accepted; implemented in `crates/ae-compiler/src/runtime.c` and `frontend/c_output.rs`, pinned by `scalar_programs_link_without_the_allocator_and_string_programs_keep_it` (tests/execution.rs), `runtime_units_name_marker_regions_across_their_functions`, and the full suite (371 tests, 0 failures).
- Scope: the runtime's allocation interface (ADR-16) and the argv conversion. Implements charter hard rule 8's allocator clause ("no allocation usage → no allocator linked") and the M3.1.01 acceptance probe.

## Evidence and problem

Before this ADR, every program carried the allocator: the interface typedef, the three OS hooks and the checked entry points were reachable from every generated `main` in two ways — `ae_args_init` converted every command-line argument to UTF-8 through `ae_alloc_try` at startup, and the `ae_allocator_impl` initializer sat in the always-emitted non-function text, seeding the hooks into the pruner's roots. A `print(6 * 7)` program therefore referenced `malloc` and linked the CRT heap API. The M3.1.01 probe ("link a scalar program without allocator symbols and inspect imports") failed.

## Decision

- **The allocation interface is one prune unit.** `runtime.c` marks the region (typedef, OS hooks, `ae_allocator_impl`, budget globals) with `// unit: ae_allocator_impl` and ends it with `// unit: -`. `runtime_units` learns markers: a `// unit: <name>` line names the following non-function region, the name spans the region's functions (globals are often interrupted by the functions serving them), and every pending chunk inside carries the same name so keeping the name keeps the whole region. Marker lines are never emitted. A region without a close marker would silently rename later header text, so the close marker is mandatory for correctness, not style.
- **The pruner seeds `ae_set_allocator` only when an allocation user survived the walk.** The documented embedding entry point joins the reachability walk after the first pass if the kept set contains any of `ae_alloc`, `ae_alloc_resize`, `ae_alloc_try`, `ae_free`, `ae_args`. A program that never allocates has no allocator to substitute; a host that substitutes necessarily runs an allocating program, which keeps both the chain and the entry point.
- **The pruner's symbol walk includes comments.** Unit bodies are scanned for `ae_...` identifiers, comments included, and a kept function's comment can seed dependencies. Runtime comments must therefore avoid naming runtime symbols unless the dependency is intended; the one accidental seed this rule caught (a comment in `ae_args_init` naming the args accessor) is documented in the function.
- **The argv conversion is lazy.** `ae_args_init` no longer converts anything; it keeps its call and globals contract with the generated `main` (the B06 entry evidence is unchanged). `ae_args` performs the Windows UTF-16 → UTF-8 conversion once, on first read, and keeps the narrow argv as the fallback when conversion is impossible — the same fallback the startup conversion had. On POSIX nothing changes: `main` receives UTF-8 directly. A program that never reads arguments allocates nothing for them. `/* win32-declared */` moves from `ae_args_init` to `ae_args`, so the header-omission decision keeps working.
- **The import-table baseline is the C toolchain's.** A scalar program's generated C contains zero allocator symbols, but its Windows binary still imports `api-ms-win-crt-heap` because the MinGW CRT startup does for every program, including an empty `int main(void) { return 0; }`. The honest acceptance evidence is the absence of allocator symbols in the generated C, not the absence of the CRT's own baseline imports.

## Alternatives rejected

- Emitting the allocator interface only when a flag is set would shift the decision onto users; reachability from the program text is the whole point of the pruner.
- Making `ae_set_allocator` unconditionally prunable (removing the documented entry point) would break embedding hosts that substitute before any allocation; the conditional seed preserves the contract exactly for the programs where substitution can matter.
- Converting argv lazily but keeping the eager capture (CommandLineToArgvW at startup) would keep the Win32 shell dependency in every program; moving the whole conversion into `ae_args` keeps a never-reading program free of it.

## Required evidence

The CLI test pins both directions: a scalar program's `emit-c` output contains none of `ae_os_alloc`, `ae_allocator_impl`, `ae_alloc`, `ae_free`, `ae_set_allocator`, `malloc` and still runs (`42`, `-7`); a string-concatenation program keeps `ae_allocator_impl` and `ae_alloc` and runs (`hi42`). The existing args fixtures (`program_arguments_arrive_as_utf8_on_every_platform`, run suite) exercise the lazy conversion on both platforms. The unit test pins marker parsing: region naming across the region's functions, the close marker, and marker lines never reaching output. Dual-platform: the args path differs by platform and both are covered by the existing args fixtures; the Windows lazy conversion is the only platform-specific branch.
