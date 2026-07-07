#!/usr/bin/env python3
"""Check repository Markdown files for broken relative links."""

from __future__ import annotations

import re
import sys
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
MARKDOWN_FILES = [
    ROOT / "README.md",
    ROOT / "CONTRIBUTING.md",
    ROOT / "SECURITY.md",
    ROOT / "SUPPORT.md",
    ROOT / "ROADMAP.md",
    *sorted((ROOT / "docs").rglob("*.md")),
]
LINK_RE = re.compile(r"(?<!!)\[[^\]]+\]\(([^)]+)\)")
SCHEME_RE = re.compile(r"^[a-zA-Z][a-zA-Z0-9+.-]*:")


def local_target(raw_target: str) -> str | None:
    target = raw_target.split("#", 1)[0].strip()
    if not target or SCHEME_RE.match(target) or target.startswith("mailto:"):
        return None
    if target.startswith("<") and target.endswith(">"):
        target = target[1:-1]
    return target


def main() -> int:
    missing: list[tuple[Path, str]] = []
    checked = 0

    for path in MARKDOWN_FILES:
        if not path.exists():
            missing.append((path, "<file is missing>"))
            continue

        for match in LINK_RE.finditer(path.read_text(encoding="utf-8")):
            target = local_target(match.group(1))
            if target is None:
                continue
            checked += 1
            if not (path.parent / target).resolve().exists():
                missing.append((path.relative_to(ROOT), target))

    if missing:
        for path, target in missing:
            print(f"{path}: missing {target}", file=sys.stderr)
        return 1

    print(f"checked {len(MARKDOWN_FILES)} markdown files and {checked} relative links")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
