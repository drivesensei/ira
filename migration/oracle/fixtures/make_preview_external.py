#!/usr/bin/env python3
"""Create a safe preview/external integration fixture and a fresh HOME."""
from pathlib import Path
import base64
import tempfile

root = Path(tempfile.mkdtemp(prefix="ira-preview-external-"))
fixture = root / "fixture"
home = root / "home"
fixture.mkdir()
home.mkdir()
(home / "README").write_text("first line\nsecond line\n", encoding="utf-8")
(home / "sample.txt").write_text("alpha\nbeta\n", encoding="utf-8")
(home / "picture.png").write_bytes(base64.b64decode(
    "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVQIHWP4z8DwHwAFgAI/ieVvxwAAAABJRU5ErkJggg=="
))
(home / "raw.dat").write_bytes(b"\0binary\n")
(home / "doc.pdf").write_text("invalid PDF fixture\n", encoding="utf-8")
(home / "clip.mp4").write_text("invalid video fixture\n", encoding="utf-8")
print(f"fixture={fixture}")
print(f"home={home}")
