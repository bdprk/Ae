# ADR-0219: Controlled Monotonic Clock and Deterministic Timers (M3.2.05, M4.2.06)

Status: Accepted (2026-11-20)

## Context

- **M3.2.05 (Clocks)** requires driving a timeout with a fake monotonic clock while wall time moves backwards.
- **M4.2.06 (Timer)** requires triggering equal deadlines and cancellation through a controlled clock.

Previously AE only had `now_ms()` / `now_s()` reading the system wall clock, which cannot be controlled or tested deterministically.

## Decision

1. **Types (`clock`, `timer`)**: both are copy-semantics handles (`ae_clock`, `ae_timer`) pruned behind `// unit: ae_clock` in `runtime.c`.
2. **Surface builtins** (all reserved names, non-`unsafe`):
   - `clock_new() -> clock`: creates a virtual monotonic clock starting at `0`.
   - `clock_now(clk) -> i64`: returns the clock's current monotonic timestamp.
   - `clock_advance(clk, delta) -> i64`: advances the clock by `delta` (`delta >= 0`; negative deltas trap with `"clock_advance delta must be non-negative"` to enforce strict monotonicity) and returns the new timestamp.
   - `timer_new(clk, delay) -> timer`: creates a timer due at `clock_now(clk) + delay` (`delay >= 0`).
   - `timer_ready(t) -> bool`: returns `true` iff `!t.cancelled && clock_now(t.clk) >= t.deadline`.
   - `timer_cancel(t) -> bool`: marks `t` cancelled; returns `true` iff `t` was neither already cancelled nor already ready at the moment of cancellation.
3. **Dual-backend parity**: both `runtime.c` (`// unit: ae_clock`) and `interp.rs` implement identical semantics, verified byte-for-byte.

## Consequences

- Closes M3.2.05 and M4.2.06 with zero wall-clock flakiness and zero platform-specific syscalls.
- Programs that do not use `clock` or `timer` emit zero clock/timer runtime symbols.
