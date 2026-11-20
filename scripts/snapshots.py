#!/usr/bin/env python3
"""Run or explicitly update ae's diagnostic and execution snapshots."""
import argparse
import os
from pathlib import Path
import subprocess
import sys

ROOT = Path(__file__).resolve().parent.parent
SUITES = {"ui": "ui_snapshots", "json": "json_ui_snapshots", "run": "run_snapshots"}


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--suite", choices=["all", *SUITES], default="all")
    parser.add_argument("--bless", action="store_true", help="rewrite expected outputs; review the resulting diff")
    args = parser.parse_args(argv)
    env = os.environ.copy()
    # A check must remain read-only even if the caller inherited this variable.
    env.pop("AE_BLESS", None)
    if args.bless:
        env["AE_BLESS"] = "1"
    command = ["cargo", "test", "--locked", "--test", "snapshots"]
    if args.suite != "all":
        command.extend([SUITES[args.suite], "--", "--exact"])
    return subprocess.run(command, cwd=ROOT, env=env, check=False).returncode


if __name__ == "__main__":
    sys.exit(main())
