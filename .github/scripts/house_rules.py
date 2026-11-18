#!/usr/bin/env python3
"""Checks the house-rules blocks in AGENTS.md against storytold/craft-contrib.

Each block starts with
    <!-- craft-house-rules:start <kind> source=<commit> sha256=<digest> -->
and ends with <!-- craft-house-rules:end -->. A block passes when its text has the recorded
digest and equals standards/house-rules-<kind>.md of craft-contrib at the recorded commit, and
that commit is on craft-contrib's main. A newer text on main is a warning, not a failure.

Usage: house_rules.py <AGENTS.md> <craft-contrib checkout> [--rust]
"""

import hashlib
import re
import subprocess
import sys

START = re.compile(r"<!-- craft-house-rules:start ([a-z]+) source=([0-9a-f]{40}) sha256=([0-9a-f]{64}) -->\r?\n")
END = "<!-- craft-house-rules:end -->"


def normalize(text):
    text = text.removeprefix("﻿").replace("\r\n", "\n").replace("\r", "\n")
    return text.rstrip("\n") + "\n"


def sha256(text):
    return hashlib.sha256(normalize(text).encode("utf-8")).hexdigest()


def git(contrib, *args):
    return subprocess.run(["git", "-C", contrib, *args], capture_output=True, text=True, check=False)


def main():
    if len(sys.argv) < 3:
        print(__doc__)
        return 2
    agents, contrib, rust = sys.argv[1], sys.argv[2], "--rust" in sys.argv[3:]
    with open(agents, encoding="utf-8") as f:
        text = f.read()
    errors, seen = [], set()
    for match in START.finditer(text):
        kind, source, recorded = match.groups()
        end = text.find(END, match.end())
        if end < 0:
            errors.append(f"{kind}: no end marker")
            continue
        block = text[match.end():end]
        seen.add(kind)
        if sha256(block) != recorded:
            errors.append(f"{kind}: the text was changed here; change it in craft-contrib instead")
        path = f"standards/house-rules-{kind}.md"
        upstream = git(contrib, "show", f"{source}:{path}")
        if upstream.returncode != 0:
            errors.append(f"{kind}: {path} not found at {source}")
            continue
        if normalize(upstream.stdout) != normalize(block):
            errors.append(f"{kind}: the text differs from {path} at {source}")
        if git(contrib, "merge-base", "--is-ancestor", source, "origin/main").returncode != 0:
            errors.append(f"{kind}: {source} is not on craft-contrib's main")
        latest = git(contrib, "show", f"origin/main:{path}")
        if latest.returncode == 0 and normalize(latest.stdout) != normalize(block):
            print(f"::warning file={agents}::{kind}: craft-contrib main has a newer {path}")
    for kind in ["core"] + (["rust"] if rust else []):
        if kind not in seen:
            errors.append(f"{kind}: no house-rules block")
    for error in errors:
        print(f"::error file={agents}::{error}")
    if not errors:
        print("house rules: " + ", ".join(sorted(seen)) + " match craft-contrib")
    return 1 if errors else 0


if __name__ == "__main__":
    sys.exit(main())
