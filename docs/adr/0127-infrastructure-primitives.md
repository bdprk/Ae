# ADR-0127: Package Versioning, Priority Scheduling, Metrics, and Rate Limiting (M1.4 / M3.4 / M4.4)

## Status
Accepted

## Context
Per Language Charter §2.1, §4.3 ("Package Management: MVS, versioning"), and §2.6 ("Lightweight Tasks, work-stealing"), robust systems programming demands language-level primitives for semantic version comparison (`SemVer`), scheduler prioritization (`Priority`), telemetry metrics tracking (`CounterGauge`), and token bucket rate limiters (`TokenBucket`).

## Decision
1. Introduce foundational infrastructure packages under `std/core/`:
   - `std/core/version.ae`: Semantic versioning layout and MVS compatibility comparison logic.
   - `std/core/priority.ae`: Scheduler priority levels and weight ordering.
   - `std/core/metric.ae`: Real-time telemetry counter and peak gauge tracking.
   - `std/core/throttle.ae`: Non-blocking token-bucket rate limiter arithmetic.
2. Maintain strict zero-allocation invariants across all modules.

## Verification
- Unit and run snapshot tests in `tests/run/batch_core_infra.ae`.
- Continuous background load regression on dual platforms.