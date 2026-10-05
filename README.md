# Station

A project-aware terminal workflow hub for Linux, built in Rust with
[Ratatui](https://ratatui.rs/) and themed with Catppuccin Macchiato.
Station shows your workspaces, persistent agent sessions, system metrics,
files, and services in one keyboard-driven dashboard. It never
autostarts anything: every agent session, tunnel, or tool launches only
when you explicitly start it.

Overview has four panes: orbiting planets at the top left, workspaces at the
top right, system information at the bottom left, and services and ports at
the bottom right. Tab cycles the panes; smaller terminals show the focused
pane, including the animation when selected.

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
| State (tunnel logs, history, agent session metadata) | `~/.local/state/station` | `--state-dir PATH` or `$XDG_STATE_HOME` |

A missing config file is fine — sensible defaults are used and nothing is
written to your configuration file. See [`config.example.toml`](config.example.toml) for all
options; `project_roots`/`pinned_projects` accept `~/`, and `[theme]` offers
`accent = "mauve" | "blue"` plus a `compact` mode. Tunnel examples in
that file never run on startup, and the `[editor]` override is optional.

## Keys

| Key | Action |
| --- | --- |
| `1`–`8` | Overview, Workspaces, Agents, Services, Connections, Files, System, Activity |
| `/` | Search |
| `j` / `k`, arrows | Move |
| `Tab` | Switch pane |
| `Enter` | Activate selection |
| `e` | Open editor |
| `t` | Shell in the selected workspace |
| `g` | Git view |
| `h` | Herdr |
| `f` | Files |
| `n` | New session in Agents |
| `F12` | Return from an agent to Station |
| `x` | Close selected agent or stop selected tunnel (confirmation required) |
| `.` | Toggle hidden items |
| `y` | Copy |
| `F5` | Refresh |
| `?` | Help |
| `q` | Quit — while background processes run, choose keep running, stop them, or cancel |

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

Start a configured tunnel from Connections (**5**) or search. Its run appears
in Connections; Enter opens its logs and `x` offers to stop it. Each run has
a durable supervisor that verifies process ownership before stopping it.

The Tasks feature has been removed. Older task configuration and saved
recipes are ignored; existing recipe files and logs remain on disk. Station
still checks for previously started background processes when quitting.

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
and a private tmux server, without invoking a real AI provider. Supervisor tests require permission to create local
Unix sockets. File and subprocess discovery runs in a bounded worker pool.
Git and services refresh about every five seconds; system and background process state refresh
about every second. CPU and network rates warm up after their first sample.

Background stdin is closed; interactive commands belong in foreground tools. A run's
process group ends when its leader exits, including any remaining descendants.
Tunnels use SSH BatchMode: establish credentials with a normal SSH connection
first. Unknown historical process ownership disables stopping instead of trusting
a saved PID. Tunnel log viewing is a bounded snapshot; reopen to refresh.
