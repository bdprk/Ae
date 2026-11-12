# ADR-0128: Networking Address, Socket Endpoints, and Path Utilities (M1.4 / M3.2 / M4.6)

## Status
Accepted

## Context
Per Language Charter §2.1 and Roadmap M4.6 ("Networking: net, TCP/UDP/DNS"), network and filesystem abstractions require zero-copy, typed representations for IPv4 addresses (`Ipv4Addr`), endpoint pairs (`SocketAddrV4`), cross-platform path separators (`path_is_sep`), and environment key validators (`env_is_valid_key_char`).

## Decision
1. Introduce fundamental networking and path utilities under `std/core/`:
   - `std/core/ip.ae`: Quad-octet IPv4 representation and 32-bit packing.
   - `std/core/socket.ae`: Socket address binding endpoints and privilege level detection.
   - `std/core/path_utils.ae`: Unified POSIX/Windows path separator checks.
   - `std/core/env_utils.ae`: Environment variable key character validation.
2. Maintain strict zero-allocation and pure value semantics.

## Verification
- Unit and run snapshot tests in `tests/run/batch_core_netpath.ae`.
- Continuous background load regression on dual platforms.