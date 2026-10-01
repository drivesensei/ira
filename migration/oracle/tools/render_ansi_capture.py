#!/usr/bin/env python3
"""Render the final basic 16/256-color terminal screen from a `script` log."""
from pathlib import Path
import argparse
from wcwidth import wcwidth

parser = argparse.ArgumentParser()
parser.add_argument("input", type=Path)
parser.add_argument("output", type=Path)
parser.add_argument("--rows", type=int, default=40)
parser.add_argument("--cols", type=int, default=120)
parser.add_argument("--through", help="stop after the first occurrence of this visible marker")
args = parser.parse_args()
raw = args.input.read_bytes()
start = raw.find(b"\x1b")
if start < 0:
    raise SystemExit("no ANSI terminal output found")
stream = raw[start:].decode("utf-8", "replace")
if args.through:
    end = stream.find(args.through)
    if end < 0:
        raise SystemExit(f"marker not found: {args.through}")
    stream = stream[:end + len(args.through)]
rows, cols = args.rows, args.cols
screen = [[" "] * cols for _ in range(rows)]
y = x = i = 0
while i < len(stream):
    ch = stream[i]
    if ch == "\x1b":
        i += 1
        if i == len(stream):
            break
        kind = stream[i]
        if kind == "[":
            i += 1
            j = i
            while j < len(stream) and not ("@" <= stream[j] <= "~"):
                j += 1
            if j == len(stream):
                break
            params, final = stream[i:j].lstrip("?").split(";"), stream[j]
            def num(index: int, default: int = 1) -> int:
                try:
                    return int(params[index]) if params[index] else default
                except (ValueError, IndexError):
                    return default
            if final in ("H", "f"):
                y, x = max(0, min(rows - 1, num(0) - 1)), max(0, min(cols - 1, num(1) - 1))
            elif final == "A": y = max(0, y - num(0))
            elif final == "B": y = min(rows - 1, y + num(0))
            elif final == "C": x = min(cols - 1, x + num(0))
            elif final == "D": x = max(0, x - num(0))
            elif final == "G": x = max(0, min(cols - 1, num(0) - 1))
            elif final == "d": y = max(0, min(rows - 1, num(0) - 1))
            elif final == "J" and num(0, 0) == 2:
                screen, y, x = [[" "] * cols for _ in range(rows)], 0, 0
            elif final == "K":
                mode = num(0, 0)
                if mode == 0:
                    for col in range(x, cols): screen[y][col] = " "
                elif mode == 1:
                    for col in range(x + 1): screen[y][col] = " "
                else: screen[y] = [" "] * cols
            i = j
        elif kind == "]":
            i += 1
            while i < len(stream) and stream[i] != "\x07" and not (stream[i] == "\x1b" and i + 1 < len(stream) and stream[i + 1] == "\\"):
                i += 1
            if i < len(stream) and stream[i] == "\x1b": i += 1
        elif kind in ("P", "_", "^"):
            i += 1
            while i < len(stream) and not (stream[i] == "\x1b" and i + 1 < len(stream) and stream[i + 1] == "\\"):
                i += 1
            if i < len(stream): i += 1
        else:
            i += 1
    elif ch == "\n": y, x = min(rows - 1, y + 1), 0
    elif ch == "\r": x = 0
    elif ch == "\t": x = min(cols - 1, (x // 8 + 1) * 8)
    elif ord(ch) >= 32:
        width = wcwidth(ch)
        if width > 0 and y < rows and x < cols:
            screen[y][x] = ch
            for offset in range(1, width):
                if x + offset < cols: screen[y][x + offset] = " "
            x = min(cols - 1, x + width)
    i += 1
args.output.write_text("\n".join("".join(row).rstrip() for row in screen).rstrip() + "\n")
