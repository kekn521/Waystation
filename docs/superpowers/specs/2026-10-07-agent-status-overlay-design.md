# Floating agent status

Status: direction approved 2026-10-07 (floating box over a Waystation-hosted terminal, chosen
over a tmux status bar after comparing mockups). Section approvals waived by the user.

## Goal

While the terminal is handed to an agent, editor, shell or any tool Waystation opens, a small
box floats in the top-right corner showing every agent session's state and its latest
activity, so the user can see when another agent needs them without leaving what they are
doing.

- States: **running**, **needs input**, **done** (turn finished, waiting for the next prompt),
  **ready** (started, no prompt yet), **exited**, **saved**.
- Latest activity: the command or tool being run, the question being asked, or the start of
  the last reply; one line per agent.
- F9 hides or shows the box; the choice persists while Waystation runs.
- Scope is inside Waystation only. Other desktop apps are untouched.

## Why an embedded terminal

A terminal program cannot draw over another one. Waystation therefore runs the program it
opens in a PTY, emulates the terminal with `alacritty_terminal`, draws the program's screen
with ratatui and draws the box on top. A throwaway spike hosted Helix, bash and Claude Code
this way with correct rendering, input, exit codes and the floating box.

## Host (`src/runtime/host.rs`)

- Spawn the command on a PTY sized to the terminal, in its own session with the PTY as its
  controlling terminal, `TERM=xterm-256color`, `COLORTERM=truecolor`, and `KITTY_*`
  variables removed (the hosted program talks to Waystation's emulator, not kitty).
- Output: a reader thread feeds bytes to the emulator; the screen redraws at most every
  16 ms while content changes.
- Input: raw stdin bytes are forwarded unchanged. To make the outer terminal encode keys and
  mouse the way the program asked, Waystation **mirrors input modes** onto it: cursor keys
  (DECCKM), keypad, bracketed paste, mouse reporting (1000/1002/1003/1005/1006), focus events,
  alternate scroll and the kitty keyboard flags. All mirrored modes are reset on exit.
- Hotkeys taken by Waystation: F9 (legacy `CSI 20 ~` and kitty `CSI 20 ; 1[:event] ~`
  without modifiers; key releases are swallowed too). Shift+PageUp/PageDown scroll the
  emulator's history while the program is on the normal screen; any other key returns to the
  bottom. Everything else reaches the program.
- Emulator events: replies to queries go back to the program; title changes and the bell go to
  the outer terminal; clipboard writes are forwarded as OSC 52; colour queries are answered
  with the outer terminal's own foreground and background (queried once at the first hosted
  launch, falling back to Waystation's palette). Clipboard reads are refused.
- Rendering maps named colours to the outer terminal's ANSI palette (so the user's kitty
  theme still applies), indexed and true colour directly, and bold, italic, dim, underline,
  strikeout, hidden, inverse and wide characters. The cursor position, visibility and shape
  follow the program.
- Resize: the outer size is checked every loop; the emulator and PTY follow.
- Exit: when the child exits (or the PTY closes) the host returns its status and the last
  non-empty screen lines, used for error messages such as a failed `tmux attach`.
- Known gaps: hyperlinks (OSC 8) and terminal images do not pass through.

## Agent activity

`waystation __agent-hook` (already the SessionStart hook) handles more events and writes
`<state>/agents/<id>.activity.json` = `{ "state", "detail", "at" }` atomically, after the
existing checks that the hook comes from the session's own agent.

| Event (Claude / Codex) | State | Detail |
| --- | --- | --- |
| SessionStart | ready | — (also records the conversation id, as today) |
| UserPromptSubmit | running | `› ` + first line of the prompt |
| PreToolUse | running | Bash: `$ ` + command; file tools: `Edit path`; others: tool name |
| PostToolUse / PostToolUseFailure | running | first non-empty output line, or the error |
| PermissionRequest | needs input | `Allow ` + the PreToolUse description `?` |
| Notification (Claude: permission_prompt, elicitation_*, agent_needs_input) | needs input | message |
| Stop | done | first line of `last_assistant_message` |
| Interrupt (Codex) | ready | `Interrupted` |

- Details are one line, control characters removed, at most 200 characters.
- Hooks run synchronously (a few milliseconds) so events cannot land out of order.
- Claude receives all hooks through `--settings`; Codex through one `-c hooks.<Event>=…` per
  event, which Codex asks the user to trust once.
- Exited and saved come from tmux and the session record, as today, and win over activity.

## Overlay

- Built from the agent list (already refreshed every second) plus each activity file.
- Box: rounded border in mauve, title `Agents · F9`, 2 lines per agent (state line, detail
  line), at most 5 agents then `+N more`, about 36 columns wide, placed one row below the top
  and one column from the right edge. Needs-input agents sort first.
- Hidden when there are no agent sessions or the terminal is too small for it.

## Wiring

- Every program Waystation opens in the foreground (editor, shell, Git UI, file browser,
  Herdr, agent attach) runs through the host instead of taking over the terminal directly.
- `waystation __host [--state-dir DIR] -- CMD...` runs the host directly; it is how the
  integration tests drive it through tmux.

## Testing

- Unit: mode-mirroring sequences; F9 detection across legacy and kitty encodings (with and
  without modifiers, press and release); cell style mapping; activity mapping per event;
  overlay row building and ordering.
- Hook binary: each event writes the expected activity; foreign and nested hooks are ignored.
- Host through tmux: output renders, input reaches the program, bracketed paste mode is
  mirrored, F9 shows and hides the box with an agent from a temp state dir, exit status and
  last lines come back, resize follows.
- Manual: Claude Code, Codex, Helix and a shell hosted for real; Claude's hooks drive the box
  through running, needs input and done.

## Order

1. Activity recording in `__agent-hook` and hook registration.
2. Host with `__host`.
3. Overlay and wiring.
4. End-to-end checks, README.
