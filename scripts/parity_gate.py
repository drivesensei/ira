#!/usr/bin/env python3
"""Completion gate for the TUI -> GPUI parity migration.

Checks (all must pass for exit code 0):
  1. PARITY_MATRIX.md: every row has a final status (VERIFIED, EQUIVALENT_VERIFIED, WAIVED(D-xxxx)),
     final rows have evidence, waivers have complete decision records.
  2. Source tree: no open GAP( notes, no GAP-FIXED( notes awaiting verification, no #[ignore] without an
     "env:" reason, no todo!() / unimplemented!().
  3. surface.txt (optional): every line appears in the matrix text.
  4. cargo (optional, --with-cargo): fmt --check, clippy -D warnings, test, for the whole workspace.

Usage:
  python scripts/parity_gate.py                       # fast mode, no cargo
  python scripts/parity_gate.py --surface migration/oracle/surface.txt
  python scripts/parity_gate.py --with-cargo --surface migration/oracle/surface.txt
  python scripts/parity_gate.py --summary             # counts only, exit 0
  python scripts/parity_gate.py --json
"""
from __future__ import annotations

import argparse
import json
import re
import shutil
import subprocess
import sys
from collections import Counter
from pathlib import Path

FINAL_STATUSES = {"VERIFIED", "EQUIVALENT_VERIFIED"}
KNOWN_STATUSES = {
    "NOT_STARTED",
    "SPECCED",
    "RED_TESTS",
    "IN_DEV",
    "IN_REVIEW",
    "FIXING",
    "VERIFIED",
    "EQUIVALENT_VERIFIED",
    "REGRESSED",
}
SCAN_EXTENSIONS = {".rs", ".toml", ".snap", ".ron", ".json", ".txt", ".trace"}
SKIP_ANYWHERE = {".git", "target", "node_modules", ".venv", "dist"}
SKIP_TOP_LEVEL = {"migration", ".cursor", "scripts"}

OPEN_GAP = re.compile(r"(?<![\w-])GAP\(")
FIXED_GAP = re.compile(r"(?<![\w-])GAP-FIXED\(")
IGNORE_ATTR = re.compile(r"#\[\s*ignore\b([^\]]*)\]")
IGNORE_REASON = re.compile(r'=\s*"([^"]*)"')
TODO_MACRO = re.compile(r"(?<![\w])(todo|unimplemented)!\s*[\(\[{]")
WAIVER = re.compile(r"^WAIVED\((D-\d+)\)$")


def repo_root(start: Path) -> Path:
    try:
        out = subprocess.run(
            ["git", "rev-parse", "--show-toplevel"],
            cwd=start,
            capture_output=True,
            text=True,
            check=True,
        ).stdout.strip()
        return Path(out)
    except Exception:
        return start


def parse_matrix(path: Path):
    """Return (rows, errors). Each row is a dict keyed by lowercase column name."""
    errors: list[str] = []
    if not path.exists():
        return [], [f"matrix not found: {path}"]
    header: list[str] | None = None
    rows: list[dict] = []
    for lineno, line in enumerate(path.read_text(encoding="utf-8").splitlines(), 1):
        stripped = line.strip()
        if not stripped.startswith("|"):
            continue
        cells = [c.strip() for c in stripped.strip("|").split("|")]
        if cells and cells[0].lower() == "id":
            header = [c.lower() for c in cells]
            continue
        if set("".join(cells)) <= set("-: "):
            continue
        if not cells[0].startswith("F-"):
            continue
        if header is None:
            errors.append(f"{path}:{lineno}: row before header")
            continue
        if len(cells) != len(header):
            errors.append(f"{path}:{lineno}: expected {len(header)} columns, found {len(cells)} ({cells[0]})")
            continue
        row = dict(zip(header, cells))
        row["_line"] = lineno
        rows.append(row)
    ids = Counter(r["id"] for r in rows)
    for rid, n in ids.items():
        if n > 1:
            errors.append(f"duplicate matrix id {rid}")
    return rows, errors


def decision_block(decisions_text: str, decision_id: str) -> str | None:
    pattern = re.compile(rf"^##\s+{re.escape(decision_id)}\b.*?(?=^##\s|\Z)", re.M | re.S)
    m = pattern.search(decisions_text)
    return m.group(0) if m else None


def check_matrix(rows, decisions_path: Path):
    violations: list[str] = []
    counts: Counter = Counter()
    decisions_text = decisions_path.read_text(encoding="utf-8") if decisions_path.exists() else ""
    if not rows:
        violations.append("matrix has no feature rows")
    for r in rows:
        status = r.get("status", "")
        base = status.split("(")[0]
        counts[base if base in KNOWN_STATUSES else ("WAIVED" if status.startswith("WAIVED") else status or "EMPTY")] += 1
        rid = r["id"]
        gaps = r.get("gaps", "-")
        evidence = r.get("evidence", "-")
        waiver = WAIVER.match(status)
        if status in FINAL_STATUSES:
            if evidence in ("", "-", "n/a", "N/A") or len(evidence) < 12:
                violations.append(f"{rid}: status {status} but evidence is missing or vague")
            if gaps not in ("", "-"):
                violations.append(f"{rid}: status {status} but Gaps column lists {gaps}")
        elif waiver:
            block = decision_block(decisions_text, waiver.group(1))
            if block is None:
                violations.append(f"{rid}: waiver {waiver.group(1)} has no decision record in DECISIONS.md")
            else:
                for role in ("Advisor", "Adversarial", "Logic"):
                    m = re.search(rf"^-\s*{role}:\s*(.+)$", block, re.M | re.I)
                    if not m or m.group(1).strip().lower() in ("", "pending", "tbd", "-"):
                        violations.append(f"{rid}: waiver {waiver.group(1)} missing sign-off line '- {role}: ...'")
        else:
            violations.append(f"{rid}: status {status or 'EMPTY'} is not final")
    return violations, counts


def iter_source_files(root: Path):
    for p in root.rglob("*"):
        if not p.is_file() or p.suffix not in SCAN_EXTENSIONS:
            continue
        rel_parts = p.relative_to(root).parts
        dirs = rel_parts[:-1]
        if any(part in SKIP_ANYWHERE for part in dirs):
            continue
        if dirs and dirs[0] in SKIP_TOP_LEVEL:
            continue
        yield p


def check_tree(root: Path, allow_todo: bool):
    violations: list[str] = []
    stats = Counter()
    for p in iter_source_files(root):
        try:
            text = p.read_text(encoding="utf-8", errors="replace")
        except OSError:
            continue
        rel = p.relative_to(root).as_posix()
        for lineno, line in enumerate(text.splitlines(), 1):
            if OPEN_GAP.search(line):
                stats["open_gap"] += 1
                violations.append(f"{rel}:{lineno}: open {line.strip()[:110]}")
            if FIXED_GAP.search(line):
                stats["fixed_gap"] += 1
                violations.append(f"{rel}:{lineno}: awaiting verification {line.strip()[:100]}")
        if p.suffix == ".rs":
            for m in IGNORE_ATTR.finditer(text):
                lineno = text.count("\n", 0, m.start()) + 1
                reason = IGNORE_REASON.search(m.group(1))
                if not reason:
                    stats["bad_ignore"] += 1
                    violations.append(f"{rel}:{lineno}: #[ignore] without reason")
                elif reason.group(1).startswith("GAP"):
                    stats["open_gap"] += 1
                    violations.append(f"{rel}:{lineno}: test still ignored for an open gap ({reason.group(1)})")
                elif not reason.group(1).startswith("env:"):
                    stats["bad_ignore"] += 1
                    violations.append(f"{rel}:{lineno}: #[ignore] reason must start with 'env:' (found '{reason.group(1)}')")
            if not allow_todo:
                for lineno, line in enumerate(text.splitlines(), 1):
                    code = line.split("//", 1)[0]
                    if TODO_MACRO.search(code):
                        stats["todo"] += 1
                        violations.append(f"{rel}:{lineno}: todo!/unimplemented! present")
    return violations, stats


def check_surface(surface_path: Path, matrix_path: Path):
    if not surface_path.exists():
        return [f"surface file not found: {surface_path}"]
    matrix_text = matrix_path.read_text(encoding="utf-8") if matrix_path.exists() else ""
    cells = set()
    for line in matrix_text.splitlines():
        if line.strip().startswith("| F-"):
            for cell in line.strip().strip("|").split("|"):
                for item in cell.split(","):
                    cells.add(item.strip())
    missing = []
    for raw in surface_path.read_text(encoding="utf-8").splitlines():
        item = raw.strip()
        if not item or item.startswith("#"):
            continue
        if item not in cells:
            missing.append(f"surface item not covered by any matrix row: {item}")
    return missing


def run_cargo(root: Path):
    if shutil.which("cargo") is None:
        return ["cargo not found on PATH"]
    steps = [
        ["cargo", "fmt", "--all", "--", "--check"],
        ["cargo", "clippy", "--workspace", "--all-targets", "--", "-D", "warnings"],
        ["cargo", "test", "--workspace"],
    ]
    failures = []
    for cmd in steps:
        print(f"$ {' '.join(cmd)}", flush=True)
        rc = subprocess.run(cmd, cwd=root).returncode
        if rc != 0:
            failures.append(f"command failed ({rc}): {' '.join(cmd)}")
    return failures


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--root", default=".", help="repo root (default: git toplevel of cwd)")
    ap.add_argument("--matrix", default="migration/PARITY_MATRIX.md")
    ap.add_argument("--decisions", default="migration/DECISIONS.md")
    ap.add_argument("--surface", default=None)
    ap.add_argument("--with-cargo", action="store_true")
    ap.add_argument("--allow-todo", action="store_true", help="do not flag todo!/unimplemented! (not for the final gate)")
    ap.add_argument("--summary", action="store_true", help="print matrix counts and exit 0")
    ap.add_argument("--json", action="store_true")
    args = ap.parse_args()

    root = repo_root(Path(args.root).resolve())
    matrix_path = root / args.matrix
    rows, parse_errors = parse_matrix(matrix_path)
    matrix_violations, counts = check_matrix(rows, root / args.decisions)

    if args.summary:
        total = sum(counts.values())
        done = sum(counts[s] for s in FINAL_STATUSES) + counts.get("WAIVED", 0)
        pct = (100.0 * done / total) if total else 0.0
        print(f"rows={total} final={done} ({pct:.1f}%)")
        for status, n in sorted(counts.items()):
            print(f"  {status}: {n}")
        return 0

    tree_violations, tree_stats = check_tree(root, args.allow_todo)
    surface_violations = check_surface(root / args.surface, matrix_path) if args.surface else []
    cargo_violations = run_cargo(root) if args.with_cargo else []

    groups = {
        "matrix_parse": parse_errors,
        "matrix": matrix_violations,
        "tree": tree_violations,
        "surface": surface_violations,
        "cargo": cargo_violations,
    }
    total_violations = sum(len(v) for v in groups.values())

    if args.json:
        print(json.dumps({"ok": total_violations == 0, "counts": dict(counts), "tree_stats": dict(tree_stats), **groups}, indent=2))
    else:
        for name, items in groups.items():
            if items:
                print(f"\n[{name}] {len(items)} violation(s)")
                for item in items[:200]:
                    print(f"  - {item}")
                if len(items) > 200:
                    print(f"  ... {len(items) - 200} more")
        print()
        print("matrix counts:", dict(counts))
        print("tree stats:", dict(tree_stats))
        print("RESULT:", "PASS" if total_violations == 0 else f"FAIL ({total_violations} violation(s))")
    return 0 if total_violations == 0 else 1


if __name__ == "__main__":
    sys.exit(main())
