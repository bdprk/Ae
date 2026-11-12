# Workload B: File Search (Linux / InstaBox node3)

Date: 2026-11-10 ｜ Input: `bench-input.txt` (20,000 lines, ~1.4 MB, generated with a fixed seed)
Semantics: Count total lines and lines containing the substring `ae`. All four implementations output identically: `20,000` / `12,140`.

Version pinning: AE commit `8bf8000`+ (working tree at the time of archiving this directory); Gojo `b460b17` ("Report where a segfault happened", `git clone --depth 1`); Go 1.27.1; rustc 1.99.0; GCC 14.2.0. Machine: 4 vCPU / 7.8 GiB / Debian 13.

## Results (Median of 7 runs / 3 builds)

| Language | Pure Execution | Cold Build | Hot Build | Artifact Size |
|---|---|---|---|---|
| AE (-O2) | 5.81 ms | 43 ms | 2.5 ms (cache hit) | **16,888 B** |
| Gojo (default optimization) | 3.74 ms | **28 ms** | 28 ms | 230,904 B |
| Go | 2.84 ms | 116 ms | ~116 ms | 2,490,891 B |
| Rust (-O) | **2.35 ms** | — | — | 4,546,568 B |

`gojo run` (compile + run) 35.5 ms; `ae run` hot cache 25.0 ms; `go run` 37.0 ms.

## Fairness Disclosures (Proposal §17)

- **Execution gap stems from the standard library and slice allocations**: Gojo's `str.contains` is an optimized native implementation; the AE version uses a naive byte-by-byte scan in the language (`bytes()` + nested loops, zero allocations). The string representation has been changed to carry its length (ADR-8 evolution, `len`/`slice` in $O(1)$ time); switching to `contains(slice(...))` for line-by-line matching measured 7.58 ms—20,000 small slice allocations remain a cost, whereas Gojo's slices are zero-allocation borrowing views. Borrowing views (such as `&str`) are targeted for the next evolution.
- Go and Rust use `strings.Contains` and `str::lines + contains` from their respective standard libraries; semantics are identical (splitting lines by `\n`, substring matching).
- AE's hot build goes through a content-addressed cache; cold build is a pristine, empty cache. Gojo and Go have no caching tier; their "hot" builds are simply repeated builds (the toolchain's internal cache state is uncontrolled).
- Single-machine, single-workload, small input; this does not constitute a "comprehensive superiority" conclusion. Per Proposal §7.3, this is the first public Linux file search data: AE leads in artifact size (13.7× smaller than Gojo) and hot build speed, while lagging behind Gojo in cold build and execution speed.

## Reproduction

Programs: `grep.ae` (in examples/), `grep.gojo`, `grep.go`, `grep.rs` (archived copies in this directory).
Commands: `ae run examples/grep.ae`, `gojo run grep.gojo`, `go run grep.go`, `rustc -O grep.rs && ./grep_rs`; input generated via `python -c` with a fixed seed (42).

## Re-test Notes (Second round on the same day, after string representation change)

Re-tested after changing strings to length-carrying representations (ADR-8 evolution): AE 7.62 ms / Gojo 5.87 ms (median of 7 runs each). Both are about 30% higher than the first round due to the host bearing other workloads concurrently, causing absolute values to drift; the relative ratio remains stable ($\approx 1.3:1$). The naive byte-scan variant remains AE's fastest variant; the impact of slice allocations and borrowing views is detailed in ADR-8.

## Third Round of Re-testing (After borrowing views landed)

`slice_view` + `contains` grep version: 7.34 ms (GoJo 4.62 / Go 3.86 / Rust 3.16 in the same round). Views eliminated slice allocations, but exposed the true next bottleneck: **byte-by-byte `data[i]` bounds-checked loops** (1.4 million checked accesses, dominating ~5-7 ms). The three reference implementations use memchr-class word-length scan for line splitting. Next steps: native line iteration (e.g., `lines()`) or word-length scan matching—this pertains to standard library maturity rather than the representation itself; string representation and borrowing views themselves are now up to par.

## Fourth Round of Re-testing (After native lines() landed)

`lines()` (runtime 8-byte block SWAR newline scan) + view iteration + `contains`: **6.37 ms** (Gojo 4.62$\rightarrow$4.46 / Go 3.86$\rightarrow$3.50 / Rust 3.16$\rightarrow$2.57 in the same round; host load continued to fluctuate, making ratios across rounds more comparable). The gap with Gojo narrowed from 1.55× to ~1.43×; the remaining gap is due to per-line `strstr` startup overhead × 20,000 lines and allocator behavior, falling into the realm of micro-optimizations. `grep.ae` now shares the same structure as the reference implementations (line-by-line iteration + substring matching), and the code no longer requires manual byte-level scanning.

## Fifth Round of Re-testing (After fused count_lines_with landed)

`count_lines_with(text, pattern)` (single-pass, zero-allocation: memchr to find lines + leading-byte memchr skipped in-line search): **2.14 ms** (Gojo 4.72 / Go 3.99 / Rust 3.17 in the same round; median of 9 runs)—**faster than all reference implementations (2.2× faster than Gojo)**, with the smallest artifact size (16,856 B). CLI version driven by `args()` (reading args + reading file + counting) takes 2.24 ms. Output 12140 ✓.

Fairness: The fused path is a reflection of standard library maturity (Gojo, Go, and Rust all achieve the same semantics via their respective `lines + contains` combinations); the combinatorial path (`lines() + view + contains`) remains available for comparison at 6.37 ms. Evolution records for all five variants can be found in ADR-8 and the round data in this file.