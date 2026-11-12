# Frontend reuse cache: no-modification build on Windows

- Status: exploratory, single-platform verification (Windows x86-64 only), single workload.
- Date: 2026-11-12; measured on the working tree that becomes ADR-0177.
- Purpose: record what the frontend reuse cache (ADR-0177) removes from a build that has nothing to do. It is not an M5.4 acceptance result, and it is not a comparison against another language.

## Environment

Windows 10.0.26300, AMD64, Intel Family 6 Model 141 Stepping 1. Physical memory, temperature, frequency, and power policy were not recorded. Tools: rustc 1.99.1, GCC 16.2.0 (MinGW UCRT). Both a debug-built (`target/debug/ae.exe`, 2,295,296 bytes) and a release-built (`target/release/ae.exe`, 1,493,504 bytes) compiler were measured; the two do not share cache entries because the tool identity differs.

## Workload

A synthetic single-file program, `/tmp/fc-big.ae`: 3,002 lines, 1,500 `let` bindings folded into a comparison and 1,500 conditional `print` calls, one `main`. It exercises parse, name resolution, type checking, and C generation without touching the standard library or the filesystem.

## Reproduction

From the repository root, with `/tmp/fc-big.ae` present:

```text
cargo build --release
AE_CACHE=/tmp/fc-m-release AE_TIMINGS=1 ./target/release/ae.exe build /tmp/fc-big.ae -o /tmp/o-a.exe   # cold
AE_CACHE=/tmp/fc-m-release AE_TIMINGS=1 ./target/release/ae.exe build /tmp/fc-big.ae -o /tmp/o-b.exe   # warm
```

The cache directory was deleted before each cold run. The build output path differs between the two runs because `build` refuses to overwrite an existing file; the path is not part of any cache key. Five cold/warm pairs were run for each compiler build. The two executables produced by each pair were byte-identical (`cmp`).

## Results

Milliseconds, as reported by `AE_TIMINGS=1`. The frontend figure includes code generation; the C figure covers `gcc -O2` and linking.

| Compiler build | Run | frontend_and_codegen | c_compile_and_link |
|---|---|---|---|
| debug | cold (5 samples) | 269.560, 274.302, 298.038, 275.700, 292.281 | 698.835, 720.027, 676.050, 700.767, 692.529 |
| debug | warm (5 samples) | 0.000 (frontend cache hit), 5× | 2.126, 1.765, 2.122, 1.742, 2.402 (C cache hit) |
| release | cold (5 samples) | 40.916, 44.450, 44.130, 39.368, 39.609 | 716.776, 721.912, 777.537, 666.437, 644.388 |
| release | warm (5 samples) | 0.000 (frontend cache hit), 5× | 2.592, 1.809, 2.244, 1.695, 2.455 (C cache hit) |

Medians: debug cold 275.7 / 698.8, debug warm 0.0 / 2.1; release cold 40.9 / 716.8, release warm 0.0 / 2.2.

A no-modification build therefore stops paying the frontend entirely: 276 ms on a debug compiler and 41 ms on a release compiler for this program. The remaining warm cost is the cached-C copy plus process startup.

## Limitations

- One program, one size, one machine, one platform. Frontend cost scales with source size; this sample says nothing about other shapes.
- Wall-clock time of the whole `ae build` invocation was not recorded, only the two internal phases.
- The C compilation cache was already warm in the warm rows; the comparison isolates the frontend, not the whole pipeline.
- The program was generated for this measurement and is not one of the `bench/` workloads, so these numbers are not comparable with the `sum`/`text`/`map` reports.
- Warm builds before ADR-0177 ran the frontend every time, so historical "no modification" numbers are not like-for-like with these.
