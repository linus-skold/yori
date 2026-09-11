#!/usr/bin/env python3
"""Generate source-like diff inputs without adding large files to the repository."""

from pathlib import Path
import argparse

parser = argparse.ArgumentParser()
parser.add_argument("output", type=Path)
parser.add_argument("--lines", type=int, default=5000)
args = parser.parse_args()
args.output.mkdir(parents=True, exist_ok=True)

left = args.output / f"left-{args.lines}.rs"
right = args.output / f"right-{args.lines}.rs"
with left.open("w", encoding="utf-8", newline="") as before, right.open(
    "w", encoding="utf-8", newline=""
) as after:
    for line in range(args.lines):
        before.write(f"fn row_{line}() -> usize {{ {line} }}\n")
        if line % 997 == 0:
            after.write(f"// inserted before row {line} 👋\n")
        value = line + 1 if line % 503 == 0 else line
        after.write(f"fn row_{line}() -> usize {{ {value} }}\n")

print(left)
print(right)
