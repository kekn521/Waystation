# Station

A project-aware terminal workflow hub for Linux, built in Rust with
[Ratatui](https://ratatui.rs/) and themed with Catppuccin Macchiato.
Station shows your workspaces, persistent agent sessions, system metrics,
files, and durable background tasks in one keyboard-driven dashboard. It never
autostarts anything: every agent session, task, tunnel, or tool launches only
when you explicitly start it.

## Build and install

```sh
cargo build --release --locked   # binary at target/release/station
cargo install --path . --locked  # or install into ~/.cargo/bin
```

Run it:

```sh
station
station --config PATH --state-dir PATH   # override either location
```

## Files

| What | Default | Override |
| --- | --- | --- |
| Config | `~/.config/station/config.toml` | `--config PATH` or `$XDG_CONFIG_HOME` |
| UI-created tasks (one JSON per task) | `~/.config/station/config.tasks/` | follows the config path, extension replaced by `tasks` |
| State (task logs, history, agent session metadata) | `~/.local/state/station` | `--state-dir PATH` or `$XDG_STATE_HOME` |

A missing config file is fine — sensible defaults are used and nothing is
written to your configuration file. See [`config.example.toml`](config.example.toml) for all
options; `project_roots`/`pinned_projects` accept `~/`, and `[theme]` offers
`accent = "mauve" | "blue"` plus a `compact` mode. Task and tunnel examples in
that file never run on startup, and the `[editor]` override is optional.

## Keys

| Key | Action |
| --- | --- |
| `1`–`9` | Jump to a section |
| `/` | Search |
| `j` / `k`, arrows | Move |
| `Tab` | Switch pane |
| `Enter` | Activate selection |
| `e` | Open editor |
| `t` | Shell in the selected workspace |
| `g` | Git view |
| `h` | Herdr |
| `f` | Files |
| `n` | New session in Agents; saved tasks/history elsewhere |
| `a` | Add task in Tasks |
| `F12` | Return from an agent to Station |
| `r` | Rerun task |
| `x` | Close selected agent or stop selected task (confirmation required) |
| `.` | Toggle hidden items |
| `y` | Copy |
| `F5` | Refresh |
| `?` | Help |
| `q` | Quit — while tasks run, Station asks: keep running, stop them, or cancel |

## Agents

Press **3 → n** to create a named Codex or Claude session. Enter a name,
choose the tool and project, then **Ctrl+S** creates and opens it. Tab moves
between fields; left/right chooses the tool or project. Enter opens an
existing session; **F12 returns to Station** without stopping it. Use `/` to
find another session or project. Sessions are grouped by project, with the
current project first.

Station uses a private tmux server and requires `tmux`. Your ordinary tmux
and Herdr sessions keep their own configuration. Named sessions survive
Station quitting or restarting, but not a computer reboot. `q` leaves agents
running; `x` closes a selected session after confirmation, ending its process
and scrollback. Unavailable sessions can be closed and recreated.

Status means the process is running, exited, or unavailable; it does not infer
whether the model needs input. Exited output stays in scrollback: Ctrl+B then
`[` enters copy mode, `q` leaves it, and F12 returns to Station. Configure
custom invocations through `[tools.codex]` or `[tools.claude]`.

## Tasks

A recipe is a saved command you can run again. Press **4 → a** to add one:
enter a name (such as “Test suite”), a command (`cargo test`), and a project.
**Ctrl+S saves without running**. Enter on the saved task starts it; `n`
switches between saved tasks and history, Enter opens a run's logs, `r` reruns
it, and `x` stops it.

The command field supports quotes and backslash escaping, then launches
literal program arguments. It does not expand variables or wildcards and
rejects unquoted shell operators. For shell behavior, explicitly use a script
or a command such as `sh -c 'npm test && npm run build'`. Existing TOML recipes
still use `program` + `args` arrays.

Form-created tasks are stored in `config.tasks/<UUID>.json` beside the default
config; a custom config path uses the same path with its extension replaced
by `tasks`. Both sources load together. Your handwritten TOML stays intact.

Each run has a durable supervisor that owns its process group. Stop requests
use a private socket and verify ownership. Logs roll through four 5 MiB
segments per run; the UI shows a 64 KiB tail and retains run history.

## System information

Baseline CPU, memory, and network metrics come from `/proc`, with disk capacity from filesystem statistics. GPU stats
appear only when `nvidia-smi` is available.

## External tools

Station uses tools you already have installed, detecting each from an
optional set: editors `hx`/`nvim`/`vim`/`vi` (or the `[editor]` override),
plus `herdr`, `codex`, `claude`, `git`, `lazygit`, `tmux`, `ssh`, `htop`,
`docker`, `ss`, `nvidia-smi`, and `wl-copy`/`xclip`. Missing tools produce an unavailable message with configuration guidance.

## SSH tunnels

Tunnel hosts can use a hostname or saved SSH alias. Station lists aliases from
ssh_config with bounded `Include` files. Conditional settings are
left to SSH itself, which performs the actual connection.

## Verification and development

Requires Linux, a UTF-8 terminal, and a Rust toolchain supporting edition 2024.

```sh
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test --all-targets
cargo build --release --locked
python3 scripts/pty-check.py target/release/station
python3 scripts/workflow-pty.py target/release/station
```

The PTY checks use temporary local commands for editor, shell, and SSH handoff;
they never contact a remote host. Agent workflow checks use local stand-ins
and a private tmux server, without invoking a real AI provider. Task tests require permission to create local
Unix sockets. File and subprocess discovery runs in a bounded worker pool.
Git and services refresh about every five seconds; system and task state refresh
about every second. CPU and network rates warm up after their first sample.

Task stdin is closed; interactive commands belong in foreground tools. A task's
process group ends when its leader exits, including any remaining descendants.
Tunnels use SSH BatchMode: establish credentials with a normal SSH connection
first. Unknown historical task ownership disables stopping instead of trusting
a saved PID. Task log viewing is a bounded snapshot; reopen to refresh.
