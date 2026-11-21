# Handover — state as of 2026-11-21 08:45 (+08:00)

This file is the entry point for the next agent. It describes what is on
disk right now, not what is merged.

## Repository

- `main` is the integration branch; run `git log --oneline -1 main` for the
  current tip. Do not trust a commit id written here.
- Remote: the repository's `origin`
- Branches: `main`, `feat/m1-type-system-expansion`, tag `v0.1.0`

## Current handoff

`docs/HANDOFF-20261121.md` is the live checklist; read it first. Everything
below is the historical record of the 08:45 stop point, kept only so the
shape of that work is still legible.

The 27 dirty entries described here were all committed to `main` afterwards.
Do not restore an older work-in-progress snapshot over the current tree.

- New crate `crates/ae-resolve/` — resolver pass (ADR-0227), modules and
  imports, plus `src/dump.rs`.
- `docs/adr/0228-type-spelling-resolution.md` — implemented.
- `docs/adr/0229-type-checker-slice-ladder.md` — proposed, not started.
- `docs/checklist.md`, `scripts/gen_checklist.py`,
  `scripts/test_gen_checklist.py` — R5.1 checklist generator.
- `scripts/std_layering.py`, `scripts/test_std_layering.py` — std-layering
  checker.
- `scripts/check_commit_ids.py` — commit-id ledger check.
- Modified: `Cargo.toml`, `Cargo.lock`, `src/main.rs`,
  `crates/ae-syntax/src/{declaration,expression}.rs`,
  `docs/implementation-ledger.md`, `AGENT_PROGRESS.md`, `CHANGELOG.md`,
  `.github/workflows/test.yml`, `scripts/diff_fuzz.py`,
  `scripts/verify_generated_c.py`, `tests/execution.rs`,
  `tests/snapshots.rs`, plus new `tests/resolve/` fixtures.

The automatic checkpoint task still snapshots dirty worktrees into local
`refs/wip/<timestamp>` refs; `git wip-list` shows the current ones.

## Next action

```sh
git status -sb
pwsh scripts/agent-worktree.ps1 -Action new -Name <you> -Base main
```

Work in the new worktree, copy the uncommitted files you need from the
primary checkout (or restore the snapshot there), finish one slice at a
time, and commit by area:

```sh
cargo test --workspace --locked
python scripts/verify_generated_c.py
```

## Known risks

- Several agents share this checkout: do not `git add -A`, stage explicit
  paths only.
- `crates/ae-resolve` is not yet a workspace member in the committed
  `Cargo.toml`; adding it changes build order and CI time.
- ADR-0229 (type-checker slice ladder) is design only; no code yet.
- The interrupted agent's `tests/execution.rs` edits may encode
  expectations for code that is not committed yet; run the suite before
  keeping them.

## Rules

See `AGENTS.md` and `docs/coordination/README.md`. Claims live in
`<git-common-dir>/claims/`; `scripts/agent-worktree.ps1 -Action status`
shows them.
