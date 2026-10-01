#!/usr/bin/env python3
"""Create the deterministic, disposable tree used by entry/input oracle captures."""

from pathlib import Path
import sys


def main() -> None:
    if len(sys.argv) != 2:
        raise SystemExit("usage: make_entry_input_fixture.py FIXTURE_DIR")
    root = Path(sys.argv[1]).resolve()
    root.mkdir(parents=True, exist_ok=True)
    (root / "alpha.txt").write_text("alpha fixture\n", encoding="utf-8")
    (root / "bravo.md").write_text("# bravo fixture\n", encoding="utf-8")
    (root / "charlie space.txt").write_text("charlie fixture\n", encoding="utf-8")
    (root / "folder").mkdir(exist_ok=True)
    (root / ".hidden-fixture").write_text("hidden fixture\n", encoding="utf-8")


if __name__ == "__main__":
    main()
