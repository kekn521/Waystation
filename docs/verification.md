# Native release verification

The initial release was exercised on this Linux/Hyprland machine using its
Catppuccin Macchiato Kitty theme, without modifying terminal or desktop settings.

- Rust formatting, Clippy with warnings denied, and 66 all-target tests passed.
- Optimized locked release build passed.
- Controlled PTY checks passed for editor, shell, and SSH stand-ins; exit 0,
  exit 7, Ctrl-C, a missing program, and exact terminal attribute restoration.
- Resize checks covered 140×45, 120×38, 100×32, 89×30, 80×24, 60×18, and 59×17.
  Committed cell snapshots cover all nine sections, search, help, confirmation,
  unavailable data, and log errors.
- A task completed after the UI exited. Separate tests cover authenticated stop,
  forged process identity rejection, concurrent managers, and rotated log bounds.
- Fresh HOME/XDG paths and two simultaneous UI instances passed the PTY checks.
- Live inspection found 21 workspaces, read a Git branch, sampled CPU/RAM/disk/
  network, detected the NVIDIA GeForce RTX 5070, and read local listeners.
  No remote SSH destination or existing project recipe was launched.
- A temporary Kitty instance rendered the dashboard and exited through `q`.
  Desktop capture was obscured by the lock screen; the native Ratatui cell
  buffer was separately rendered and visually inspected.

[Live native buffer preview](../artifacts/station-live.png) is generated from
observations on this machine, not demonstration data. Reproduce the SVG with:

```sh
cargo run --example capture -- /tmp/station.svg
```

Optional integration availability depends on executable installation and user
permissions. Docker and GPU failures are represented independently; task
history is retained until the user removes it. No autostart was installed.

## Fresh review and implementation decisions

A separate reviewer inspected process ownership, terminal recovery, providers,
state, and UI routing. All six findings were resolved with failing-then-passing
regression tests, followed by a green full suite and release PTY run.

- Configuration stores command program/argument arrays as UTF-8 strings, then
  converts them to OS strings at execution. Discovered paths retain exact Unix
  bytes in state. The tradeoff is that config command text must be valid UTF-8.
- Foreground actions and async providers were implemented in one integrated
  commit while parsing work was pending. The tradeoff is a larger review unit.

No review findings remain deferred. The tested executable is installed at
`~/.local/bin/station`; it was created without replacing an existing command.
The source branch is `feat/native-station` in `.worktrees/native` and is kept
for further iteration. No repository was pushed or published.

## 0.2.0 — persistent agents and task creation

- All 93 Rust tests pass, including seven refreshed terminal-size snapshots,
  agent/task form rendering and input, literal command parsing, recipe reload,
  and lossless executable paths in non-UTF-8 project directories.
- Formatting, Clippy with warnings denied, and the locked release build pass.
- Both PTY scripts pass on the optimized executable. Local stand-ins exercise
  two named Codex/Claude sessions, F12 detach, quitting and reconnecting,
  an inherited outer TMUX environment, and closing only the selected session.
- The task form saves without execution, then runs from the saved-task list;
  output is logged, recipes reload after restart, and existing TOML stays intact.
- Agent tests cover retained output after process exit and unavailable records
  after loss of the private server. Sessions survive Station exit, not reboot.
- A separate reviewer found one executable-path encoding defect. It was fixed
  with lossless path serialization and a regression using a non-UTF-8 directory.
- The actual native form buffer was rendered and visually inspected in
  [this preview](../artifacts/station-agent-form.png). No real AI provider or
  remote host was contacted by verification.

Use `3`, `n`, and Ctrl+S to create/open an agent; F12 returns to Station.
Use `4`, `a`, and Ctrl+S to save a task, then Enter to run it.
