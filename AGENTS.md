# Working rules for every agent in this repository

These rules exist because several agents share one machine and one
repository. Ignoring them produces interleaved edits, uncommitted work and
commits attributed to the wrong account.

## Before you touch anything

1. `git status -sb`, `git rev-parse --short HEAD`, `git worktree list`.
2. Read `AGENT_PROGRESS.md`, then the newest entries of
   `docs/implementation-ledger.md`, then `docs/checklist.md`.
3. Read the highest-numbered ADRs in `docs/adr/`.
4. Check `docs/coordination/HANDOVER.md` for the state the last agent left.

## Isolation (mandatory)

- Never edit the primary checkout while another agent has uncommitted
  changes there.
- One agent = one worktree + one branch:

  ```
  pwsh scripts/agent-worktree.ps1 -Action new -Name <agent> -Base main
  ```

  This creates branch `agent/<agent>` and a worktree under
  `<area>/worktrees/<agent>`.
- The primary checkout stays on `main` and clean; only tested merges land
  there.
- Remove your worktree when the branch is merged:
  `pwsh scripts/agent-worktree.ps1 -Action remove -Name <agent>`.

## Claim the area you are editing

- Before editing shared code:

  ```
  pwsh scripts/agent-worktree.ps1 -Action claim -Name <agent> -Area <area>
  ```

- Areas: `parser`, `borrow`, `resolver`, `typecheck`, `mir`, `codegen`,
  `runtime`, `stdlib`, `docs`, `scripts`, `packaging`.
- A second claim on the same area within 12 hours is reported as a
  conflict; coordinate instead of editing anyway.
- Release when your slice is committed:
  `pwsh scripts/agent-worktree.ps1 -Action release -Name <agent> -Area <area>`.

## Commits

- One coherent slice per commit, message `type: summary` (`feat:` `fix:`
  `docs:` `refactor:` `test:` `chore:`).
- Stage explicit paths. Never `git add -A` a worktree that may contain
  another agent's files.
- Run the narrowest test that covers the change, then
  `cargo test --workspace --locked` for cross-cutting work, then
  `python scripts/verify_generated_c.py` for codegen/runtime changes.
- Wait for the `Test` workflow to go green before merging to `main`.
- Uncommitted work is snapshotted every 10 minutes into local
  `refs/wip/<timestamp>`; `git wip` forces a snapshot, `git wip-list`
  lists them. Snapshots are local-only and never pushed.

## Handover

- Copy `docs/coordination/handover-template.md` to
  `docs/coordination/HANDOVER-<agent>.md`, fill it in, commit it with your
  branch, and push the branch.
- Your final message must state: branch, worktree path, commits, what
  works, what is half-done, the next command to run, and blockers.
- Never leave `main` dirty. If you must stop mid-change, snapshot it
  (`git wip`) and say so in the handover.
