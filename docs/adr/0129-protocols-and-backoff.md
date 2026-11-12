# ADR-0129: Application Protocols, URI Schemes, MIME Types, and Retry Backoff (M1.4 / M4.6 / M4.7)

## Status
Accepted

## Context
Per Language Charter §2.1 and §4.7 ("Networking and HTTP: http 1.1 client/server, TLS, proxy"), building production-grade network utilities and reverse proxies without runtime bloat requires standardized URI scheme definitions (`UriScheme`), HTTP header character validators (`header_is_token_char`), MIME type classifications (`ContentType`), and bounded exponential backoff calculators (`BackoffPolicy`).

## Decision
1. Introduce protocol and networking primitives under `std/core/`:
   - `std/core/uri.ae`: Scheme enumeration, TLS security classification, and default port mapping.
   - `std/core/header.ae`: RFC 7230 header name token validation.
   - `std/core/mime.ae`: Primary MIME media type classifications.
   - `std/core/backoff.ae`: Deterministic bounded exponential retry backoff state machine.
2. Maintain strict zero-allocation and non-moving pure value semantics.

## Verification
- Unit and run snapshot tests in `tests/run/batch_core_protocols.ae`.
- Continuous background load regression on dual platforms.