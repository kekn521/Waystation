# Station: terminal workflow hub

Status: visual direction and build specification approved by the user on 2026-10-03.

## Purpose and approved direction

Station is a personal, keyboard-first terminal home for starting work, returning
to existing sessions, and inspecting the machine and processes supporting that
work. It connects the user's tools through project context and searchable
actions. It runs locally as a native Rust application using Ratatui.

The approved direction is Dispatch, using the computer's Catppuccin Macchiato
palette and Fira Code terminal font. The visual reference is
[Dispatch v2](/home/kekn521/station-design/dispatch-v2.html).
The reference's project states, metrics, tasks, ports, and hosts are examples;
the application must derive those values from actual observations or show an
explicit empty/unavailable state.

The first screen should answer: what can I resume, what needs my attention,
what is running, and how much capacity does this machine have?

## Interface

Keep the reference's header, grouped navigation, command search, four-pane
overview, and contextual shortcut footer. Main navigation contains Overview,
Workspaces, Agents, Tasks, Services, Connections, Files, System, and Activity.

The overview gives the largest pane to recent/pinned workspaces and their Git
state. The other panes show machine resources, Station task runs, and local
services/listeners. Attention items link directly to the relevant detail view.
An occupied port is informational unless a configured task requires that port;
an ordinary listening service must not automatically become a warning.

Use Macchiato base `#24273a`, mantle `#1e2030`, crust `#181926`, text `#cad3f5`,
subtext `#a5adcb`, separators `#494d64`, mauve `#c6a0f6`, blue `#8aadf4`,
teal `#8bd5ca`, green `#a6da95`, peach `#f5a97f`, and red `#ed8796`.
Selected panes use lavender/mauve outlines; selected rows have a subdued mauve
background. Rounded terminal box borders, block gauges, and sparklines provide
the visual detail. Status includes words or symbols as well as color.
Use the terminal's font rather than changing terminal configuration.

At 120 columns and 38 rows, show the navigation rail and four-pane overview.
At 90–119 columns, collapse the navigation rail into a compact section bar.
Below 90 columns or 30 rows, show one focused pane with section/pane switching;
do not shrink text or clip actions. Below 60 columns or 18 rows, show a small
resize message and an available quit key. Terminal dimensions, not browser
pixels, govern these breakpoints.

## Functional scope

| Area | First native release |
| --- | --- |
| Workspaces | Configured project roots, bounded discovery, pins, recents, Git branch/changes, read-only worktree listing, project editor/shell actions, and tmux session attach. |
| Agents | Launch installed Herdr, Codex, and Claude Code in the selected workspace. Herdr retains ownership of its agent sessions. Launch arguments are verified against installed tools. |
| Tasks | Named, configured commands; background execution; running/completed/failed state; bounded log viewing; rerun; persisted run metadata. A recipe can be a test, build, training job, or personal script. |
| Services | Read-only Docker container summary and local listening ports, with process/workspace attribution when available. Open logs and related workspace. No automatic container or service changes. |
| Connections | Saved SSH aliases, explicit connect, and configured tunnel recipes represented as managed tasks. Display saved destinations without implying reachability. |
| Files | Browse within the selected workspace, open files in the configured editor, open a shell at a directory, and copy a path when supported. |
| System | CPU, memory, disk, network throughput, process summary, optional NVIDIA GPU measurements, and launch of installed htop. |
| Activity | Recent Station launches and task outcomes, with links to workspace/log detail. It is not a reconstruction of shell history or other applications' private activity. |
| Search | One fuzzy-searchable action catalogue spanning projects, installed tools, task recipes, SSH aliases, sessions, and files already loaded in the current directory. |

Discovery defaults to direct children of `~/code` plus `~/dotfiles` if present.
The user can add roots explicitly. Discovery depth is capped at two levels,
deduplicates canonical paths, and excludes dependency, build, and hidden
directories. It must not recursively crawl the home directory on startup.

Git and worktree views are observational. Git editing actions open a configured
external Git tool when installed; otherwise show useful read-only details.
External tools are not installed automatically. Missing optional integrations
remain understandable and do not block the home screen.

## Interaction and process ownership

`1`–`9` select sections; `/` opens/focuses the command palette; arrows and `j/k`
move selection; Tab changes pane focus; Enter opens the selected item's primary
action. In workspace context, `e` opens the editor, `t` opens a shell, `g` opens
Git detail, `h` opens Herdr, and `f` opens files. `?` shows contextual help and
Escape closes the current transient view. Text entry consumes printable keys.
Mouse selection supplements keyboard operation.

Interactive programs take over the full terminal. Station suspends drawing and
restores normal terminal state before spawning them, then restores its UI and
refreshes the relevant data when they return. Long-lived interactive sessions
use tmux when configured; Station does not implement a terminal emulator.

Task recipes specify an executable/argument array and working directory.
Station passes arguments directly, without interpreting a shell command string.
A shell script is an explicit recipe executable. Search selects registered
actions; it is not an implicit shell interpreter.

Starting a background task is an explicit action. Record an immutable run ID,
recipe ID, working directory, start/end times, exit status, log location, and
process identity. Start each run in its own process group. Quitting Station
while tasks run offers keep-running, stop-owned-tasks, or cancel. After restart,
reconnect only when the saved process identity still matches; a PID alone is
insufficient. Unverifiable historical runs are marked unknown, not running.
Task-stop actions affect only verified Station-owned process groups and require
confirmation. Rerunning a task preserves its earlier run and logs.

SSH authentication and host-key prompts belong to the normal SSH client.
Recognize Host aliases and bounded local Include files; ignore wildcard-only
patterns as launchable destinations. Do not read private keys. Tunnel recipes
specify endpoints explicitly and use ordinary SSH authentication.

## Architecture and state

Use these boundaries:

- **UI and navigation:** Ratatui views, focus, selection, command palette, and
  contextual help. They consume typed snapshots and emit actions.
- **Providers:** independent adapters for projects/Git, Linux system data,
  Docker, listeners, SSH aliases, and tmux. Each returns data plus freshness and
  availability information.
- **Action runner:** validates working directories and executable availability,
  launches foreground tools, and restores terminal state on every exit path.
- **Task supervisor:** owns background process lifecycle, logs, and run history.
- **Configuration and persistence:** TOML configuration and atomic state writes.

Each managed task runs under a small supervisor process that can outlive the
UI. The supervisor captures output and writes the final exit result atomically,
so keeping a task running after Station closes does not lose its completion
status. UI restart reads that state and verifies the supervisor/process
identities before exposing lifecycle actions.

Blocking subprocesses and filesystem work stay off the UI thread. Providers
have bounded concurrency and timeouts, send results through messages, and
discard stale results after a workspace switch. Refresh system metrics about
once per second, task state once per second, and Git/services about every five
seconds while visible. Slow or failed providers do not freeze keyboard input.
Keep bounded sparkline history and cap subprocess output and log tail sizes.

Use Linux `/proc` and filesystem statistics for baseline monitoring. Use
`nvidia-smi`, `docker`, `ss`, `git`, `ssh`, and `tmux` only where available and
necessary. Resolve executables at runtime. Detect rate metrics after two
samples; show warming-up before a valid interval exists. Never present an
unavailable GPU or permission-denied process as zero usage.

Configuration lives under `$XDG_CONFIG_HOME/station/config.toml`, defaulting to
`~/.config/station/config.toml`. State/logs live under
`$XDG_STATE_HOME/station`, defaulting to `~/.local/state/station`. Configuration
includes project roots/pins, editor and tool argument arrays, task recipes,
connection recipes, and theme preferences. Keep recent workspaces, selected
workspace, task metadata, and Station activity in state. Invalid configuration
produces an actionable error without overwriting the file.

First launch works with defaults and explains how to add a recipe. No fabricated
tasks, sessions, hosts, or metrics appear. No shell startup, Kitty, desktop, or
system service settings are changed as part of installing Station.

## Errors and verification

Show permission denied, unavailable executable, disconnected Docker, missing
workspace, stale data, and command failure at their relevant surface, with a
retry or configuration action. Preserve selection through refreshes. A task
that exits nonzero remains visible with its log. A disconnected SSH session
returns control to Station with its exit outcome.

Verification must include:

- Rust formatting, linting, and tests for configuration, action arguments,
  provider parsing, task identity/lifecycle, search, and key routing.
- Ratatui TestBackend snapshots at wide, compact, and minimum terminal sizes,
  including long names, empty data, unavailable providers, and log errors.
- PTY checks for editor/shell/SSH handoff, resize, interruption, and terminal
  restoration. Use controlled local commands rather than connecting to a real
  remote host as a test.
- Live read-only checks of this computer's system metrics and available
  integrations, followed by a visual inspection in its terminal theme.
- A clean build and reproducible run/install instructions for a single local
  `station` executable.

## Delivery boundary

Build in `/home/kekn521/code/station`. Keep the approved HTML preview as the
visual reference. The native app is complete when the specified views are
usable with live data or truthful empty states, keyboard navigation and
foreground handoffs work, task runs/logs persist correctly, and the required
checks pass. Changes to desktop shortcuts or terminal autostart are a separate
user choice after the app is working.

Embedded editor/terminal implementations, automatic Git changes, automatic
service repair, cloud sync, and third-party plugin loading are outside this
release. They are not needed to make the hub useful.
