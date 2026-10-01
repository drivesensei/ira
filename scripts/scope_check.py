#!/usr/bin/env python3
"""Verify that a subagent only changed files it is allowed to change.

Usage:
  python scripts/scope_check.py reviewer  --range <base>..HEAD
  python scripts/scope_check.py advisor   --range <base>..HEAD
  python scripts/scope_check.py explorer  --range <base>..HEAD
  python scripts/scope_check.py auditor
  python scripts/scope_check.py developer --range <base>..HEAD --allow 'crates/app/src/features/rename/**'
  python scripts/scope_check.py custom --allow 'glob1' --allow 'glob2'

Without --range, checks uncommitted changes (staged, unstaged, untracked). With --range, checks files changed in
that commit range and also any uncommitted changes. Exit code 1 and a list of offending paths when out of scope.
Developers may never change the manager-owned files, regardless of --allow.
"""
from __future__ import annotations

import argparse
import re
import subprocess
import sys
from pathlib import Path

TEST_GLOBS = ["**/tests/**", "**/tests.rs", "**/*_tests.rs"]
ROLE_ALLOW = {
    "reviewer": TEST_GLOBS + ["migration/reports/**"],
    "advisor": ["migration/reports/**"],
    "auditor": ["migration/reports/**"],
    "explorer": ["migration/inventory/**", "migration/oracle/**"],
    "developer": TEST_GLOBS + ["migration/reports/**"],
    "custom": [],
}
MANAGER_OWNED = [
    "migration/PARITY_MATRIX.md",
    "migration/STATE.md",
    "migration/DECISIONS.md",
    "migration/journal.md",
    "migration/waves/**",
    "migration/specs/**",
]


def glob_to_regex(glob: str) -> re.Pattern:
    out = []
    i = 0
    while i < len(glob):
        c = glob[i]
        if glob.startswith("**/", i):
            out.append("(?:.*/)?")
            i += 3
            continue
        if glob.startswith("**", i):
            out.append(".*")
            i += 2
            continue
        if c == "*":
            out.append("[^/]*")
        elif c == "?":
            out.append("[^/]")
        else:
            out.append(re.escape(c))
        i += 1
    return re.compile("^" + "".join(out) + "$")


def git_lines(*args: str) -> list[str]:
    res = subprocess.run(["git", *args], capture_output=True, text=True)
    if res.returncode != 0:
        print(res.stderr.strip(), file=sys.stderr)
        sys.exit(2)
    return [ln for ln in res.stdout.splitlines() if ln.strip()]


def changed_files(commit_range: str | None) -> set[str]:
    files: set[str] = set()
    if commit_range:
        files.update(git_lines("diff", "--name-only", commit_range))
    files.update(git_lines("diff", "--name-only", "HEAD"))
    files.update(git_lines("ls-files", "--others", "--exclude-standard"))
    return {f.replace("\\", "/") for f in files}


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("role", choices=sorted(ROLE_ALLOW))
    ap.add_argument("--range", dest="commit_range", default=None)
    ap.add_argument("--allow", action="append", default=[])
    args = ap.parse_args()

    allow = [glob_to_regex(g) for g in ROLE_ALLOW[args.role] + args.allow]
    manager = [glob_to_regex(g) for g in MANAGER_OWNED]

    offenders: list[str] = []
    files = sorted(changed_files(args.commit_range))
    for f in files:
        if args.role == "developer" and any(m.match(f) for m in manager):
            offenders.append(f"{f}  (manager-owned)")
            continue
        if not any(a.match(f) for a in allow):
            offenders.append(f)

    print(f"role={args.role} checked={len(files)} file(s)")
    if offenders:
        print("OUT OF SCOPE:")
        for f in offenders:
            print(f"  - {f}")
        print("Revert these paths (git checkout -- <path>, or git rm for new files) and relaunch with a tighter brief.")
        return 1
    print("OK")
    return 0


if __name__ == "__main__":
    sys.exit(main())
