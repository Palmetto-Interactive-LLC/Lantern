#!/usr/bin/env python3
"""
iterm_launch_pattern.py — Create a pane layout in iTerm2 for a non-team
LaunchPattern (Simple Orchestrator, Executor, Fix a Bug), driven generically
by a `LayoutSpec` JSON instead of the fixed 9-pane grid `iterm_launch.py`
hardcodes for the team pattern.

LayoutSpec shape (see `patterns::LayoutSpec` in patterns.rs):
  { "columns": [ { "weight": 33, "rows": ["orch", "inp"] },
                 { "weight": 67, "rows": ["worker-1", "worker-2", ...] } ] }

Writes to stdout (JSON): { "<role>": "session_id", ... } — one entry per row
across all columns.

Optional --startup-file / --titles-file: JSON maps role -> shell command /
pane title, injected on the same Python API connection after panes exist.
"""

import argparse
import asyncio
import json
import sys
from pathlib import Path

import iterm2


async def set_pane_appearance(
    session: iterm2.Session,
    role: str,
    title: str,
    color: tuple[int, int, int] | None,
) -> None:
    r, g, b = color or (40, 40, 40)
    c = iterm2.Color(r, g, b, 255)

    change = iterm2.LocalWriteOnlyProfile()
    change.set_use_tab_color(True)
    change.set_tab_color(c)
    change.set_tab_color_light(c)
    change.set_tab_color_dark(c)
    change.set_background_color(c)
    change.set_background_color_light(c)
    change.set_background_color_dark(c)
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
        return  # best-effort; not all iTerm2 versions expose this API


async def build_columns(
    first_session: iterm2.Session, num_columns: int
) -> list[iterm2.Session]:
    """Split the window into `num_columns` vertical columns, returning each
    column's head (top) session, left to right."""
    heads = [first_session]
    for _ in range(1, num_columns):
        new_head = await heads[-1].async_split_pane(vertical=True)
        heads.append(new_head)
    return heads


async def build_rows(head: iterm2.Session, num_rows: int) -> list[iterm2.Session]:
    """Split one column's head session into `num_rows` stacked rows, top to
    bottom (row 0 is `head` itself)."""
    rows = [head]
    for _ in range(1, num_rows):
        rows.append(await rows[-1].async_split_pane(vertical=False))
    return rows


async def apply_layout_sizes(
    window: iterm2.Window,
    tab: iterm2.Tab,
    columns: list[dict],
    role_to_session: dict[str, iterm2.Session],
) -> None:
    try:
        frame = await window.async_get_frame()
    except Exception:
        return

    total_w = max(frame.size.width, 120)
    total_h = max(frame.size.height, 24)
    total_weight = sum(max(c["weight"], 1) for c in columns) or 1

    for col in columns:
        col_w = max(int(total_w * (max(col["weight"], 1) / total_weight)), 40)
        rows = col["rows"]
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
    app: iterm2.App,
    role: str,
    session: iterm2.Session,
    title: str,
    color: tuple[int, int, int] | None,
    cmd: str | None,
) -> None:
    refreshed = await find_session(app, session.session_id)
    target = refreshed or session
    await set_pane_appearance(target, role, title, color)
    if cmd:
        if not cmd.endswith("\n"):
            cmd += "\n"
        await target.async_send_text(cmd)


async def main(
    connection: iterm2.Connection,
    session_id: str,
    layout: dict,
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
    first_session = resolve_session(tab)

    columns = layout.get("columns", [])
    if not columns:
        raise RuntimeError("layout has no columns")

    column_heads = await build_columns(first_session, len(columns))

    role_to_session: dict[str, iterm2.Session] = {}
    for col, head in zip(columns, column_heads):
        rows = col.get("rows", [])
        if not rows:
            continue
        row_sessions = await build_rows(head, len(rows))
        for role, session in zip(rows, row_sessions):
            role_to_session[role] = session

    await apply_layout_sizes(window, tab, columns, role_to_session)

    result: dict[str, str] = {}
    appearance_tasks = []
    app = await iterm2.async_get_app(connection)
    for role, session in role_to_session.items():
        title = titles_by_role.get(role, role.upper())
        color = tuple(ROLE_COLOR_HINTS.get(role, (40, 40, 40)))
        appearance_tasks.append(inject_one_pane(
            app, role, session, title, color, startup_by_role.get(role)
        ))
        result[role] = session.session_id
    if appearance_tasks:
        await asyncio.gather(*appearance_tasks)

    first_session_role = columns[0]["rows"][0] if columns[0].get("rows") else None
    if first_session_role and first_session_role in role_to_session:
        await role_to_session[first_session_role].async_activate()

    print(json.dumps(result))


# Best-effort color hints for roles this script knows about; anything else
# falls back to a neutral gray. Actual per-pane background color/theming is
# driven by the caller via ROLE_COLOR_HINTS overrides in future patterns —
# today's callers (Simple Orchestrator) don't rely on this for correctness,
# only cosmetics, since the Rust side already themes panes via startup banner.
ROLE_COLOR_HINTS: dict[str, tuple[int, int, int]] = {
    "orch": (30, 32, 35),
    "inp": (45, 45, 45),
}


if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument("--session", required=True, help="Devorch session ID (e.g. m7-navi-40)")
    parser.add_argument("--layout-file", required=True, help="JSON file with the LayoutSpec")
    parser.add_argument(
        "--startup-file",
        help="JSON file mapping role → shell startup command",
    )
    parser.add_argument(
        "--titles-file",
        help="JSON file mapping role → pane title",
    )
    args = parser.parse_args()

    layout: dict = json.loads(Path(args.layout_file).read_text(encoding="utf-8"))

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
        lambda conn: main(conn, args.session, layout, titles_by_role, startup_by_role)
    )
