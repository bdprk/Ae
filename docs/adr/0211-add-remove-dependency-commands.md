# ADR-0211: `ae add` and `ae remove` Manifest Editing

- Date: 2026-11-19
- Status: Accepted; implemented in `src/package.rs` with the CLI test `add_and_remove_manage_manifest_dependencies_and_the_lockfile`.
- Scope: the single-CLI surface commands `add <path>` and `remove <name>` (charter §3.2). They do not change resolution semantics; they only edit `ae.toml` and refresh `ae.lock` through the existing MVS/lock paths (ADR for the package graph).

## Evidence and problem

Declaring a path dependency today requires hand-editing TOML and knowing the lockfile contract (`ae update` after every manifest change, per the M6.1 evidence). The charter lists `add` and `remove` among the commands of the one `ae` executable, so a fresh user cannot create, test, document and ship a package using only the toolchain without touching manifest text by hand. The edit must also stay platform-honest: `ae.toml` should read identically on Windows and Linux, and path comparison must survive Windows canonicalization, which returns verbatim (`\\?\`) paths whose `..` components the OS refuses to fold.

## Decision

- `ae add <path>` resolves `<path>` to a package manifest (directory or explicit `ae.toml`), names the dependency after the target package's `name`, and inserts exactly one line `alias = { path = "<relative>" }` at the top of the `[dependencies]` section, creating the section when absent. The relative path is computed between canonical directories with `/` separators so the manifest is identical on both platforms. The lockfile is then refreshed in place; a successful `add` never leaves a stale lock behind.
- Re-adding the same source is idempotent: the recorded path is resolved lexically against the canonical package directory (folding `..` without OS help, which verbatim paths require) and canonicalized; equal directories report `'<alias>' is already declared` with exit 0 and rewrite nothing. The same alias from a different directory fails with `already declared with a different source`.
- `ae remove <alias>` deletes the dependency line for that alias and is all-or-nothing on the manifest: rejections (unknown alias, or a line shape the editor does not understand) leave the file untouched. The lockfile refresh after a successful edit is best-effort: because the package's own sources may still import the removed alias, `update_lock` can legitimately fail, and the command then succeeds (exit 0) while reporting that `ae.lock` is stale and that `ae update` must run after the imports are removed. The old lock stays on disk, so the next build fails closed ("stale"/"unknown package dependency") rather than trusting an outdated graph.
- Both commands operate on the manifest discovered from the working directory (`discover_manifest`), matching `update`/`deps`. A package cannot depend on itself; every failure exits 1 through the standard failure path (usage errors exit 2).

## Alternatives rejected

- Absolutizing recorded dependency paths would make manifests host-specific and non-diffable; rejected in favor of canonical-relative `/`-separated paths.
- Refreshing the lock from the manifest graph alone (no compile) would fork lock semantics: the current lock records source units reachable from the entry, which only the compiler knows. Rejected; `remove` instead surfaces staleness honestly.
- Rolling back the manifest line when the post-`add` lock refresh fails would hide the compile error behind a silently reverted edit; the line stays and the error surfaces immediately.
- Deleting the stale lock on failed refresh would erase recorded hashes for the remaining dependencies; leaving it keeps every remaining hash verifiable until `ae update`.

## Required evidence

The CLI test covers: add from a clean manifest (section created, lock written, `run` works); idempotent re-add (no rewrite, Windows verbatim-path regression pinned); name collision from a different directory; self-add rejection; unknown-alias removal rejection (manifest untouched); remove with a live import (exit 0, stale-lock message, next run fails closed); `ae update` after removing the import (lock rebuilt without the dependency, `run` works); double remove rejected. The full workspace suite passes on Windows; the Linux run is recorded as single-platform until the next Linux probe.
