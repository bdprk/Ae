# Multi-agent workbench: claude-squad in WSL2

`claude-squad` runs several coding agents (Codex, Claude Code, Aider, any
custom command) side by side, each in its own git worktree and tmux
session. It complements `AGENTS.md`: the TUI is the cockpit, the rules in
`AGENTS.md` are the contract.

## What is installed

| Item | Location |
|---|---|
| WSL distro | `Ubuntu` (WSL2) |
| claude-squad `cs` | `/root/.local/bin/cs` (v1.0.20) |
| Codex CLI | `/usr/bin/codex` (0.171.0, same account as Windows) |
| tmux | `/usr/bin/tmux` |
| config | `/root/.claude-squad/config.json` |
| launcher | desktop shortcut `AE Agents` -> `wt wsl -d Ubuntu -e /root/ae-agents.sh` |

Config:

```json
{ "default_program": "codex", "auto_yes": true,
  "daemon_poll_interval": 1000, "branch_prefix": "agent/" }
```

`auto_yes` answers the agent's confirmation prompts automatically; the
`AGENTS.md` rules still decide what the agent is allowed to commit and
push.

## Launch

- Double-click the `AE Agents` shortcut on the desktop, or
- `wt wsl -d Ubuntu -e /root/ae-agents.sh`

## Keys

- `n` new instance (Codex), `N` new instance with a prompt
- `Tab` / arrows switch panes: Preview, Diff, Terminal
- `?` help, `q` quit (sessions keep running in tmux and are restored on
  the next launch)
- `/` filter, `r` reset, `d` delete instance

## Rules of the road

1. One instance = one task = one worktree; never open the primary
   checkout in a session.
2. Give every instance a prompt that names its area (`parser`,
   `resolver`, `typecheck`, ...) so two instances never edit the same
   module.
3. The agent commits, pushes its `agent/<name>` branch and opens a PR
   using the protocol in `AGENTS.md`.
4. Merge only after the `Test` workflow is green.

## Credentials inside WSL

- SSH: `~/.ssh/id_ed25519` (bdprk key) plus a `github-bdprk` host entry
  on port 443; `ssh -T git@github-bdprk` answers `Hi bdprk!`.
- Repo-local identity in `/mnt/d/.../ae`: `jay young
  <parrishmary29@gmail.com>`.
- Codex auth: `~/.codex/auth.json`, copied from the Windows install.
