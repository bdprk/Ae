# ADR-0219: Controlled Monotonic Clock and Deterministic Timers (M3.2.05, M4.2.06)

Status: Accepted (2026-11-20)

## Context

- **M3.2.05 (Clocks)** requires driving a timeout with a fake monotonic clock while wall time moves backwards.
- **M4.2.06 (Timer)** requires triggering equal deadlines and cancellation through a controlled clock.

Previously ae only had `now_ms()` / `now_s()` reading the system wall clock, which cannot be controlled or tested deterministically.

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

## Amendment 2026-11-20 — ADR-0224 R2.2 (project rename + clock demotion)

The project is now named **ae**. Under the ADR-0224 realignment the fake clock
left the compiler: `clock_new` / `clock_now` / `clock_advance` and the `clock`
handle type were removed (commit `29d2e2e`), and the deterministic clock lives
in `std/test/fake_clock.ae` as an ae module over the `cell_*` primitive family
(one shared i64 cell behind a copy-semantics handle — the same sharing
semantics the builtin handle provided).

Timer signatures changed accordingly — timers take the current time as a
plain value instead of a clock handle:

- `timer_new(now: i64, delay: i64) -> timer` — deadline fixed at creation
  (`now + delay`; `delay < 0` still traps).
- `timer_ready(t: timer, now: i64) -> bool`
- `timer_cancel(t: timer, now: i64) -> bool`

The C runtime timer is now a plain value struct `{ deadline, cancelled }`
(cancel takes a pointer; no allocation, no clock pointer). The negative
delta trap of the old `clock_advance` became a documented no-op in
`std/test/fake_clock.ae` (the language has no panic yet). Migration of
`timer_*` themselves to `std/task` remains open (ADR-0224 R2.4).
