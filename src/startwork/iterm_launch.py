#!/usr/bin/env python3
"""
iterm_launch.py — Create the squad window layout in iTerm2.

Layout (9 panes in 1 tab, 1 new window):
  [ORCH (33% width, full height)] | [AI  | SEC ]
                                  | [DAT | OPS ]
                                  | [PLT | UI  ]
                                  | [DOC | QA  ]

Writes to stdout (JSON):
  { "orchestrator": "session_id", "ai": "session_id", ... }

Optional --startup-file JSON map role → shell command; injected on the same
Python API connection after panes exist (reliable vs separate inject processes).
"""

import argparse
import asyncio
import json
import sys
from pathlib import Path

import iterm2


ROLE_COLORS: dict[str, tuple[int, int, int]] = {
    "orchestrator": (30, 32, 35),
    "orch": (30, 32, 35),
    "ai": (62, 49, 0),
    "dat": (45, 27, 83),
    "sec": (0, 17, 51),
    "ops": (0, 53, 58),
    "plt": (7, 57, 25),
    "ui": (78, 24, 24),
    "doc": (70, 28, 0),
    "qa": (80, 0, 80),
    "input": (45, 45, 45),
    "inp": (45, 45, 45),
}

ROLE_LABELS: dict[str, str] = {
    "orchestrator": "ORCH",
    "orch": "ORCH",
    "ai": "AI",
    "dat": "DAT",
    "sec": "SEC",
    "ops": "OPS",
    "plt": "PLT",
    "ui": "UI",
    "doc": "DOC",
    "qa": "QA",
    "input": "INPUT",
    "inp": "INPUT",
}


async def set_pane_appearance(
    session: iterm2.Session,
    role: str,
    title: str,
) -> None:
    """Tab color, session name, and OSC window titles."""
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


def resolve_tab(window: iterm2.Window) -> iterm2.Tab:
    tab = window.current_tab
    if tab is not None:
        return tab
    if not window.tabs:
        raise RuntimeError("new iTerm2 window has no tabs")
    return window.tabs[0]


def resolve_session(tab: iterm2.Tab) -> iterm2.Session:
    session = tab.current_session
    if session is not None:
        return session
    sessions = tab.sessions
    if not sessions:
        raise RuntimeError("iTerm2 tab has no sessions")
    return sessions[0]


async def configure_iterm_for_squads(connection: iterm2.Connection) -> None:
    prefs = [
        (iterm2.PreferenceKey.TAP_BAR_POSTIION, 0),
        (iterm2.PreferenceKey.HIDE_TAB_BAR_WHEN_ONLY_ONE_TAB, True),
        (iterm2.PreferenceKey.DEFAULT_TOOLBELT_WIDTH, 0),
        # Show role labels on split pane dividers
        (iterm2.PreferenceKey.SHOW_PANE_TITLES, True),
    ]
    for key, value in prefs:
        try:
            await iterm2.async_set_preference(connection, key, value)
        except Exception:
            continue  # unsupported preference key in this iTerm2 version — non-fatal


async def hide_window_toolbelt(window: iterm2.Window) -> None:
    try:
        await window.async_invoke_function("iterm2.toolbelt_hide()", timeout=2)
    except Exception:
        return  # toolbelt hide is best-effort; not all iTerm2 versions expose this API


async def apply_layout_sizes(
    window: iterm2.Window, tab: iterm2.Tab, orch: iterm2.Session, input_session: iterm2.Session
) -> None:
    try:
        frame = await window.async_get_frame()
    except Exception:
        return

    total_w = max(frame.size.width, 120)
    total_h = max(frame.size.height, 24)
    orch_w = max(int(total_w * 0.33), 40)
    worker_w = max(total_w - orch_w, 40)
    row_h = max(total_h // 4, 6)

    orch.preferred_size = iterm2.util.Size(orch_w, int(total_h * 0.66))
    input_session.preferred_size = iterm2.util.Size(orch_w, int(total_h * 0.33))
    for session in tab.sessions:
        if session.session_id in (orch.session_id, input_session.session_id):
            continue
        session.preferred_size = iterm2.util.Size(worker_w // 2, row_h)

    try:
        await tab.async_update_layout()
    except Exception:
        return  # layout update is best-effort; pane sizes may be approximate


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


async def inject_one_pane(
    connection: iterm2.Connection,
    app: iterm2.App,
    role: str,
    session: iterm2.Session,
    title: str,
    cmd: str | None,
) -> None:
    refreshed = await find_session(app, session.session_id)
    target = refreshed or session
    await set_pane_appearance(target, role, title)
    if cmd:
        if not cmd.endswith("\n"):
            cmd += "\n"
        await target.async_send_text(cmd)


async def inject_startup_commands(
    connection: iterm2.Connection,
    role_to_session: dict[str, iterm2.Session],
    titles_by_role: dict[str, str],
    startup_by_role: dict[str, str],
) -> None:
    """Launch all 9 panes concurrently — one coroutine per worker channel."""
    await asyncio.sleep(0.15)
    app = await iterm2.async_get_app(connection)
    tasks = []
    for role, session in role_to_session.items():
        cmd = startup_by_role.get(role)
        if not cmd:
            continue
        title = titles_by_role.get(role, ROLE_LABELS.get(role, role.upper()))
        tasks.append(inject_one_pane(connection, app, role, session, title, cmd))
    if tasks:
        await asyncio.gather(*tasks)


def title_for_role(role: str, session_name: str, titles_by_role: dict[str, str]) -> str:
    return titles_by_role.get(role, f"{ROLE_LABELS.get(role, role.upper())} - {session_name}")


async def main(
    connection: iterm2.Connection,
    session_id: str,
    titles_by_role: dict[str, str],
    startup_by_role: dict[str, str],
) -> None:
    await iterm2.async_get_app(connection)
    await configure_iterm_for_squads(connection)

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
    orch_session = resolve_session(tab)

    # Split vertically to create the 3 columns first
    right = await orch_session.async_split_pane(vertical=True)
    right2 = await right.async_split_pane(vertical=True)

    # Now split the Orchestrator pane horizontally to put the Input router directly under it
    input_session = await orch_session.async_split_pane(vertical=False)

    # Split the worker columns horizontally into rows
    r1c2 = await right.async_split_pane(vertical=False)
    r1c3 = await r1c2.async_split_pane(vertical=False)
    r1c4 = await r1c3.async_split_pane(vertical=False)

    r2c2 = await right2.async_split_pane(vertical=False)
    r2c3 = await r2c2.async_split_pane(vertical=False)
    r2c4 = await r2c3.async_split_pane(vertical=False)

    role_to_session: dict[str, iterm2.Session] = {
        "orchestrator": orch_session,
        "input": input_session,
        "ai": right,
        "dat": r1c2,
        "plt": r1c3,
        "doc": r1c4,
        "sec": right2,
        "ops": r2c2,
        "ui": r2c3,
        "qa": r2c4,
    }

    await apply_layout_sizes(window, tab, orch_session, input_session)

    result: dict[str, str] = {}
    appearance_tasks = []
    for role, session in role_to_session.items():
        title = title_for_role(role, session_id, titles_by_role)
        appearance_tasks.append(set_pane_appearance(session, role, title))
        result[role] = session.session_id
    if appearance_tasks:
        await asyncio.gather(*appearance_tasks)

    if startup_by_role:
        await inject_startup_commands(
            connection,
            role_to_session,
            titles_by_role,
            startup_by_role,
        )

    await orch_session.async_activate()
    print(json.dumps(result))


if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument("--session", required=True, help="Devorch session ID (e.g. m7-navi-40)")
    parser.add_argument(
        "--startup-file",
        help="JSON file mapping role → shell startup command",
    )
    parser.add_argument(
        "--titles-file",
        help="JSON file mapping role → pane title (TEAM - worktree)",
    )
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
