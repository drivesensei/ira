#!/usr/bin/env python3
"""Make the isolated directory used by the filesystem oracle capture."""
import os
from pathlib import Path
import shutil
import sys

root = Path(sys.argv[1]).resolve()
if root.exists():
    shutil.rmtree(root)
root.mkdir(parents=True)
(root / "alpha.txt").write_bytes(b"alpha\n")
(root / ".hidden.txt").write_bytes(b"hidden\n")
(root / "folder").mkdir()
(root / "folder" / "inside.txt").write_bytes(b"inside\n")
(root / "empty-dir").mkdir()
for link, target, is_dir in (
    ("link-dir", root / "folder", True),
    ("link-file", root / "alpha.txt", False),
    ("dangling-link", root / "missing-target", False),
):
    try:
        (root / link).symlink_to(target, target_is_directory=is_dir)
    except (OSError, NotImplementedError) as exc:
        print(f"symlink fixture {link} unavailable: {exc}")
if hasattr(os, "mkfifo"):
    try:
        os.mkfifo(root / "pipe")
    except OSError as exc:
        print(f"FIFO fixture unavailable: {exc}")
print(root)
