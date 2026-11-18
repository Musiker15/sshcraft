#!/usr/bin/env python3
"""Refuse a release build whose crates could come from somewhere unexpected.

Two ways a crate can be swapped without anyone noticing in a pull request:

1. A git dependency. Cargo.lock names its repository; SSHCraft takes git dependencies only from
   github.com/storytold (the Craft libraries), never from another host or account.
2. A cargo configuration that replaces a source, patches a crate or overrides a path. None of
   that shows up in Cargo.lock, so the file itself is read. Cargo still reads the old name
   `.cargo/config` beside `.cargo/config.toml`.

Usage: check_sources.py [workspace root]
"""

import sys
import tomllib
from pathlib import Path

ALLOWED_GIT = "git+https://github.com/storytold/"


def config_problems(root: Path) -> list[str]:
    problems = []
    for name in (".cargo/config", ".cargo/config.toml"):
        path = root / name
        if not path.is_file():
            continue
        with path.open("rb") as f:
            config = tomllib.load(f)
        # Parsed, not grepped: TOML can spell one key several ways.
        for source, table in config.get("source", {}).items():
            if isinstance(table, dict) and "replace-with" in table:
                problems.append(f"{name}: source.{source} is replaced")
        for key in ("patch", "paths"):
            if key in config:
                problems.append(f"{name}: has [{key}]")
    return problems


def lock_problems(root: Path) -> list[str]:
    with (root / "Cargo.lock").open("rb") as f:
        lock = tomllib.load(f)
    foreign = sorted(
        {
            pkg["source"]
            for pkg in lock.get("package", [])
            if pkg.get("source", "").startswith("git+")
            and not pkg["source"].startswith(ALLOWED_GIT)
        }
    )
    return [f"Cargo.lock: git dependency outside github.com/storytold: {s}" for s in foreign]


def main() -> int:
    root = Path(sys.argv[1] if len(sys.argv) > 1 else ".")
    if not (root / "Cargo.lock").is_file():
        print("::error::Cargo.lock is missing")
        return 1
    problems = config_problems(root) + lock_problems(root)
    for problem in problems:
        print(f"::error::{problem}")
    if not problems:
        print("Crate sources: crates.io and github.com/storytold only, no overrides")
    return 1 if problems else 0


if __name__ == "__main__":
    sys.exit(main())
