# ADR-0116: Core Duration Layout and Monotonic Time Primitives (M1.4 / M3.3)

## Status
Accepted

## Context
Per Language Charter §2.1 and Roadmap M3.3, time and interval measurements must not rely on raw unstructured integer seconds or floats. A typed `Duration` structure guarantees non-overflow second/nanosecond representation across platform clocks (Windows QPC and Linux `CLOCK_MONOTONIC`).

## Decision
1. Introduce standard `Duration` in `std/core/time.ae`.
2. Encapsulate second and sub-second nanosecond components without float approximations.
3. Constructors `duration_from_secs` and `duration_from_millis` normalize values deterministically.

## Verification
- Unit and run snapshot tests in `tests/run/duration_primitives.ae`.
- Continuous stress regression on dual platforms (Windows x86-64 and Linux AMD EPYC on InstaBox).