# ADR-0179: Dependency Requirement Resolution Primitives (Version Order, Requirement Forms, MVS Floor, Lock Entries)

## Status
Accepted

## Context
`docs/plan.md` §4.3 requires Minimal Version Selection and a lock file, and M3.4 makes dependency resolution a milestone step. What exists today is `std/core/version.ae`: a three-field version and one `semver_is_compatible` predicate that compares major and then treats "current >= target" as compatible. That predicate cannot express what a resolver needs:

- It has no order. Nothing in the standard library answers "which of these two versions is newer", and `1.0.0-rc.1` versus `1.0.0` has no defined answer at all.
- It has no requirement forms. `^1.2.3`, `~1.2.3`, and `=1.2.3` differ, and the caret form changes meaning below 1.0.
- It cannot record a resolution or its provenance, so a lock file would have nothing to compare against.

## Decision
1. Implemented `std/core/ir_semver_order.ae`:
   - `SemVerOrder` adds a prerelease ordinal to the three numeric fields; 0 means "release", which is the greatest value, so `1.0.0-rc.1 < 1.0.0` falls out of the comparison instead of being special-cased at every call site.
   - `semver_cmp_fields` compares eight integers and is the primitive the rest builds on; `semver_cmp`, `semver_gt`, `semver_lt`, `semver_same`, `semver_max` wrap it. The field-level form exists because this compiler moves struct arguments, so a helper that took two `SemVerOrder` values could not be used twice in one function.
   - `semver_compatible_major` treats 1.x as one compatibility line and 0.x as exact-prefix only.
2. Implemented `std/core/ir_version_req.ae`:
   - Three requirement kinds (exact, caret, tilde) with the SemVer caret special case for pre-1.0 versions: `^1.2.3` stops at 2.0.0, `^0.2.3` stops at 0.3.0, `^0.0.3` is exact.
   - `req_matches` dispatches and returns false for an unknown kind — failing closed is the only safe default when a manifest carries a form this compiler cannot read.
   - `req_allows_prerelease` keeps a prerelease from satisfying a release requirement, so `1.0.0-rc.1` does not quietly satisfy `^1.0.0`.
   - `req_caret_upper_bound_ok` exposes the upper-bound half of the caret rule separately, which is what a resolver needs once it knows the candidate clears the floor.
3. Implemented `std/core/ir_mvs.ae`:
   - `MvsFloor` is one package's accumulated minimum: it starts at 0.0.0 and `mvs_require` only ever raises it, so the order requirements arrive in cannot change the result. This is Minimal Version Selection without a solver and without a search.
   - `mvs_resolve` turns the floor into an `MvsPick`; `mvs_would_raise` answers whether a new requirement would move it; `mvs_pick_is_exact` and `mvs_same_pick` compare results.
4. Implemented `std/core/ir_lockfile.ae`:
   - `LockEntry` records the resolved version, the source kind (registry, git, path), and whether the content checksum verified.
   - `lock_entry_needs_update` compares an entry against a pick; `lock_entry_is_trusted` requires a known source and, for anything but a path dependency, a verified checksum; `lockfile_needs_rewrite` combines both so an untrusted entry is rewritten even when its version still matches.
5. Added `tests/run/batch_core_semver.ae` covering: numeric ordering and prerelease ordering, 1.x versus 0.x compatibility, every caret special case at 1.x/0.x/0.0.x, tilde and exact, unknown-kind failure, prerelease admission, MVS floors under out-of-order requirements (including a same-multiset permutation), and lock trust rules for registry, git, and path sources.

## Consequences
- These are predicates and small value types. Nothing here parses a manifest, reads a registry, walks a transitive dependency graph, or writes `ae.lock`. M3.4 stays unimplemented, and no claim about package management follows.
- Prerelease identifiers are an ordinal, not SemVer's identifier list. `1.0.0-alpha.1` versus `1.0.0-alpha.2` works, but `1.0.0-alpha` versus `1.0.0-1` does not follow SemVer's numeric-before-alphanumeric rule: a caller must map identifiers to ordinals itself.
- Only exact, caret, and tilde requirements exist. Ranges (`>=1.2, <2`), wildcards, unions, and multiple constraints per dependency are absent.
- `std/core/version.ae` keeps its own `semver_is_compatible`. The two do not contradict each other — it answers a narrower question — but a caller should use the ordering module, and `version.ae` should be folded into it when it next changes.
- All comparison helpers are pure and allocation-free, so the resolver can run inside the compiler without pulling in `alloc`.
- Revisit criteria: when the manifest parser lands, the requirement kinds gain a parser and the ordinal model is replaced by identifier-list comparison with its own tests.
