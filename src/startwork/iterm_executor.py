#!/usr/bin/env python3
"""
iterm_executor.py — Create the Executor-pattern window layout in iTerm2.

Layout (3 panes in 1 tab, 1 new window):
  [EXECUTOR (70% width, 66% height)] | [ADVISOR (30% width, full height)]
  [INPUT    (70% width, 33% height)] |

Writes to stdout (JSON): { "executor": "session_id", "advisor": "session_id", "inp": "session_id" }

Mirrors iterm_launch.py's pane-appearance/startup-injection conventions but
for the fixed 3-role Executor pattern layout.
"""

import argparse
import asyncio
import json
import sys
from pathlib import Path

import iterm2

ROLE_COLORS: dict[str, tuple[int, int, int]] = {
    "executor": (30, 32, 35),
    "advisor": (45, 27, 83),
    "input": (45, 45, 45),
}

ROLE_LABELS: dict[str, str] = {
    "executor": "EXEC",
    "advisor": "ADVISOR",
    "input": "INPUT",
}


async def set_pane_appearance(session: "iterm2.Session", role: str, title: str) -> None:
    r, g, b = ROLE_COLORS.get(role, (40, 40, 40))
    color = iterm2.Color(r, g, b, 255)

    change = iterm2.LocalWriteOnlyProfile()
    change.set_use_tab_color(True)
    change.set_tab_color(color)
    change.set_tab_color_light(color)
    change.set_tab_color_dark(color)
    change.set_background_color(color)
    change.set_background_color_light(color)
    change.set_background_color_dark(color)
    change.set_foreground_color(iterm2.Color(220, 220, 220, 255))
    await session.async_set_profile_properties(change)

    await session.async_set_name(title)
    osc = f"\x1b]0;{title}\x07\x1b]1;{title}\x07\x1b]2;{title}\x07"
    await session.async_inject(osc.encode("utf-8"))


def resolve_tab(window: "iterm2.Window") -> "iterm2.Tab":
    tab = window.current_tab
    if tab is not None:
        return tab
    if not window.tabs:
        raise RuntimeError("new iTerm2 window has no tabs")
    return window.tabs[0]


def resolve_session(tab: "iterm2.Tab") -> "iterm2.Session":
    session = tab.current_session
    if session is not None:
        return session
    sessions = tab.sessions
    if not sessions:
        raise RuntimeError("iTerm2 tab has no sessions")
    return sessions[0]


async def hide_window_toolbelt(window: "iterm2.Window") -> None:
    try:
        await window.async_invoke_function("iterm2.toolbelt_hide()", timeout=2)
    except Exception:
        return


async def apply_layout_sizes(
    window: "iterm2.Window",
    tab: "iterm2.Tab",
    executor: "iterm2.Session",
    input_session: "iterm2.Session",
) -> None:
    try:
        frame = await window.async_get_frame()
    except Exception:
        return
    total_w = max(frame.size.width, 120)
    total_h = max(frame.size.height, 24)
    exec_w = max(int(total_w * 0.7), 40)
    executor.preferred_size = iterm2.util.Size(exec_w, int(total_h * 0.66))
    input_session.preferred_size = iterm2.util.Size(exec_w, int(total_h * 0.33))
    try:
        await tab.async_update_layout()
    except Exception:
        return


async def find_session(app: "iterm2.App", session_id: str) -> "iterm2.Session | None":
    session = app.get_session_by_id(session_id)
    if session is not None:
        return session
    for window in app.windows:
        for tab in window.tabs:
            for s in tab.sessions:
                if s.session_id == session_id:
                    return s
    return None


async def inject_one_pane(
    app: "iterm2.App", role: str, session: "iterm2.Session", title: str, cmd: str | None
) -> None:
    refreshed = await find_session(app, session.session_id)
    target = refreshed or session
    await set_pane_appearance(target, role, title)
    if cmd:
        if not cmd.endswith("\n"):
            cmd += "\n"
        await target.async_send_text(cmd)


async def main(
    connection: "iterm2.Connection",
    session_id: str,
    titles_by_role: dict[str, str],
    startup_by_role: dict[str, str],
) -> None:
    await iterm2.async_get_app(connection)

    window = await iterm2.Window.async_create(connection)
    if window is None:
        print(json.dumps({"error": "Failed to create iTerm2 window"}), file=sys.stderr)
        sys.exit(1)

    for _ in range(20):
        app = await iterm2.async_get_app(connection)
        refreshed = app.get_window_by_id(window.window_id)
        if refreshed is not None and refreshed.tabs:
            window = refreshed
            break
        await asyncio.sleep(0.05)

    tab = resolve_tab(window)
    await tab.async_activate()
    await hide_window_toolbelt(window)
    executor_session = resolve_session(tab)

    advisor_session = await executor_session.async_split_pane(vertical=True)
    input_session = await executor_session.async_split_pane(vertical=False)

    role_to_session: dict[str, "iterm2.Session"] = {
        "executor": executor_session,
        "advisor": advisor_session,
        "input": input_session,
    }

    await apply_layout_sizes(window, tab, executor_session, input_session)

    result: dict[str, str] = {}
    appearance_tasks = []
    for role, session in role_to_session.items():
        title = titles_by_role.get(role, ROLE_LABELS.get(role, role.upper()))
        appearance_tasks.append(set_pane_appearance(session, role, title))
        result[role] = session.session_id
    if appearance_tasks:
        await asyncio.gather(*appearance_tasks)

    if startup_by_role:
        await asyncio.sleep(0.15)
        app = await iterm2.async_get_app(connection)
        tasks = []
        for role, session in role_to_session.items():
            cmd = startup_by_role.get(role)
            if not cmd:
                continue
            title = titles_by_role.get(role, ROLE_LABELS.get(role, role.upper()))
            tasks.append(inject_one_pane(app, role, session, title, cmd))
        if tasks:
            await asyncio.gather(*tasks)

    await executor_session.async_activate()
    print(json.dumps(result))


if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument("--session", required=True, help="Devorch session ID (e.g. myrepo-3)")
    parser.add_argument("--startup-file", help="JSON file mapping role -> shell startup command")
    parser.add_argument("--titles-file", help="JSON file mapping role -> pane title")
    args = parser.parse_args()

    startup_by_role: dict[str, str] = {}
    if args.startup_file:
        path = Path(args.startup_file)
        if path.is_file():
            startup_by_role = json.loads(path.read_text(encoding="utf-8"))

    titles_by_role: dict[str, str] = {}
    if args.titles_file:
        path = Path(args.titles_file)
        if path.is_file():
            titles_by_role = json.loads(path.read_text(encoding="utf-8"))

    iterm2.run_until_complete(
        lambda conn: main(conn, args.session, titles_by_role, startup_by_role)
    )
