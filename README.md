# Station

A project-aware terminal workflow hub for Linux, built in Rust with
[Ratatui](https://ratatui.rs/) and themed with Catppuccin Macchiato.
Station shows your workspaces, system metrics, files, and durable background
tasks in one keyboard-driven dashboard. It never autostarts anything: every
task, tunnel, or tool launches only when you explicitly start it.

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
| State (task logs, history) | `~/.local/state/station` | `--state-dir PATH` or `$XDG_STATE_HOME` |

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
| `n` | Recipes |
| `r` | Rerun task |
| `x` | Stop a task (requires confirmation) |
| `.` | Toggle hidden items |
| `y` | Copy |
| `F5` | Refresh |
| `?` | Help |
| `q` | Quit — while tasks run, Station asks: keep running, stop them, or cancel |

## Tasks

Task recipes are explicit `program` + `args` arrays; Station never splits a
shell command line. Each run gets a durable per-run supervisor that owns the
task's process group and listens on a private socket, so stopping is possible
even after Station restarts — and stop requests are ownership-verified before
anything is signaled. Logs roll through four 5 MiB segments per run, the UI
shows a 64 KiB tail, and history is retained.

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
```

The PTY checks use temporary local commands for editor, shell, and SSH handoff;
they never contact a remote host. Task tests require permission to create local
Unix sockets. File and subprocess discovery runs in a bounded worker pool.
Git and services refresh about every five seconds; system and task state refresh
about every second. CPU and network rates warm up after their first sample.

Task stdin is closed; interactive commands belong in foreground tools. A task's
process group ends when its leader exits, including any remaining descendants.
Tunnels use SSH BatchMode: establish credentials with a normal SSH connection
first. Unknown historical task ownership disables stopping instead of trusting
a saved PID. Task log viewing is a bounded snapshot; reopen to refresh.
