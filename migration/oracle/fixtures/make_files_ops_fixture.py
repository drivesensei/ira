#!/usr/bin/env python3
"""Create a disposable listing/operations oracle tree; safe to remove as one directory."""
from pathlib import Path
import argparse

p = argparse.ArgumentParser()
p.add_argument("root", type=Path)
a = p.parse_args()
root = a.root.resolve()
left, right = root / "left", root / "right"
(left / "folder" / "nested").mkdir(parents=True, exist_ok=True)
right.mkdir(parents=True, exist_ok=True)
(left / "a.txt").write_text("source-a\n")
(left / "b.txt").write_text("source-b\n")
(left / "large.bin").write_bytes(bytes(range(256)) * 4096)
(left / ".secret").write_text("hidden\n")
(left / "folder" / "nested" / "inside.txt").write_text("inside\n")
(left / "README").write_text("extensionless\n")
(right / "a.txt").write_text("destination-a-must-survive\n")
(root / "state").write_text(
    "split=1\nactive=0\nhidden=0\npreview0=0\npreview1=0\n"
    f"left=left\t{left}\nright=right\t{right}\n"
)
print(f"left={left}\nright={right}\nstate={root / 'state'}")
