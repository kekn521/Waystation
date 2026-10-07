//! Claude and Codex plan limits and token totals, read from files the agents already write.
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    fs::File,
    io::{BufRead, BufReader, Seek, SeekFrom},
    path::{Path, PathBuf},
    time::{Duration, SystemTime},
};

const HOUR: i64 = 3600;
const WEEK: i64 = 7 * 24 * HOUR;

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct LimitWindow {
    pub used_percent: f64,
    pub window_minutes: u64,
    /// Epoch seconds.
    pub resets_at: i64,
}
impl LimitWindow {
    /// The percentage of the window still available, or `None` once it has reset since it
    /// was reported.
    pub fn remaining(&self, now: i64) -> Option<f64> {
        (self.resets_at > now).then_some((100. - self.used_percent).clamp(0., 100.))
    }
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Limits {
    pub five_hour: Option<LimitWindow>,
    pub weekly: Option<LimitWindow>,
    pub plan: Option<String>,
    /// When the agent reported these, in epoch seconds.
    pub updated_at: i64,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TokenTotals {
    pub five_hour: u64,
    pub today: u64,
    pub week: u64,
}
#[derive(Clone, Debug, PartialEq)]
pub struct ToolUsage {
    pub limits: Option<Limits>,
    pub tokens: TokenTotals,
}
/// Usage for each installed tool; `None` for a tool that is not installed.
#[derive(Clone, Debug, PartialEq)]
pub struct Usage {
    pub now: i64,
    pub claude: Option<ToolUsage>,
    pub codex: Option<ToolUsage>,
    pub statusline: StatusLine,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tool {
    Claude,
    Codex,
}

/// Where each token total starts, in epoch seconds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Windows {
    pub five_hour_start: i64,
    pub today_start: i64,
    pub week_start: i64,
}
impl Windows {
    /// The 5-hour total follows the tool's own 5-hour limit window while it runs.
    pub fn new(now: i64, today_start: i64, five_hour: Option<&LimitWindow>) -> Self {
        let five_hour_start = match five_hour {
            Some(w) if w.resets_at > now => w.resets_at - w.window_minutes as i64 * 60,
            _ => now - 5 * HOUR,
        };
        Self {
            five_hour_start,
            today_start,
            week_start: now - WEEK,
        }
    }
}
/// Local midnight before `now`.
pub fn local_today_start(now: i64) -> i64 {
    use chrono::{Local, TimeZone};
    Local
        .timestamp_opt(now, 0)
        .single()
        .and_then(|t| t.date_naive().and_hms_opt(0, 0, 0))
        .and_then(|midnight| Local.from_local_datetime(&midnight).earliest())
        .map_or(now - 24 * HOUR, |t| t.timestamp())
}
pub fn now() -> i64 {
    SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() as i64)
}

#[derive(Default)]
struct FileState {
    offset: u64,
    /// Codex: the running token total at the last event.
    running: Option<u64>,
    /// Codex: this file's token events, replaced if the file is rewritten.
    entries: Vec<(i64, u64)>,
    /// Codex: the newest rate limits reported in this file.
    limits: Option<Limits>,
}
/// Reads agent transcripts incrementally, keeping a week of token entries.
pub struct Scanner {
    claude_home: PathBuf,
    codex_home: PathBuf,
    claude_files: HashMap<PathBuf, FileState>,
    codex_files: HashMap<PathBuf, FileState>,
    /// Claude messages by `(message id, request id)` (or by position when ids are missing);
    /// streaming writes each message several times and the last copy is final.
    claude: HashMap<String, (i64, u64)>,
}
impl Scanner {
    pub fn new(claude_home: PathBuf, codex_home: PathBuf) -> Self {
        Self {
            claude_home,
            codex_home,
            claude_files: HashMap::new(),
            codex_files: HashMap::new(),
            claude: HashMap::new(),
        }
    }
    pub fn scan(&mut self, now: i64) {
        let cutoff = now - WEEK;
        let since = SystemTime::UNIX_EPOCH + Duration::from_secs(cutoff.max(0) as u64);
        let claude = recent_jsonl(&self.claude_home.join("projects"), since);
        self.claude_files.retain(|path, _| claude.contains(path));
        for path in claude {
            let state = self.claude_files.entry(path.clone()).or_default();
            let claude = &mut self.claude;
            read_lines(&path, state, |_, offset, line| {
                if let Some((key, entry)) = claude_entry(line) {
                    let key = key.unwrap_or_else(|| format!("{}\0{offset}", path.display()));
                    claude.insert(key, entry);
                }
            });
        }
        self.claude.retain(|_, (t, _)| *t >= cutoff);
        let codex = recent_jsonl(&self.codex_home.join("sessions"), since);
        self.codex_files.retain(|path, _| codex.contains(path));
        for path in codex {
            let state = self.codex_files.entry(path.clone()).or_default();
            read_lines(&path, state, |state, _, line| codex_event(state, line));
            state.entries.retain(|(t, _)| *t >= cutoff);
        }
    }
    pub fn tokens(&self, tool: Tool, windows: &Windows) -> TokenTotals {
        let mut totals = TokenTotals::default();
        let mut add = |t: i64, tokens: u64| {
            if t >= windows.week_start {
                totals.week += tokens;
            }
            if t >= windows.today_start {
                totals.today += tokens;
            }
            if t >= windows.five_hour_start {
                totals.five_hour += tokens;
            }
        };
        match tool {
            Tool::Claude => self.claude.values().for_each(|&(t, n)| add(t, n)),
            Tool::Codex => self
                .codex_files
                .values()
                .flat_map(|f| &f.entries)
                .for_each(|&(t, n)| add(t, n)),
        }
        totals
    }
    pub fn codex_limits(&self) -> Option<Limits> {
        self.codex_files
            .values()
            .filter_map(|f| f.limits.as_ref())
            .max_by_key(|l| l.updated_at)
            .cloned()
    }
}

/// `*.jsonl` files under `root` modified since `since`.
fn recent_jsonl(root: &Path, since: SystemTime) -> Vec<PathBuf> {
    let mut found = vec![];
    let mut stack = vec![(root.to_path_buf(), 0)];
    while let Some((dir, depth)) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let Ok(meta) = entry.metadata() else {
                continue;
            };
            if meta.is_dir() {
                if depth < 6 {
                    stack.push((path, depth + 1));
                }
            } else if path.extension().is_some_and(|e| e == "jsonl")
                && meta.modified().is_ok_and(|m| m >= since)
            {
                found.push(path);
            }
        }
    }
    found
}

/// Feeds each complete line appended since the last read to `f` with its byte offset.
///
/// A trailing line without its newline yet is left for the next read; a file that shrank
/// was rewritten and is read again from the start.
fn read_lines(path: &Path, state: &mut FileState, mut f: impl FnMut(&mut FileState, u64, &[u8])) {
    let Ok(mut file) = File::open(path) else {
        return;
    };
    let Ok(len) = file.metadata().map(|m| m.len()) else {
        return;
    };
    if len < state.offset {
        *state = FileState::default();
    }
    if len == state.offset || file.seek(SeekFrom::Start(state.offset)).is_err() {
        return;
    }
    let mut reader = BufReader::with_capacity(256 * 1024, file);
    let mut line = vec![];
    loop {
        line.clear();
        match reader.read_until(b'\n', &mut line) {
            // Nothing more, or a line still being written.
            Ok(0) | Err(_) => break,
            Ok(_) if line.last() != Some(&b'\n') => break,
            Ok(n) => {
                let offset = state.offset;
                state.offset += n as u64;
                f(state, offset, &line);
            }
        }
    }
}

fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    haystack.windows(needle.len()).any(|w| w == needle)
}
fn timestamp(value: &serde_json::Value) -> Option<i64> {
    let text = value["timestamp"].as_str()?;
    Some(chrono::DateTime::parse_from_rfc3339(text).ok()?.timestamp())
}
fn count(value: &serde_json::Value, key: &str) -> u64 {
    value[key].as_u64().unwrap_or(0)
}

/// A Claude assistant message's dedupe key (when it has ids), time and token count.
fn claude_entry(line: &[u8]) -> Option<(Option<String>, (i64, u64))> {
    if !contains(line, b"\"usage\"") {
        return None;
    }
    let value: serde_json::Value = serde_json::from_slice(line).ok()?;
    if value["type"] != "assistant" {
        return None;
    }
    let usage = value["message"].get("usage")?;
    let tokens = count(usage, "input_tokens")
        + count(usage, "cache_creation_input_tokens")
        + count(usage, "output_tokens");
    let key = match (value["message"]["id"].as_str(), value["requestId"].as_str()) {
        (Some(message), Some(request)) => Some(format!("{message}\0{request}")),
        _ => None,
    };
    Some((key, (timestamp(&value)?, tokens)))
}

/// Codex counts fresh input and output; cached input is excluded like Claude's cache reads.
fn codex_tokens(usage: &serde_json::Value) -> u64 {
    count(usage, "input_tokens").saturating_sub(count(usage, "cached_input_tokens"))
        + count(usage, "output_tokens")
}
fn codex_event(state: &mut FileState, line: &[u8]) {
    if !contains(line, b"token_count") {
        return;
    }
    let Ok(value) = serde_json::from_slice::<serde_json::Value>(line) else {
        return;
    };
    let payload = &value["payload"];
    if payload["type"] != "token_count" {
        return;
    }
    let Some(t) = timestamp(&value) else {
        return;
    };
    if let Some(info) = payload.get("info").filter(|i| i.is_object()) {
        let running = codex_tokens(&info["total_token_usage"]);
        let added = match state.running {
            Some(previous) if running >= previous => running - previous,
            // Totals reset (say after compaction): count this event's own usage.
            Some(_) => codex_tokens(&info["last_token_usage"]),
            None => running,
        };
        state.running = Some(running);
        if added > 0 {
            state.entries.push((t, added));
        }
    }
    if let Some(limits) = codex_limits(&payload["rate_limits"], t)
        && state.limits.as_ref().is_none_or(|l| l.updated_at <= t)
    {
        state.limits = Some(limits);
    }
}
fn codex_limits(value: &serde_json::Value, t: i64) -> Option<Limits> {
    let windows = ["primary", "secondary"]
        .iter()
        .filter_map(|slot| {
            let w = &value[*slot];
            Some(LimitWindow {
                used_percent: w["used_percent"].as_f64()?,
                window_minutes: w["window_minutes"].as_u64()?,
                resets_at: w["resets_at"].as_i64()?,
            })
        })
        .collect::<Vec<_>>();
    if windows.is_empty() {
        return None;
    }
    // Matched by length: Codex has moved windows between its slots.
    Some(Limits {
        five_hour: windows
            .iter()
            .filter(|w| w.window_minutes <= 6 * 60)
            .min_by_key(|w| w.window_minutes)
            .copied(),
        weekly: windows
            .iter()
            .filter(|w| w.window_minutes >= 24 * 60)
            .max_by_key(|w| w.window_minutes)
            .copied(),
        plan: value["plan_type"].as_str().map(String::from),
        updated_at: t,
    })
}

fn claude_limits_path(state: &Path) -> PathBuf {
    state.join("usage/claude.json")
}
#[derive(Serialize, Deserialize)]
struct SavedClaudeLimits {
    saved_at: i64,
    rate_limits: serde_json::Value,
}
fn claude_window(value: &serde_json::Value, minutes: u64) -> Option<LimitWindow> {
    Some(LimitWindow {
        used_percent: value["used_percentage"].as_f64()?,
        window_minutes: minutes,
        resets_at: value["resets_at"].as_i64()?,
    })
}
fn parse_claude_limits(rate_limits: &serde_json::Value, saved_at: i64) -> Option<Limits> {
    let five_hour = claude_window(&rate_limits["five_hour"], 5 * 60);
    let weekly = claude_window(&rate_limits["seven_day"], 7 * 24 * 60);
    (five_hour.is_some() || weekly.is_some()).then_some(Limits {
        five_hour,
        weekly,
        plan: None,
        updated_at: saved_at,
    })
}
/// Claude's limits as last saved by `__statusline`.
pub fn claude_limits(state: &Path) -> Option<Limits> {
    let saved: SavedClaudeLimits =
        serde_json::from_slice(&std::fs::read(claude_limits_path(state)).ok()?).ok()?;
    parse_claude_limits(&saved.rate_limits, saved.saved_at)
}

/// Handles Claude's status line command: saves its documented `rate_limits` for the usage
/// panel. It prints nothing, so Claude Code shows no status line of its own.
pub fn statusline(input: &[u8], state: &Path, now: i64) {
    let Ok(input) = serde_json::from_slice::<serde_json::Value>(input) else {
        return;
    };
    let rate_limits = &input["rate_limits"];
    if parse_claude_limits(rate_limits, now).is_some() {
        let _ = crate::store::atomic_json(
            &claude_limits_path(state),
            &SavedClaudeLimits {
                saved_at: now,
                rate_limits: rate_limits.clone(),
            },
        );
    }
}

/// Whether Claude's `statusLine` setting runs Waystation's `__statusline`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum StatusLine {
    Installed,
    Missing,
    /// Waystation's, for a different executable or state directory.
    Stale,
    /// The user's own status line script, which passes Claude's input on to Waystation.
    Chained,
    /// Someone else's status line, which Waystation never replaces.
    Other(String),
}
const STATUSLINE_ARG: &str = "__statusline";
fn statusline_command(launcher: &Path, state: &Path) -> Result<String> {
    use crate::agents::shell_quote;
    Ok(format!(
        "{} {STATUSLINE_ARG} {}",
        shell_quote(launcher)?,
        shell_quote(state)?
    ))
}
fn claude_settings(claude_home: &Path) -> Result<serde_json::Value> {
    match std::fs::read(claude_home.join("settings.json")) {
        Ok(bytes) => serde_json::from_slice(&bytes).context("Reading Claude settings.json"),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(serde_json::json!({})),
        Err(e) => Err(e.into()),
    }
}
fn status_of(settings: &serde_json::Value, expected: &str) -> StatusLine {
    let Some(current) = settings.get("statusLine").filter(|s| !s.is_null()) else {
        return StatusLine::Missing;
    };
    match current["command"].as_str() {
        Some(command) if command == expected => StatusLine::Installed,
        Some(command) if command.contains(&format!(" {STATUSLINE_ARG} ")) => StatusLine::Stale,
        Some(command) if runs_waystation_script(command) => StatusLine::Chained,
        Some(command) => StatusLine::Other(command.into()),
        None => StatusLine::Other(current.to_string()),
    }
}
/// Whether `command` runs a script file that itself calls `__statusline`.
fn runs_waystation_script(command: &str) -> bool {
    let home = std::env::var_os("HOME").map(PathBuf::from);
    command.split_whitespace().any(|word| {
        let word = word.trim_matches(|c| c == '\'' || c == '"');
        let path = match (word.strip_prefix("~/"), &home) {
            (Some(rest), Some(home)) => home.join(rest),
            _ => PathBuf::from(word),
        };
        path.is_absolute()
            && std::fs::metadata(&path).is_ok_and(|m| m.is_file() && m.len() < 1024 * 1024)
            && std::fs::read(&path).is_ok_and(|script| {
                script
                    .windows(STATUSLINE_ARG.len() + 1)
                    .any(|w| w == format!(" {STATUSLINE_ARG}").as_bytes())
            })
    })
}
pub fn statusline_status(claude_home: &Path, launcher: &Path, state: &Path) -> StatusLine {
    match (
        claude_settings(claude_home),
        statusline_command(launcher, state),
    ) {
        (Ok(settings), Ok(expected)) => status_of(&settings, &expected),
        _ => StatusLine::Missing,
    }
}
/// Points Claude's `statusLine` at Waystation, changing no other setting.
pub fn install_statusline(claude_home: &Path, launcher: &Path, state: &Path) -> Result<()> {
    let mut settings = claude_settings(claude_home)?;
    let command = statusline_command(launcher, state)?;
    match status_of(&settings, &command) {
        StatusLine::Other(existing) => anyhow::bail!(
            "Claude already has a status line ({existing}); Waystation won't replace it"
        ),
        StatusLine::Chained => {
            anyhow::bail!("Claude's status line already passes its limits to Waystation")
        }
        StatusLine::Installed | StatusLine::Missing | StatusLine::Stale => {}
    }
    settings
        .as_object_mut()
        .context("Claude settings.json is not a JSON object")?
        .insert(
            "statusLine".into(),
            serde_json::json!({"type": "command", "command": command, "padding": 0}),
        );
    crate::store::replace_user_json(&claude_home.join("settings.json"), &settings)
}

/// Collects usage for the tools that are installed.
pub fn collect(
    scanner: &mut Scanner,
    state: &Path,
    claude: bool,
    codex: bool,
    statusline: StatusLine,
    now: i64,
) -> Usage {
    scanner.scan(now);
    let today = local_today_start(now);
    let tool = |limits: Option<Limits>, tool: Tool| {
        let windows = Windows::new(
            now,
            today,
            limits.as_ref().and_then(|l| l.five_hour.as_ref()),
        );
        ToolUsage {
            tokens: scanner.tokens(tool, &windows),
            limits,
        }
    };
    Usage {
        now,
        claude: claude.then(|| tool(claude_limits(state), Tool::Claude)),
        codex: codex.then(|| tool(scanner.codex_limits(), Tool::Codex)),
        statusline,
    }
}

/// A token count in at most four characters or so: `950`, `41k`, `1.2M`.
pub fn format_tokens(n: u64) -> String {
    match n {
        0..1_000 => n.to_string(),
        1_000..1_000_000 => format!("{}k", n / 1_000),
        1_000_000..1_000_000_000 => format!("{:.1}M", n as f64 / 1e6),
        _ => format!("{:.1}B", n as f64 / 1e9),
    }
}

/// What is left: `77%`, `reset` once the window has rolled over, or `—` when the tool
/// reported none.
pub fn remaining_percent(window: Option<&LimitWindow>, now: i64) -> String {
    match window {
        Some(w) => w
            .remaining(now)
            .map_or("reset".into(), |p| format!("{p:.0}%")),
        None => "—".into(),
    }
}
/// `77% left`, `reset` or `—`.
pub fn remaining_label(window: Option<&LimitWindow>, now: i64) -> String {
    match window.and_then(|w| w.remaining(now)) {
        Some(_) => format!("{} left", remaining_percent(window, now)),
        None => remaining_percent(window, now),
    }
}
/// `<1m`, `4m`, `3h`, `2d`.
pub fn format_age(seconds: i64) -> String {
    match seconds.max(0) {
        s if s < 60 => "<1m".into(),
        s if s < HOUR => format!("{}m", s / 60),
        s if s < 24 * HOUR => format!("{}h", s / HOUR),
        s => format!("{}d", s / (24 * HOUR)),
    }
}
/// Local reset time: `14:20` within a day, else `Fri 09:00`.
pub fn format_reset(resets_at: i64, now: i64) -> String {
    use chrono::{Local, TimeZone};
    let Some(at) = Local.timestamp_opt(resets_at, 0).single() else {
        return String::new();
    };
    if resets_at - now < 24 * HOUR {
        at.format("%H:%M").to_string()
    } else {
        at.format("%a %H:%M").to_string()
    }
}
/// What is left per tool with limits, for the Overview: `("Claude", "5h 77% wk 59%")`.
pub fn summary(usage: &Usage) -> Vec<(&'static str, String)> {
    [("Claude", &usage.claude), ("Codex", &usage.codex)]
        .into_iter()
        .filter_map(|(name, tool)| {
            let limits = tool.as_ref()?.limits.as_ref()?;
            Some((
                name,
                format!(
                    "5h {} wk {}",
                    remaining_percent(limits.five_hour.as_ref(), usage.now),
                    remaining_percent(limits.weekly.as_ref(), usage.now)
                ),
            ))
        })
        .collect()
}
