# ADR-0148: Environment Variables, Exit Status, Monotonic Clock, and Pipe IPC

## Context
Under the Standard Library Blueprint (Tier 3 std: Operating Environment & Process Management), child processes, environment discovery, timing, and pipes require:
1. Environment variable key character and bounded length verification (`env_key_len_is_valid`, `is_valid_env_key_char`).
2. Exit status codes (`ExitStatus`, `Success` -> 0, `GeneralFailure` -> 1, `InvalidUsage` -> 2, `NotFound` -> 127, `SignalKilled(sig)` -> 128+sig).
3. Monotonic instant tick delta and elapsed timeout detection (`MonotonicInstant`, `monotonic_instant_elapsed`, `monotonic_instant_has_elapsed`).
4. IPC anonymous pipe channel state machine (`PipeChannelState`, `Open`, `Broken`, `Closed`).

## Design
- `std/core/env_var.ae`: `is_valid_env_key_char`, `env_key_len_is_valid`.
- `std/core/exit_code.ae`: `ExitStatus`, `exit_status_code`, `exit_status_is_success`.
- `std/core/clock_ticks.ae`: `MonotonicInstant`, `monotonic_instant_elapsed`, `monotonic_instant_has_elapsed`.
- `std/core/pipe_state.ae`: `PipeChannelState`, `pipe_state_can_write`, `pipe_state_can_read`.
