#!/usr/bin/env python3
"""ae-orchestrator: a small, dependency-free task queue for coding agents.

One task = one git worktree = one branch = one pull request. The queue is a
JSON file, the state machine is explicit, and every step is logged. Nothing
here touches the primary checkout, and a merge only happens after the
configured gates pass and the CI checks are green.

Commands:
    init                 create the state directory and default config
    add TITLE --prompt P add a task (optionally --area, --files)
    run [--once]         work the queue (default: loop forever)
    status               show the queue
    logs TASK            print a task's log
    stop                 ask a running worker to finish the current task
    reset TASK           move a failed/stuck task back to pending

Design notes:
  * state.json holds tasks and their status; it is written atomically.
  * A task owns an "area" (parser, resolver, ...). Two running tasks may not
    share an area, which is the cheap version of a file lock.
  * --dry-run replaces the agent with a stub that appends one line, so the
    whole pipeline can be exercised without spending model tokens.
"""

from __future__ import annotations

import argparse
import json
import os
import shutil
import signal
import subprocess
import sys
import time
import uuid
from dataclasses import dataclass, field
from datetime import datetime, timezone
from pathlib import Path

STATE_DIR = ".orchestrator"
CONFIG_FILE = "config.json"
STATE_FILE = "state.json"
STOP_FILE = "STOP"

DEFAULT_CONFIG = {
    "max_parallel": 2,
    "agent_command": [
        "codex", "exec", "--sandbox", "danger-full-access",
        "--skip-git-repo-check", "--color", "never", "-",
    ],
    "gates": [
        ["cargo", "test", "--workspace", "--locked"],
        ["python3", "scripts/verify_generated_c.py"],
    ],
    "commit_name": "jay young",
    "commit_email": "parrishmary29@gmail.com",
    "base_branch": "main",
    "branch_prefix": "agent/",
    "auto_merge": True,
    "merge_method": "squash",
    "ci_timeout_seconds": 3600,
    "ci_poll_seconds": 30,
    "max_attempts": 2,
    "worktree_root": "/root/worktrees",
}


def now() -> str:
    return datetime.now(timezone.utc).astimezone().isoformat(timespec="seconds")


def run(cmd, cwd=None, check=False, timeout=None, env=None):
    proc = subprocess.run(
        cmd, cwd=cwd, capture_output=True, text=True, timeout=timeout,
        env=env, errors="replace",
    )
    if check and proc.returncode != 0:
        raise RuntimeError(
            f"command failed ({proc.returncode}): {' '.join(cmd)}\n"
            f"{proc.stdout[-2000:]}\n{proc.stderr[-2000:]}"
        )
    return proc


class Store:
    """Atomic JSON state, safe against a crash mid-write."""

    def __init__(self, root: Path):
        self.dir = root / STATE_DIR
        self.dir.mkdir(parents=True, exist_ok=True)
        (self.dir / "logs").mkdir(exist_ok=True)
        self.config_path = self.dir / CONFIG_FILE
        self.state_path = self.dir / STATE_FILE
        if not self.config_path.exists():
            self.config_path.write_text(
                json.dumps(DEFAULT_CONFIG, indent=2) + "\n", encoding="utf-8")
        if not self.state_path.exists():
            self._write({"tasks": []})

    @property
    def config(self) -> dict:
        cfg = json.loads(self.config_path.read_text(encoding="utf-8"))
        merged = dict(DEFAULT_CONFIG)
        merged.update(cfg)
        return merged

    def read(self) -> dict:
        return json.loads(self.state_path.read_text(encoding="utf-8"))

    def _write(self, state: dict) -> None:
        tmp = self.state_path.with_suffix(".tmp")
        tmp.write_text(json.dumps(state, indent=2) + "\n", encoding="utf-8")
        os.replace(tmp, self.state_path)

    def update(self, fn) -> dict:
        state = self.read()
        fn(state)
        self._write(state)
        return state

    def log_path(self, task_id: str) -> Path:
        return self.dir / "logs" / f"{task_id}.log"

    def log(self, task_id: str, message: str) -> None:
        line = f"[{now()}] {message}\n"
        with self.log_path(task_id).open("a", encoding="utf-8") as fh:
            fh.write(line)
        print(line, end="", flush=True)


def find_task(state: dict, task_id: str) -> dict | None:
    for task in state["tasks"]:
        if task["id"] == task_id or task["id"].startswith(task_id):
            return task
    return None


def slug(text: str, limit: int = 28) -> str:
    keep = [c if (c.isalnum() or c in "-") else "-" for c in text.lower()]
    out = "".join(keep)
    while "--" in out:
        out = out.replace("--", "-")
    return out.strip("-")[:limit] or "task"


def github_token() -> str | None:
    for name in ("GITHUB_TOKEN", "GH_TOKEN"):
        value = os.environ.get(name)
        if value:
            return value.strip()
    for path in (Path.home() / ".gh-token", Path("/root/.gh-token")):
        if path.exists():
            return path.read_text(encoding="utf-8").strip()
    return None


def api(method: str, url: str, payload: dict | None = None) -> dict:
    token = github_token()
    if not token:
        raise RuntimeError("no GitHub token (set GITHUB_TOKEN or write ~/.gh-token)")
    cmd = [
        "curl", "-sS", "-X", method, "-H", f"Authorization: Bearer {token}",
        "-H", "Accept: application/vnd.github+json",
        "-H", "X-GitHub-Api-Version: 2022-11-28", url,
    ]
    if payload is not None:
        cmd += ["-d", json.dumps(payload)]
    proc = run(cmd)
    try:
        return json.loads(proc.stdout or "{}")
    except json.JSONDecodeError:
        return {"message": proc.stdout.strip() or proc.stderr.strip()}


def repo_slug(root: Path) -> str:
    url = run(["git", "remote", "get-url", "origin"], cwd=root, check=True).stdout.strip()
    if url.startswith("git@"):
        path = url.split(":", 1)[1]
    else:
        path = url.split("github.com/", 1)[-1]
    return path.removesuffix(".git")


@dataclass
class Worker:
    root: Path
    store: Store
    dry_run: bool = False
    stop_requested: bool = False
    children: list = field(default_factory=list)

    @property
    def cfg(self) -> dict:
        return self.store.config

    def gate_commands(self) -> list[list[str]]:
        return [list(cmd) for cmd in self.cfg["gates"]]

    def agent_command(self) -> list[str]:
        return list(self.cfg["agent_command"])

    def handle_signal(self, *_args) -> None:
        self.stop_requested = True
        print("\nstop requested; finishing the current task", flush=True)

    def loop(self, once: bool = False) -> int:
        while not self.stop_requested:
            if (self.store.dir / STOP_FILE).exists():
                print("STOP file present; exiting", flush=True)
                return 0
            task = self.claim()
            if task is None:
                if once:
                    return 0
                time.sleep(5)
                continue
            try:
                self.execute(task)
            except Exception as exc:  # a bad task must not kill the loop
                self.fail(task, f"unhandled error: {exc}")
            if once:
                return 0
        return 0

    def claim(self) -> dict | None:
        chosen: dict | None = None

        def pick(state: dict) -> None:
            nonlocal chosen
            busy_areas = {
                t["area"] for t in state["tasks"]
                if t["status"] in {"running", "verifying", "merging"}
            }
            running = sum(
                1 for t in state["tasks"]
                if t["status"] in {"running", "verifying", "merging"}
            )
            if running >= self.cfg["max_parallel"]:
                return
            for task in sorted(state["tasks"], key=lambda t: t["created"]):
                if task["status"] != "pending":
                    continue
                if task["area"] in busy_areas:
                    continue
                task["status"] = "running"
                task["started"] = now()
                task["attempts"] = task.get("attempts", 0) + 1
                chosen = task
                return

        self.store.update(pick)
        return chosen

    def execute(self, task: dict) -> None:
        task_id = task["id"]
        self.store.log(task_id, f"start: {task['title']} (attempt {task['attempts']})")

        worktree = Path(self.cfg["worktree_root"]) / task_id
        branch = f"{self.cfg['branch_prefix']}{task_id}"
        base = self.cfg["base_branch"]

        run(["git", "fetch", "origin", base], cwd=self.root)
        worktree.parent.mkdir(parents=True, exist_ok=True)
        add = run(
            ["git", "worktree", "add", "-b", branch, str(worktree), f"origin/{base}"],
            cwd=self.root,
        )
        if add.returncode != 0 and "already exists" not in add.stderr:
            raise RuntimeError(f"worktree add failed: {add.stderr.strip()}")

        self.store.log(task_id, f"worktree {worktree} on {branch}")
        self.run_agent(task, worktree)

        changed = run(["git", "status", "--porcelain"], cwd=worktree,
                      check=True).stdout.strip()
        if not changed:
            self.finish(task, "no-change", "agent produced no edits")
            return

        self.store.log(task_id, f"agent changed {len(changed.splitlines())} path(s)")
        self.set_status(task_id, "verifying")
        for gate in self.gate_commands():
            self.store.log(task_id, f"gate: {' '.join(gate)}")
            proc = run(gate, cwd=worktree, timeout=3600)
            if proc.returncode != 0:
                tail = (proc.stdout + proc.stderr)[-3000:]
                raise RuntimeError(f"gate failed: {' '.join(gate)}\n{tail}")
        self.store.log(task_id, "gates passed")

        name = self.cfg["commit_name"]
        email = self.cfg["commit_email"]
        run(["git", "add", "-A"], cwd=worktree, check=True)
        run(
            ["git", "-c", f"user.name={name}", "-c", f"user.email={email}",
             "commit", "-q", "-m", f"agent: {task['title']}"],
            cwd=worktree, check=True,
        )
        sha = run(["git", "rev-parse", "HEAD"], cwd=worktree, check=True).stdout.strip()
        self.store.log(task_id, f"commit {sha[:8]}")

        if self.dry_run:
            self.finish(task, "dry-run", f"commit {sha[:8]} created locally; push skipped")
            return

        push = run(["git", "push", "-q", "-u", "origin", branch], cwd=worktree)
        if push.returncode != 0:
            raise RuntimeError(f"push failed: {push.stderr.strip()}")

        number = self.open_pr(task, branch, worktree, sha)
        if number is None:
            self.finish(task, "pushed", f"branch {branch} pushed; PR not opened")
            return

        self.store.update(lambda s: find_task(s, task_id).update({"pr": number}))
        if not self.cfg["auto_merge"]:
            self.finish(task, "pushed", f"PR #{number} awaiting review")
            return

        self.set_status(task_id, "merging")
        if self.wait_for_ci(task, sha):
            self.merge_pr(task, number)
            self.finish(task, "done", f"PR #{number} merged")
            self.cleanup(task, worktree, branch)
        else:
            self.finish(task, "ci-failed", f"PR #{number} has failing checks")

    def run_agent(self, task: dict, worktree: Path) -> None:
        prompt_path = worktree / ".ae-task.txt"
        prompt = task["prompt"] + (
            "\n\nRepository rules: read AGENTS.md first. Work only inside this "
            "worktree. Do not push, do not open a pull request, do not merge; "
            "the orchestrator does that. Run the narrowest test that covers "
            "your change and leave the tree compiling.\n"
        )
        prompt_path.write_text(prompt, encoding="utf-8")
        log = self.store.log_path(task["id"]).open("a", encoding="utf-8")
        try:
            if self.dry_run:
                (worktree / "DRY-RUN.md").write_text(
                    f"dry run for {task['id']}\n", encoding="utf-8")
                self.store.log(task["id"], "dry-run agent wrote DRY-RUN.md")
                return
            env = dict(os.environ)
            env.setdefault("CARGO_TARGET_DIR", "/root/.cache/ae-target")
            proc = subprocess.run(
                self.agent_command(), cwd=worktree, stdin=prompt_path.open(),
                stdout=log, stderr=subprocess.STDOUT, env=env, text=True,
            )
            if proc.returncode != 0:
                raise RuntimeError(f"agent exited {proc.returncode}; see {log.name}")
        finally:
            log.close()

    def open_pr(self, task: dict, branch: str, worktree: Path, sha: str) -> int | None:
        slug_name = repo_slug(self.root)
        existing = api("GET", f"https://api.github.com/repos/{slug_name}/pulls?head={slug_name.split('/')[0]}:{branch}")
        if isinstance(existing, list) and existing:
            return existing[0]["number"]
        body = (
            f"Automated agent task.\n\n"
            f"**Task**: {task['title']}\n\n{task['prompt']}\n\n"
            f"**Commit**: `{sha}`\n**Branch**: `{branch}`\n"
            f"**Worktree**: `{worktree}`\n**Log**: `.orchestrator/logs/{task['id']}.log`\n"
        )
        created = api(
            "POST", f"https://api.github.com/repos/{slug_name}/pulls",
            {"title": f"agent: {task['title']}", "head": branch,
             "base": self.cfg["base_branch"], "body": body},
        )
        if "number" in created:
            self.store.log(task["id"], f"PR #{created['number']} {created.get('html_url', '')}")
            return created["number"]
        self.store.log(task["id"], f"PR creation failed: {created.get('message')}")
        return None

    def wait_for_ci(self, task: dict, sha: str) -> bool:
        slug_name = repo_slug(self.root)
        deadline = time.time() + self.cfg["ci_timeout_seconds"]
        while time.time() < deadline and not self.stop_requested:
            checks = api("GET", f"https://api.github.com/repos/{slug_name}/commits/{sha}/check-runs")
            runs = checks.get("check_runs", []) if isinstance(checks, dict) else []
            if runs and all(r["status"] == "completed" for r in runs):
                bad = [r["name"] for r in runs if r.get("conclusion") != "success"]
                self.store.log(task["id"], f"CI {'green' if not bad else 'red: ' + ', '.join(bad)}")
                return not bad
            time.sleep(self.cfg["ci_poll_seconds"])
        self.store.log(task["id"], "CI wait timed out")
        return False

    def merge_pr(self, task: dict, number: int) -> None:
        slug_name = repo_slug(self.root)
        result = api(
            "PUT", f"https://api.github.com/repos/{slug_name}/pulls/{number}/merge",
            {"merge_method": self.cfg["merge_method"]},
        )
        if not result.get("merged"):
            raise RuntimeError(f"merge failed: {result.get('message')}")

    def cleanup(self, task: dict, worktree: Path, branch: str) -> None:
        run(["git", "worktree", "remove", "--force", str(worktree)], cwd=self.root)
        run(["git", "branch", "-D", branch], cwd=self.root)
        run(["git", "push", "-q", "origin", "--delete", branch], cwd=self.root)

    def set_status(self, task_id: str, status: str) -> None:
        self.store.update(lambda s: find_task(s, task_id).update({"status": status}))

    def finish(self, task: dict, status: str, note: str) -> None:
        def apply(state: dict) -> None:
            record = find_task(state, task["id"])
            record["status"] = status
            record["note"] = note
            record["finished"] = now()
        self.store.update(apply)
        self.store.log(task["id"], f"{status}: {note}")

    def fail(self, task: dict, reason: str) -> None:
        attempts = task.get("attempts", 1)
        status = "pending" if attempts < self.cfg["max_attempts"] else "failed"

        def apply(state: dict) -> None:
            record = find_task(state, task["id"])
            record["status"] = status
            record["note"] = reason[-500:]
        self.store.update(apply)
        self.store.log(task["id"], f"{status}: {reason[-500:]}")
        if status == "failed":
            run(["git", "worktree", "remove", "--force",
                 str(Path(self.cfg["worktree_root"]) / task["id"])], cwd=self.root)


def cmd_init(args) -> int:
    store = Store(args.root)
    print(f"initialised {store.dir}")
    print(json.dumps(store.config, indent=2))
    return 0


def cmd_add(args) -> int:
    store = Store(args.root)
    task_id = f"{datetime.now().strftime('%Y%m%d-%H%M%S')}-{slug(args.title)}-{uuid.uuid4().hex[:4]}"
    task = {
        "id": task_id,
        "title": args.title,
        "prompt": args.prompt or args.title,
        "area": args.area or "general",
        "status": "pending",
        "created": now(),
        "attempts": 0,
    }
    store.update(lambda state: state["tasks"].append(task))
    print(task_id)
    return 0


def cmd_run(args) -> int:
    store = Store(args.root)
    worker = Worker(args.root, store, dry_run=args.dry_run)
    signal.signal(signal.SIGINT, worker.handle_signal)
    signal.signal(signal.SIGTERM, worker.handle_signal)
    return worker.loop(once=args.once)


def cmd_status(args) -> int:
    store = Store(args.root)
    state = store.read()
    if not state["tasks"]:
        print("queue is empty")
        return 0
    print(f"{'id':<44} {'status':<10} {'area':<10} title")
    for task in state["tasks"]:
        print(f"{task['id']:<44} {task['status']:<10} {task['area']:<10} {task['title']}")
        if task.get("note"):
            print(f"{'':<44} {task['note'].splitlines()[0][:90]}")
    return 0


def cmd_logs(args) -> int:
    store = Store(args.root)
    state = store.read()
    task = find_task(state, args.task)
    if not task:
        print(f"no task matching {args.task}")
        return 1
    path = store.log_path(task["id"])
    if not path.exists():
        print("no log yet")
        return 0
    print(path.read_text(encoding="utf-8"))
    return 0


def cmd_stop(args) -> int:
    store = Store(args.root)
    (store.dir / STOP_FILE).write_text(now() + "\n", encoding="utf-8")
    print("stop requested; the worker exits after the current task")
    return 0


def cmd_reset(args) -> int:
    store = Store(args.root)
    task = find_task(store.read(), args.task)
    if not task:
        print(f"no task matching {args.task}")
        return 1

    def apply(state: dict) -> None:
        record = find_task(state, task["id"])
        record["status"] = "pending"
        record["attempts"] = 0
        record["note"] = "reset by hand"
    store.update(apply)
    stop = store.dir / STOP_FILE
    if stop.exists():
        stop.unlink()
    print(f"reset {task['id']}")
    return 0


def main(argv: list[str]) -> int:
    parser = argparse.ArgumentParser(prog="ae-orchestrator")
    parser.add_argument("--root", type=Path,
                        default=Path(run(["git", "rev-parse", "--show-toplevel"],
                                         check=True).stdout.strip()))
    sub = parser.add_subparsers(dest="command", required=True)

    sub.add_parser("init").set_defaults(func=cmd_init)

    add = sub.add_parser("add")
    add.add_argument("title")
    add.add_argument("--prompt")
    add.add_argument("--area")
    add.set_defaults(func=cmd_add)

    run_cmd = sub.add_parser("run")
    run_cmd.add_argument("--once", action="store_true")
    run_cmd.add_argument("--dry-run", action="store_true")
    run_cmd.set_defaults(func=cmd_run)

    sub.add_parser("status").set_defaults(func=cmd_status)

    logs = sub.add_parser("logs")
    logs.add_argument("task")
    logs.set_defaults(func=cmd_logs)

    sub.add_parser("stop").set_defaults(func=cmd_stop)

    reset = sub.add_parser("reset")
    reset.add_argument("task")
    reset.set_defaults(func=cmd_reset)

    args = parser.parse_args(argv)
    args.root = args.root.resolve()
    return args.func(args)


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
