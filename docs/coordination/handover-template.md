# Handover — <agent name>

- Date: <YYYY-MM-DD HH:MM +08:00>
- Branch: `agent/<name>` (commit `<sha>`)
- Worktree: `<absolute path>`
- Area claim: `<area>` (released? yes/no)
- Model/tool: <name>

## What is done

- <slice> — commit `<sha>`, tests `<command>` green

## What is half-done

- <file / function> — intent, current shape, what still fails
- Snapshot ref (if uncommitted): `refs/wip/<timestamp>`

## Next step

```sh
<exact command the next agent runs first>
```

## Evidence

- `cargo test --workspace --locked` — <pass/fail counts>
- `python scripts/verify_generated_c.py` — <pass/fail>
- CI run: <url or "not pushed yet">

## Risks and blockers

- <red test, missing spec, ambiguous ADR, external dependency>

## Files touched

- <path> — <why>
