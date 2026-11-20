# M0 baseline: checked integer sum on Windows

- Status: exploratory, single-platform verification (Windows x86-64 only).
- Date: 2026-11-12; source revision: `1c7aa525a8804948c0916f2e61583d0522cea808`; tracked working tree was clean during measurement.
- Raw samples, source hashes, outputs and tool versions: [JSON](2026-11-12-windows-sum-baseline.json).
- Purpose: establish an initial reproducible same-host measurement for the existing `sum` workload. It is not an M1 performance acceptance result.

## Environment

Windows 10.0.26300, AMD64, Intel Family 6 Model 141 Stepping 1. Physical memory, temperature, frequency, and power policy were not recorded. Tools: Python 3.10.11, Cargo 1.99.1, rustc 1.99.1, Go 1.26.0, GCC 16.2.0 (MinGW UCRT). The AE release executable SHA-256 was `e5b0cf1fc59229dac9f77dbfcf4951bc7f3fb5ee31aa502dd3f41991e2d8a2d9`.

## Reproduction

From the repository root:

```text
python bench/run.py --release --cold --workload sum
```

The script builds the release AE compiler, then performs five interleaved build and run rounds for AE, Go and Rust. AE uses GCC `-O2` and a fresh AE cache for each build; Rust uses `opt-level=2`; Go uses its default build mode. External toolchain caches are not controlled. The fixed workload sums integers from 0 through 999,999 with checked addition. A separate final execution per language verified output `499999500000`; the individual timed runs were not output-checked. Raw samples are retained in the linked JSON.

## Results

| Language | Build median ms (range) | Run median ms (range) | Executable bytes |
|---|---:|---:|---:|
| AE | 498.98 (489.23–741.65) | 56.35 (53.48–110.91) | 65,076 |
| Go | 1,262.16 (1,146.59–22,658.81) | 201.34 (186.90–267.35) | 2,460,160 |
| Rust | 336.46 (314.18–447.01) | 79.43 (73.24–129.33) | 131,072 |

Each median and range contains five samples. Windows CPU time is unavailable in this runner. Peak working set is sampled in a separate execution and may be unavailable for short processes; build memory excludes compiler child processes. See the raw file for those measurements.

## Interpretation and limitations

This validates the measurement pipeline and records one machine's baseline for a small workload. Process startup is included and can dominate these short runs; Go's first build is a large outlier and external cache states were uncontrolled. Optimization settings and toolchain behavior are not equivalent, ordering was not randomized, and only final outputs were checked. These numbers do not establish general language rankings, production behavior, or Linux performance.
