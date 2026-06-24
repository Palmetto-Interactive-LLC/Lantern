# Tutorial: Your first squad

In this tutorial we will install Lantern, start local services, launch a 9-agent coding squad in iTerm2, verify the local and Temporal-facing state, and tear the workspace down.

You will end up with a working local setup, git worktrees for worker roles, and an iTerm2 window containing nine role-specific agent panes.

## Before You Begin

You need:

- macOS with iTerm2 for the current launcher
- A git repository to work in
- git on your PATH
- `agent-runner` at `~/.local/bin/agent-runner`
- Orchestration client configured in your agent's MCP settings
- At least one agent CLI (`claude` recommended)

If you are unsure whether prerequisites are met, run the installer. It checks most local dependencies and installs the helper scripts.

## Step 1: Install Lantern

Clone the repository and run the installer:

```bash
git clone https://github.com/Palmetto-Interactive-LLC/pi-code-orchestrator.git
cd pi-code-orchestrator
./scripts/install.sh
```

You should see log lines ending with health checks. The installer creates `~/.lantern/`, builds the `lantern` binary, installs iTerm helper scripts, and adds it to your PATH in `~/.zshrc`.

Reload your shell:

```bash
source ~/.zshrc
```

Verify the binary is available:

```bash
lantern --version
```

## Step 2: Start Background Services

Start the Temporal dev server and the Relay daemon:

```bash
lantern up
```

Now run the health check:

```bash
lantern doctor
```

Address any `FAIL` lines before launching a squad. A `WARN` on Temporal usually means the service is still starting; wait a few seconds and run `lantern doctor` again.

## Step 3: Launch a Squad

Move into your git repository:

```bash
cd ~/Development/my-project
```

Launch a squad. We use a high slot number (`99`) so this tutorial session does not collide with real work:

```bash
lantern startwork myproject 99 --agent claude --no-init
```

You should see nine lines like:

```text
  + myproject-99                     ORCH (iterm: ...)
  + myproject-ai-99                  AI   (iterm: ...)
```

Lantern opens a new iTerm2 window with a 4x2+1 squad layout. Pane 0 (orchestrator) uses the repo root. The other eight panes each use a git worktree under `.claude/worktrees/myproject-99/`.

## Step 4: Verify the Squad

Check Lantern's local projection:

```bash
lantern status
```

You should see session `myproject-99` with status `active`, terminal status `iterm`, and 9 agents listed.

Open the Temporal UI when Temporal is running:

```text
http://127.0.0.1:8244
```

Use Temporal workflow state as the runtime authority. Local status output is useful inventory and audit projection.

## Step 5: Tear Down the Squad

Stop the tutorial workspace:

```bash
lantern stopwork myproject-99
```

You should see confirmation that the iTerm2 window was closed, branches removed, worktrees cleaned up, workflows terminated or requested for cleanup, and the local session record marked stopped.

Verify it is gone from local inventory:

```bash
lantern status
```

## What You Accomplished

You now have:

- Lantern installed at `~/.lantern/bin/lantern`
- Background services running
- Experience launching and stopping a full 9-agent iTerm squad
- A working mental model that Temporal owns runtime state and Lantern shows local projections

## Next Steps

- [How to launch a squad](../how-to/launch-a-squad.md) - options, agent kinds, daily workflow
- [How to manage services](../how-to/manage-services.md) - up, down, logs, restart
- [CLI reference](../reference/cli.md) - every command and flag
- [Architecture](../explanation/architecture.md) - how the pieces fit together
- [Doctor-state planning](../reference/doctor-state.md) - planned diagnostic output

Legacy note: old tutorials mentioned tmux detach/reattach and pane cleanup. Those commands applied to the previous launcher only and are not current runbook steps.
