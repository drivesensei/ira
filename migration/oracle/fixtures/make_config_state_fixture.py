#!/usr/bin/env python3
"""Create isolated config/state oracle fixtures for the config-state slice."""
from pathlib import Path
import argparse

p = argparse.ArgumentParser()
p.add_argument("root", type=Path)
p.add_argument("--seed-state", action="store_true")
p.add_argument("--seed-theme", action="store_true")
p.add_argument("--hidden-off", action="store_true")
a = p.parse_args()
root = a.root.resolve()
home = root / "home"
config = root / "xdg-config" / "ira"
files = root / "fixture" / "left"
right = root / "fixture" / "right"
for path in (home, config, files, right):
    path.mkdir(parents=True, exist_ok=True)
for name, content in {
    "alpha.txt": "alpha\n", "Beta.txt": "beta\n", ".hidden": "hidden\n",
    "small.bin": "x", "large.bin": "0123456789abcdef\n",
}.items():
    (files / name).write_text(content)
(right / "right-only.txt").write_text("right\n")
if a.seed_state:
    (config / "state").write_text(
        f"split=1\nactive=1\nhidden={0 if a.hidden_off else 1}\npreview0=0\npreview1=2\n"
        f"left=Fixture Left\t{files}\nright=Fixture Right\t{right}\n"
        "theme=Tokyo_Night\nsize=1\t2\t3\t1\t1700000000\t/tmp/path with spaces\n"
        "unknown=value\n"
    )
if a.seed_theme:
    (config / "theme.toml").write_text(
        'preset = "nord"\nicons = "unicode"\nchips = "outline"\n'
        '[colors]\naccent = "#123456"\n'
    )
print(f"HOME={home}")
print(f"XDG_CONFIG_HOME={root / 'xdg-config'}")
print(f"FIXTURE={root / 'fixture'}")
