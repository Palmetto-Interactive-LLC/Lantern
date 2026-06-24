#!/usr/bin/env python3
"""
iterm_close.py — Close the iTerm2 window for a devorch session.

Finds any session whose name contains the session_id (set at launch) and
closes that window.

Usage:
    python3 iterm_close.py --session m7-navi-1
"""

import argparse
import sys

import iterm2


async def main(connection: iterm2.Connection, session_id: str) -> None:
    app = await iterm2.async_get_app(connection)

    for window in app.windows:
        for tab in window.tabs:
            for session in tab.sessions:
                name = session.name or ""
                if session_id in name:
                    # force=True: skip iTerm's "a process is running, close?"
                    # confirmation dialog, which otherwise blocks the API call
                    # indefinitely (the panes run live agent CLIs).
                    await window.async_close(force=True)
                    return

    # No matching window — not an error (already closed)
    print(f"No iTerm2 window found for session {session_id!r}", file=sys.stderr)


if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument("--session", required=True, help="Devorch session ID")
    args = parser.parse_args()

    iterm2.run_until_complete(lambda conn: main(conn, args.session))
