"""Layering enforcement for the std tree (ADR-0224 R4.1, enforcement first).

The split itself is a later slice; the enforcement exists now so the split
cannot regress it. Rules, in force today and verified against the tree:

* every `use "path";` header in a std file resolves to an existing file
  relative to the importing file;
* `std/core/**` — the no-alloc, no-OS layer — imports only `std/core/**`.
  Anything else in `std/` is a hosted module above it, and a core file
  reaching up (io, sync, task, test, …) is a violation.

When R4.1's split introduces `std/alloc` and `std/compiler`, this module
gains their ranks; the direction rule is already written as a comparison.
"""

import re
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
STD = ROOT / "std"

USE = re.compile(r'^\s*use\s+"([^"]+)"\s*;')

# std/<dir> -> layer rank. core is the bottom; every other directory is a
# hosted layer above it until R4.1 names them (alloc, compiler).
LAYERS = {"core": 0}
DEFAULT_RANK = 1


def layer_of(relative: Path) -> int:
    """Rank of a std file from its path below std/ (its first directory)."""
    parts = relative.parts
    if len(parts) >= 2:
        return LAYERS.get(parts[0], DEFAULT_RANK)
    return DEFAULT_RANK


def header_uses(source: str):
    for line in source.split("\n"):
        match = USE.match(line)
        if match:
            yield match.group(1)


def check(std_root: Path = STD):
    """Return every layering violation as a human-readable line."""
    violations = []
    files = sorted(std_root.rglob("*.ae"))
    for file in files:
        relative = file.relative_to(std_root)
        source = file.read_text(encoding="utf-8")
        for use_path in header_uses(source):
            target = (file.parent / use_path).resolve()
            if not target.exists():
                violations.append(f"{relative}: use \"{use_path}\" does not resolve")
                continue
            imported_relative = target.relative_to(std_root)
            if layer_of(relative) == LAYERS.get("core", 0) and layer_of(imported_relative) > LAYERS.get("core", 0):
                violations.append(
                    f"{relative}: core imports hosted module {imported_relative.as_posix()}"
                )
    return violations


def main() -> int:
    violations = check()
    if violations:
        print(f"{len(violations)} std layering violations:", file=__import__("sys").stderr)
        for violation in violations:
            print(f"  {violation}", file=__import__("sys").stderr)
        return 1
    print("std layering holds")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
