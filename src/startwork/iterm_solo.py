#!/usr/bin/env python3
"""Open ONE iTerm2 window with a single pane and run a command in it.

Used by `startwork --agent goose` (Option B: a single quiet Goose orchestrator
window — no 8-pane team). Returns JSON `{"orchestrator": "<session_id>"}` on
stdout so the Rust caller can register and later close it.
"""
import argparse
import json
import sys

import iterm2

_p = argparse.ArgumentParser()
_p.add_argument("--title", required=True)
_p.add_argument("--cwd", required=True)
_p.add_argument("--command", required=True)
ARGS = _p.parse_args()


async def main(connection):
    await iterm2.async_get_app(connection)
    window = await iterm2.Window.async_create(connection)
    session = window.current_tab.current_session
    try:
        await session.async_set_name(ARGS.title)
    except Exception as e:
        print(f"WARN: could not set pane title: {e}", file=sys.stderr)
    # cd into the worktree, then launch the (interactive) agent command. A single
    # trailing newline submits at the fresh shell prompt.
    await session.async_send_text("cd " + json.dumps(ARGS.cwd) + " && " + ARGS.command + "\n")
    print(json.dumps({"orchestrator": session.session_id}))


iterm2.run_until_complete(main)
