# ADR-0126: Serialization, Hex/Base64 Encoders, and Checksum Primitives (M1.4 / M3.3)

## Status
Accepted

## Context
Per Language Charter §2.1 and Roadmap M3.3 ("Standard Library: json, csv, regex, log, hash"), network protocols and data storage serialization require robust byte-level length calculation (`base64_encoded_len`, `hex_encoded_len`), hexadecimal character decoders, and deterministic lightweight checksum verification (`crc`, `checksum`).

## Decision
1. Introduce fundamental encoding and verification packages under `std/core/`:
   - `std/core/base64.ae`: Safe block-padded Base64 buffer arithmetic and validation.
   - `std/core/hex.ae`: Hexadecimal digit parsing and validation.
   - `std/core/crc.ae`: CRC32 polynomial accumulator simulation.
   - `std/core/checksum.ae`: 16-bit Fletcher/Adler checksum accumulator.
2. Ensure strict integer boundary checks and zero dynamic memory allocations.

## Verification
- Unit and run snapshot tests in `tests/run/batch_core_encoding.ae`.
- Continuous background load regression on dual platforms.