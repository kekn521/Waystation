# Agent usage: plan limits and token totals

Status: approved direction (2026-10-07); section approvals waived by the user.

## Goal

Show how much of their Claude and Codex plans the user has used, so they can
tell at a glance whether they are about to hit a limit, plus how many tokens
they have spent recently.

- Plan limits for both tools: the 5-hour and weekly windows, percent used and
  reset time.
- Token totals for both tools: the current 5-hour window, today, and the last
  7 days.
- A usage panel at the top of Agents (`3`) and a one-line summary on the
  Overview.

## Constraints

- Local files only; Waystation makes no network calls for usage.
- Figures are "as of" each tool's last reply; Waystation shows that age.
- Nothing runs on launch beyond reading files. Editing the user's Claude
  settings is a confirmed, one-time action, never silent.
- "Tokens" excludes cache reads, which dwarf everything else.

## Data sources

### Codex

Rollouts (`$CODEX_HOME/sessions/YYYY/MM/DD/rollout-*.jsonl`) contain
`event_msg` lines whose `payload.type` is `token_count`:

- `payload.rate_limits.primary` / `.secondary`: `used_percent`,
  `window_minutes`, `resets_at` (epoch seconds); `payload.rate_limits.plan_type`.
- `payload.info.total_token_usage`: running totals per rollout
  (`input_tokens`, `cached_input_tokens`, `output_tokens`, ...).

Limits are the `rate_limits` of the newest `token_count` event (by line
`timestamp`) across rollouts. The window with `window_minutes == 300` is the
5-hour window and the longer one is weekly; Waystation matches by
`window_minutes`, not by `primary`/`secondary`.

Tokens: each event contributes the increase in `total_token_usage` since the
previous event in the same file, so repeated events add nothing. If the total
drops, that event contributes its `last_token_usage`. Token count is
`input_tokens - cached_input_tokens + output_tokens`.

### Claude

Claude Code passes documented `rate_limits` to the status line command
(code.claude.com/docs/en/statusline): `five_hour` and `seven_day`, each with
`used_percentage` and `resets_at` (epoch seconds), present only for Pro/Max
after the first API response, and each window may be absent.

Waystation installs `waystation __statusline` as `statusLine` in
`~/.claude/settings.json` (confirmed by the user). It:

- reads the status line JSON from stdin;
- if `rate_limits` is present, atomically writes
  `{ "saved_at": <epoch secs>, "rate_limits": <as received> }` to
  `<state>/usage/claude.json`;
- prints `5h 23% · wk 41%` (with a trailing `!` once any window reaches 90%),
  or nothing when there are no limits;
- always exits 0 and never writes to stderr.

Tokens: every `*.jsonl` under `$CLAUDE_CONFIG_DIR/projects` (subagent
transcripts included), lines with `type: "assistant"` and `message.usage`.
Streaming repeats a message, so entries are deduplicated across files by
`(message.id, requestId)`, keeping the last copy; lines without both ids count
once. Token count is `input_tokens + cache_creation_input_tokens +
output_tokens`, timed by the line's `timestamp`.

## Windows

- 5-hour tokens: from `resets_at - window` to now while that tool's 5-hour
  limit window is still running; otherwise the last 5 hours.
- Today: since local midnight. 7 days: the last 7×24 hours.
- A limit window whose `resets_at` has passed shows as `reset`, not a stale
  percentage.

## Components

- `src/usage.rs`
  - `LimitWindow { used_percent, window_minutes, resets_at }`,
    `Limits { five_hour, weekly, plan, updated_at }`,
    `TokenTotals { five_hour, today, week }`, `ToolUsage`, `Usage`.
  - `Scanner`: incremental transcript reader. Per file it keeps the byte
    offset read so far, reads only appended bytes up to the last newline,
    rescans a file that shrank, ignores files not modified in 7 days, and drops
    entries older than 7 days.
  - Pure functions for parsing one Claude/Codex line and for totals over a
    given `now`, so they are unit-testable without the clock.
  - `statusline(stdin, state) -> String` and Claude `statusLine`
    detect/install (`Installed`, `Missing`, `Stale`, `Other(command)`), editing
    only the `statusLine` key and preserving key order; refuses to touch an
    unparseable file.
- `ProviderId::Usage` / `ProviderPayload::Usage`: owns a `Mutex<Scanner>`,
  refreshed every 30 s. The first scan runs on a worker thread.
- UI: panel above the Agents list (3 lines per tool at 16+ rows, 1 line per
  tool when shorter, hidden when very short; bars teal / peach from 70% / red
  from 90%); a status line install row with confirmation in the Agents list;
  an `AI` line at the bottom of the Overview's Machine pulse pane when a row
  is free. Tools that are not installed are omitted.
- `main.rs`: `__statusline` private subcommand.
- Dependency: `chrono` for local time (today boundary, reset times).

## Errors

- Provider failure: the panel shows the error with "F5 retries".
- Unparseable lines and unreadable files are skipped.
- Unparseable `settings.json`: install refuses with a message, file untouched.
- A different existing `statusLine` is never replaced; the row explains why.

## Testing

- Parsing: Claude dedupe (same id/request repeated, last wins; missing ids
  count once), Codex deltas (repeats add nothing, decreases fall back to
  `last_token_usage`), cache reads excluded.
- Windows: entries just inside/outside 5h, today, 7d for a fixed `now`;
  5-hour window anchored to `resets_at`; expired limits shown as reset.
- Scanner: appends picked up incrementally, partial last line deferred,
  truncated file rescanned, old files ignored.
- Codex limits: newest event wins across files; windows matched by
  `window_minutes`.
- `__statusline` binary: saves and prints with limits, prints nothing without,
  exits 0 on garbage, no stderr.
- Status line install: create when missing, preserve other keys and order,
  idempotent, repoint stale path, refuse foreign `statusLine`, refuse
  unparseable file.
- UI: panel rendering (full/compact/hidden), install row and confirmation
  flow; reviewed snapshot updates.
- Manual: real `~/.claude` and `~/.codex` totals cross-checked with an
  independent script; real `claude` with the status line saving limits.
