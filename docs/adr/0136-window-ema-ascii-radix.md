# ADR-0136: Sliding Window Accumulator, EMA Estimator, ASCII Tables, and Radix Helpers

## Context
Telemetry analysis, metrics aggregation, character casing transforms, and number formatting require:
1. Fixed-size 4-slot sliding window accumulators for moving sums and averages.
2. Exponential moving average (EMA) integer estimation for smoothed metrics.
3. Zero-allocation ASCII character casing and hex digit decoding functions.
4. Base-10 and Base-16 digit count estimation for string printer pre-allocation.

## Design
- `std/core/sliding_window.ae`: `WindowAccumulator4`, `window4_push`, `window4_sum`, `window4_average`.
- `std/core/exponential_decay.ae`: `EmaEstimator`, `ema_new`, `ema_update`, `ema_value`.
- `std/core/ascii_table.ae`: `ascii_to_uppercase`, `ascii_to_lowercase`, `ascii_is_hex_digit`, `ascii_hex_to_val`.
- `std/core/radix.ae`: `radix10_digit_count`, `radix16_digit_count`.
