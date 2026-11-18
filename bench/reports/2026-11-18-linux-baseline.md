# Linux Baseline Verification — evaluate-2 (Debian 13)

Date: 2026-11-18  ·  Revision: `6db7808` (feat: enforce caret version constraints on path dependencies)

## Host

| Field | Value |
|---|---|
| Machine | InstaBox `evaluate-2` (sandbox `m_agqxrfl2wefhmlqhycqag4dmhi`) |
| OS | Debian GNU/Linux 13 (trixie), kernel 6.18.60-instabox x86_64 |
| CPU / RAM | 4 vCPU / 8 GB |
| Rust | cargo 1.99.0 (5f94df478), rustc 1.99.0 (b940084d7) |
| C toolchain | gcc 14.2.0 (Debian 14.2.0-19), GNU ld 2.44 |
| Python | 3.13 (system) |
| Windows reference host | Windows 11 Pro, rustc 1.99.0, same revision |

The box was shared with a concurrent Go benchmark during the first run; the numbers below come from the
re-run after that load started to wind down. Only correctness claims are drawn from them, not timings.

## Setup (reproducer)

```sh
# from the Windows checkout at revision 6db7808
git archive --format=tar HEAD | gzip -1 | ssh instabox-node3 'mkdir -p ~/ae && tar xz -C ~/ae'
ssh instabox-node3 'cd ~/ae && cargo test --workspace --locked'
```

`instabox ssh-cert -ttl 86400 evaluate-2` renews the short-lived SSH certificate (24 h hard ceiling).

## Results

| Layer | Command | Result |
|---|---|---|
| Workspace unit + integration | `cargo test --workspace --locked` | 350 passed, 0 failed (10 bin, 273 execution, 3 snapshots, 42 compiler, 2 span, 20 syntax) |
| Generated-C corpus | `python3 scripts/verify_generated_c.py` | 41 PASS, 0 FAIL |
| Generated-C corpus, ASan + UBSan | `python3 scripts/verify_generated_c.py --sanitize` | 41 PASS, 0 FAIL |
| Differential fuzz (MIR / tree / release / interp) | `python3 scripts/diff_fuzz.py --seeds 40 --jobs 4` | 40 seeds, 0 failures, 3.1 s (13.0 seeds/s) |

## Host-specific observations

- The execution suite is substantially faster on this host than on Windows (2.4 s vs 23.5 s wall), which is
  a property of process startup and the C compiler, not of generated code. No test is skipped on either host:
  the same 273 execution tests run in both places.
- Artifacts are named with a literal `.exe` suffix in package tests (`tool.exe`); the name is a test fixture,
  not a Windows-only requirement — `build -o` writes whatever name it is given and Linux executes it.
- `verify_generated_c.py --leak` (the MinGW `--wrap` allocator) is Windows-specific; the Linux equivalent in this
  report is `--sanitize`, which links AddressSanitizer from gcc 14.

## Verdict

Dual-platform parity for the baseline slice: the same revision passes the same suites on Windows 11 x86_64 and
Debian 13 x86_64, including ASan/UBSan. This satisfies baseline items B01–B05/B09 for the Linux side; AE's own
`unsafe` audit (M2.4 gate) and the 24-hour fuzzing gate (M2.13) remain open.
