#!/usr/bin/env python3
"""
iterm_set_titles.py — Set iTerm2 session names and OSC titles by session UUID.

Usage:
    python3 iterm_set_titles.py --titles-file /tmp/titles.json

JSON format: { "<iterm_session_id>": "ORCH - m7-navi-52", ... }
"""

import argparse
import json
import sys
from pathlib import Path

import iterm2


async def find_session(app: iterm2.App, session_id: str) -> iterm2.Session | None:
    session = app.get_session_by_id(session_id)
    if session is not None:
        return session
    for window in app.windows:
        for tab in window.tabs:
            for s in tab.sessions:
                if s.session_id == session_id:
                    return s
    return None


async def apply_titles(connection: iterm2.Connection, titles: dict[str, str]) -> None:
    app = await iterm2.async_get_app(connection)
    for session_id, title in titles.items():
        session = await find_session(app, session_id)
        if session is None:
            print(f"WARN: session {session_id!r} not found", file=sys.stderr)
            continue
        await session.async_set_name(title)
        osc = f"\x1b]0;{title}\x07\x1b]1;{title}\x07\x1b]2;{title}\x07"
        await session.async_inject(osc.encode("utf-8"))


async def main(connection: iterm2.Connection, titles: dict[str, str]) -> None:
    await apply_titles(connection, titles)


if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument(
        "--titles-file",
        required=True,
        help="JSON map of iTerm2 session_id → pane title",
    )
    args = parser.parse_args()

    path = Path(args.titles_file)
    if not path.is_file():
        print(f"ERROR: titles file not found: {path}", file=sys.stderr)
        sys.exit(1)

    titles = json.loads(path.read_text(encoding="utf-8"))
    if not isinstance(titles, dict):
        print("ERROR: titles file must be a JSON object", file=sys.stderr)
        sys.exit(1)

    iterm2.run_until_complete(lambda conn: main(conn, titles))
