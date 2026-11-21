# ADR-0225: Rename the Language and Toolchain to AE

Status: Implemented (Dual platform)

Date: 2026-11-21

## Context

The project previously used a longer word as its name even though the file
extension was already `.ae`. The binary, the three crates, the environment-variable
and generated-C symbol prefixes, the manifest, and the lockfile all derived from
that word. The old name is long to type in every command, collides with unrelated
software on package registries, and does not match the two-letter extension that
users already type. The rename also had to keep the file extension stable so that
568 existing `.ae` sources, fixtures, and snapshot expectations stay valid.

## Options

| Option | Pros | Cons | Resource Cost (Compile/Runtime/Memory/Disk) |
|---|---|---|---|
| Keep the old name | No migration | Long CLI token, registry collision, mismatched with `.ae` | None |
| Rename to a newly coined word | Distinctive | Breaks the link to `.ae`; churns every identifier again later | Same churn as this option |
| **Rename to `AE`** | Matches `.ae`; shortest possible command (`ae run x.ae`); one-to-one mapping for every prefix | `ae` is a common bigram, so search noise; every internal prefix changes once | Generated C shrinks (shorter symbol names); no runtime cost |

## Decision

`AE` is the language and toolchain name, with a mechanical mapping applied to every
identifier so that no half-renamed surface survives:

- Crate and package names: `ae-span`, `ae-syntax`, `ae-compiler`, binary `ae`.
- Environment variables: `AE_CACHE`, `AE_CC`, `AE_TIMINGS`, `AE_ALLOC_LIMIT`,
  `AE_BLESS`, `AE_OFFLINE`, `AE_CODEGEN`, `AE_VIEW`, and friends.
- Generated C symbols and macros: `ae_*` (e.g. `ae_alloc`, `ae_thread_spawn`,
  `AE_INT_RESULT`). The prefix length and the slice width in
  `c_output::ae_symbols` are retuned together, since both encode the prefix.
- Package manifest `ae.toml`, lockfile `ae.lock`, lockfile header `ae-lock-v1`, and
  the manifest's toolchain key `ae = "0.1"`.
- Diagnostics keep their positional shape: `ae:<line>:<column>: <message>`.
- The file extension stays `.ae`, and it now reads as the initial letters of the
  name plus `e`, so no source file is renamed.
- Commit messages, paths, and file contents all agree with the new name; there is
  no legacy spelling left to learn.

## Evidence

- `rg -i` over the tree: 0 files (was 539).
- `cargo test --workspace` after the rename: 453 passing on Windows
  (14 CLI + 319 execution + 2 span + 11 LSP + 3 snapshot + 44 compiler + 2 freeze +
  58 syntax).
- `python scripts/verify_generated_c.py` passes, so the shortened `ae_` symbol
  prefix still round-trips through the reachability pruning.
- The rename was applied mechanically in one pass and verified by comparing the
  emitted tree before and after it: the only differences are the intended token
  replacements.

## Consequences

- Every published snippet, ADR body, benchmark report, and CI log now uses the new
  name, so documentation reading order is unaffected.
- Branches written before the rename must be rebased onto the current tips;
  merging one as-is would reintroduce the old spellings.
- `docs/claims.md` needs no entry: the rename changes no measured quantity. Only
  generated-C byte sizes shift slightly because symbols are shorter, and no claim
  quotes an absolute generated-C size as a fixed value (see ADR-50's note that the
  size varies with the built-ins a program uses).
- Revisit criteria: if `ae` proves too ambiguous for search and package registries,
  a display name can be reintroduced (`AE` in prose, `ae` as the command) without
  touching any identifier.
