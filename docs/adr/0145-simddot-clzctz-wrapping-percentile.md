# ADR-0145: SIMD Dot Product, CLZ/CTZ Bit Scanning, Wrapping Math, and Percentiles

## Context
Under the Standard Library Blueprint (Domain F: Arithmetic Safety & System Metrics), numerical vector math, low-level bit twiddling, bounded circular math, and latency tracking require:
1. Conceptual 4-lane SIMD vector dot product and fused multiply-accumulate (`lane4_vec_dot`, `lane4_vec_fma`).
2. Count leading zeros (CLZ) and count trailing zeros (CTZ) bitwise scanning algorithms.
3. Explicit bounded wrapping math (`wrapping_add_bound`, `wrapping_sub_bound`, `wrapping_mul_bound`).
4. Latency SLA percentile estimation (`p50`, `p99`, `latency_is_sla_breached`).

## Design
- `std/core/simd_dot.ae`: `Lane4Vec`, `lane4_vec_dot`, `lane4_vec_mul`, `lane4_vec_fma`.
- `std/core/clz_ctz.ae`: `count_leading_zeros_i64`, `count_trailing_zeros_i64`.
- `std/core/wrapping_math.ae`: `wrapping_add_bound`, `wrapping_sub_bound`, `wrapping_mul_bound`.
- `std/core/percentile.ae`: `calculate_p50_i64`, `calculate_p99_i64`, `latency_is_sla_breached`.
