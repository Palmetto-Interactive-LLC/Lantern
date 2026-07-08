#!/usr/bin/env python3
"""
iterm_launch.py — Create the squad window layout in iTerm2.

Generic layout engine: builds any pane grid described by a `LayoutSpec`
(columns of weighted width, each holding rows of role names split evenly
top-to-bottom). Handles 2..=12 panes. The legacy 4x2+1 team grid (9 panes,
1 tab) is just one `LayoutSpec` shape among several — see
`src/startwork/patterns.rs` for the shapes each launch pattern produces:

  team    : [ORCH/INPUT 33%] | [AI/DAT/PLT/DOC 33%] | [SEC/OPS/UI/QA 34%]
  executor: [EXECUTOR/INPUT 70%] | [ADVISOR 30%]
  simple  : [ORCH/INPUT 33%] | [worker columns...]
  fixbug  : [FIXER/INPUT 100%]

Writes to stdout (JSON):
  { "orchestrator": "session_id", "ai": "session_id", ... }

Optional --startup-file JSON: {"commands": {role: shell command}, "layout":
{"columns": [{"weight": int, "rows": [role, ...]}, ...]}}. Commands are
injected on the same Python API connection after panes exist (reliable vs
separate inject processes). If omitted, falls back to the legacy team
layout so the script still works when invoked without one (e.g. manual
debugging).
"""

import argparse
import asyncio
import json
import os
import sys

import iterm2


def read_handoff_json(raw: str, required: bool = False) -> dict:
    """Read a JSON handoff file written by the lantern parent process.

    Only devorch-*.json files that resolve directly into the system temp
    directory are accepted, so the CLI argument cannot name an arbitrary
    filesystem path.
    """
    handoff_root = os.path.realpath("/tmp") + os.sep
    path = os.path.realpath(raw)
    if not path.startswith(handoff_root):
        raise SystemExit(f"refusing to read handoff file outside temp dir: {raw}")
    name = os.path.basename(path)
    if not (name.startswith("devorch-") and name.endswith(".json")):
        raise SystemExit(f"unexpected handoff filename: {raw}")
    if not os.path.isfile(path):
        if required:
            raise SystemExit(f"required handoff file missing: {raw}")
        return {}
    with open(path, encoding="utf-8") as fh:
        return json.load(fh)



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

# Fallback layout used only when no --startup-file (or a file with no
# "layout" key) is supplied — matches the legacy 4x2+1 team grid exactly.
DEFAULT_LAYOUT: dict = {
    "columns": [
        {"weight": 33, "rows": ["orch", "inp"]},
        {"weight": 33, "rows": ["ai", "dat", "plt", "doc"]},
        {"weight": 34, "rows": ["sec", "ops", "ui", "qa"]},
    ]
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


async def build_layout_panes(
    root: iterm2.Session, layout: dict
) -> dict[str, iterm2.Session]:
    """Walk a `LayoutSpec` ({"columns": [{"weight", "rows": [role, ...]}]})
    and split `root` into that exact grid: columns are split off left-to-right
    first, then each column's rows are split evenly top-to-bottom. Returns a
    role → session map. Works for any column/row count (2..=12 panes total),
    and reproduces the legacy hardcoded team split order/geometry exactly
    when given `DEFAULT_LAYOUT`.
    """
    columns: list[dict] = layout["columns"]
    if not columns:
        raise RuntimeError("layout has no columns")

    # Split off columns left-to-right, peeling the remainder off the
    # rightmost pane each time so column N ends up to the right of N-1.
    col_sessions: list[iterm2.Session] = []
    current = root
    for _ in range(len(columns) - 1):
        nxt = await current.async_split_pane(vertical=True)
        col_sessions.append(current)
        current = nxt
    col_sessions.append(current)

    # Split each column into its rows, evenly, top-to-bottom.
    role_to_session: dict[str, iterm2.Session] = {}
    for col_spec, col_session in zip(columns, col_sessions):
        rows: list[str] = col_spec["rows"]
        if not rows:
            continue
        row_sessions: list[iterm2.Session] = []
        cur = col_session
        for _ in range(len(rows) - 1):
            nxt = await cur.async_split_pane(vertical=False)
            row_sessions.append(cur)
            cur = nxt
        row_sessions.append(cur)
        for role, session in zip(rows, row_sessions):
            role_to_session[role] = session

    return role_to_session


async def apply_layout_sizes(
    window: iterm2.Window, tab: iterm2.Tab, layout: dict, role_to_session: dict[str, iterm2.Session]
) -> None:
    """Size each pane from `layout`'s column weights (left-to-right) and an
    even top-to-bottom split of each column's row count."""
    try:
        frame = await window.async_get_frame()
    except Exception:
        return

    total_w = max(frame.size.width, 120)
    total_h = max(frame.size.height, 24)
    columns: list[dict] = layout["columns"]
    total_weight = sum(c.get("weight", 0) for c in columns) or 1

    for col_spec in columns:
        col_w = max(int(total_w * col_spec.get("weight", 0) / total_weight), 40)
        rows: list[str] = col_spec["rows"]
        row_h = max(total_h // max(len(rows), 1), 6)
        for role in rows:
            session = role_to_session.get(role)
            if session is None:
                continue
            session.preferred_size = iterm2.util.Size(col_w, row_h)

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
    """Launch all panes concurrently — one coroutine per channel."""
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
    layout: dict,
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
    root_session = resolve_session(tab)

    role_to_session = await build_layout_panes(root_session, layout)

    await apply_layout_sizes(window, tab, layout, role_to_session)

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

    await root_session.async_activate()
    print(json.dumps(result))


if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument("--session", required=True, help="Devorch session ID (e.g. m7-navi-40)")
    parser.add_argument(
        "--startup-file",
        help="JSON file: {\"commands\": {role: shell command}, \"layout\": LayoutSpec}",
    )
    parser.add_argument(
        "--titles-file",
        help="JSON file mapping role → pane title (TEAM - worktree)",
    )
    args = parser.parse_args()

    startup_by_role: dict[str, str] = {}
    layout: dict = DEFAULT_LAYOUT
    if args.startup_file:
        payload = read_handoff_json(args.startup_file)
        startup_by_role = payload.get("commands", {})
        layout = payload.get("layout") or DEFAULT_LAYOUT

    titles_by_role: dict[str, str] = {}
    if args.titles_file:
        titles_by_role = read_handoff_json(args.titles_file)

    iterm2.run_until_complete(
        lambda conn: main(conn, args.session, titles_by_role, startup_by_role, layout)
    )
