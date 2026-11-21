# Multi-agent coordination

One repository, several agents, one shared `main`. The rules below keep
parallel work from turning into interleaved edits.

## Layout

```
<repo>/
├─ ae/                     primary checkout, stays on main and clean
├─ worktrees/<agent>/      one git worktree per agent, branch agent/<agent>
└─ _cache/ae/target/       shared build directory (configured in .cargo/)
```

Each worktree has its own working files but shares `.git`, so commits,
branches and claims are visible everywhere immediately.

## Lifecycle

1. `scripts/agent-worktree.ps1 -Action new -Name parser-1 -Base main`
2. `scripts/agent-worktree.ps1 -Action claim -Name parser-1 -Area parser`
3. Edit, test, commit small slices on `agent/parser-1`.
4. `git push origin agent/parser-1`, open a PR, wait for CI.
5. Merge with a merge commit (keeps the agent's slice history).
6. `scripts/agent-worktree.ps1 -Action release ...` and `-Action remove`.

## Claims

Claims are JSON files under `<git-common-dir>/claims/`, not in the working
tree, so they never get committed and are visible from every worktree:

```
{ "area": "parser", "agent": "parser-1", "branch": "agent/parser-1",
  "since": "2026-11-21T08:30:00+08:00", "pid": 12345 }
```

A claim older than 12 hours is stale and may be taken over, but the new
owner must say so in the handover.

## Merge rules

- `main` must be green: the `Test` workflow covers Linux and Windows.
- Merge only after `cargo test --workspace --locked` passes in your
  worktree.
- Conflicts in generated files (`docs/checklist.md`,
  `scripts/gen_checklist.py` output) are resolved by regenerating, never
  by hand.
- Do not rewrite history on `main`; force-push only your own
  `agent/<name>` branch.

## Failure handling

Every agent appends failures to `agent-failure-log/YYYY-MM-DD.md` (see the
global agent rules). The log is ignored by git but read by handover.
