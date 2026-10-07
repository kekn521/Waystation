use std::{
    io::Write,
    path::{Path, PathBuf},
    time::{Duration, SystemTime},
};
use waystation::usage::{
    LimitWindow, Scanner, StatusLine, Tool, Windows, claude_limits, format_tokens,
    install_statusline, statusline, statusline_status,
};

const NOW: i64 = 1_791_400_000;
const HOUR: i64 = 3600;

fn iso(t: i64) -> String {
    chrono::DateTime::from_timestamp(t, 0)
        .unwrap()
        .to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}

fn append(path: &Path, text: &str) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .unwrap()
        .write_all(text.as_bytes())
        .unwrap();
}

fn claude_line(t: i64, ids: Option<(&str, &str)>, input: u64, created: u64, output: u64) -> String {
    let mut line = serde_json::json!({
        "type": "assistant",
        "timestamp": iso(t),
        "message": {
            "model": "claude-opus-5-5",
            "usage": {
                "input_tokens": input,
                "cache_creation_input_tokens": created,
                "cache_read_input_tokens": 1_000_000,
                "output_tokens": output,
            }
        }
    });
    if let Some((message, request)) = ids {
        line["message"]["id"] = message.into();
        line["requestId"] = request.into();
    }
    format!("{line}\n")
}

fn codex_line(
    t: i64,
    total: (u64, u64, u64),
    last: (u64, u64, u64),
    limits: serde_json::Value,
) -> String {
    let usage = |(input, cached, output): (u64, u64, u64)| {
        serde_json::json!({
            "input_tokens": input,
            "cached_input_tokens": cached,
            "output_tokens": output,
            "total_tokens": input + output,
        })
    };
    let line = serde_json::json!({
        "timestamp": iso(t),
        "type": "event_msg",
        "payload": {
            "type": "token_count",
            "info": {"total_token_usage": usage(total), "last_token_usage": usage(last)},
            "rate_limits": limits,
        }
    });
    format!("{line}\n")
}

fn window(percent: f64, minutes: u64, resets_at: i64) -> serde_json::Value {
    serde_json::json!({"used_percent": percent, "window_minutes": minutes, "resets_at": resets_at})
}

struct Homes {
    _dir: tempfile::TempDir,
    claude: PathBuf,
    codex: PathBuf,
}

fn homes() -> Homes {
    let dir = tempfile::tempdir().unwrap();
    Homes {
        claude: dir.path().join("claude"),
        codex: dir.path().join("codex"),
        _dir: dir,
    }
}

/// Windows that include everything from the last 7 days.
fn everything() -> Windows {
    Windows::new(NOW, NOW - 7 * 24 * HOUR, None)
}

#[test]
fn claude_tokens_dedupe_streamed_messages_and_skip_cache_reads() {
    let h = homes();
    let a = h.claude.join("projects/-work/a.jsonl");
    append(&a, &claude_line(NOW - HOUR, Some(("m1", "r1")), 10, 100, 1));
    append(
        &a,
        &claude_line(NOW - HOUR, Some(("m1", "r1")), 10, 100, 50),
    );
    append(&a, &claude_line(NOW - HOUR, None, 1, 0, 1));
    append(
        &a,
        "{\"type\":\"user\",\"message\":{\"content\":\"hi\"}}\nnot json\n",
    );
    // A subagent transcript repeating the parent's message must not count twice.
    append(
        &h.claude.join("projects/-work/a/subagents/b.jsonl"),
        &claude_line(NOW - HOUR, Some(("m1", "r1")), 10, 100, 50),
    );
    let mut scanner = Scanner::new(h.claude.clone(), h.codex.clone());
    scanner.scan(NOW);
    assert_eq!(scanner.tokens(Tool::Claude, &everything()).week, 160 + 2);
}

#[test]
fn token_windows_split_five_hours_today_and_week() {
    let h = homes();
    let a = h.claude.join("projects/-work/a.jsonl");
    append(&a, &claude_line(NOW - HOUR, None, 1, 0, 0));
    append(&a, &claude_line(NOW - 6 * HOUR, None, 10, 0, 0));
    append(&a, &claude_line(NOW - 25 * HOUR, None, 100, 0, 0));
    append(&a, &claude_line(NOW - 8 * 24 * HOUR, None, 1000, 0, 0));
    let mut scanner = Scanner::new(h.claude.clone(), h.codex.clone());
    scanner.scan(NOW);

    let rolling = scanner.tokens(Tool::Claude, &Windows::new(NOW, NOW - 10 * HOUR, None));
    assert_eq!(
        (rolling.five_hour, rolling.today, rolling.week),
        (1, 11, 111)
    );

    // While a 5-hour limit window runs, the 5-hour total starts where that window began.
    let limit = LimitWindow {
        used_percent: 10.,
        window_minutes: 300,
        resets_at: NOW + 4 * HOUR + 1800,
    };
    let anchored = Windows::new(NOW, NOW - 10 * HOUR, Some(&limit));
    assert_eq!(anchored.five_hour_start, NOW - 1800);
    assert_eq!(scanner.tokens(Tool::Claude, &anchored).five_hour, 0);
    let expired = LimitWindow {
        resets_at: NOW - 60,
        ..limit
    };
    assert_eq!(
        Windows::new(NOW, NOW - 10 * HOUR, Some(&expired)).five_hour_start,
        NOW - 5 * HOUR
    );
}

#[test]
fn codex_tokens_count_increases_of_running_totals() {
    let h = homes();
    let rollout = h.codex.join("sessions/2026/10/07/rollout-a.jsonl");
    let none = serde_json::Value::Null;
    append(
        &rollout,
        &codex_line(NOW - HOUR, (100, 40, 10), (100, 40, 10), none.clone()),
    );
    // A repeated event adds nothing.
    append(
        &rollout,
        &codex_line(NOW - HOUR, (100, 40, 10), (100, 40, 10), none.clone()),
    );
    append(
        &rollout,
        &codex_line(NOW - HOUR, (300, 100, 30), (200, 60, 20), none.clone()),
    );
    // Totals that drop (say after compaction) fall back to the event's own usage.
    append(
        &rollout,
        &codex_line(NOW - HOUR, (50, 0, 5), (50, 0, 5), none),
    );
    let mut scanner = Scanner::new(h.claude.clone(), h.codex.clone());
    scanner.scan(NOW);
    assert_eq!(
        scanner.tokens(Tool::Codex, &everything()).week,
        70 + 160 + 55
    );
}

#[test]
fn codex_limits_come_from_the_newest_event_matched_by_window_length() {
    let h = homes();
    let older = h.codex.join("sessions/2026/10/06/rollout-old.jsonl");
    let newer = h.codex.join("sessions/2026/10/07/rollout-new.jsonl");
    let limits = |five: f64, week: f64| {
        serde_json::json!({
            // Matched by length, not by which slot Codex used.
            "primary": window(week, 10080, NOW + 5 * 24 * HOUR),
            "secondary": window(five, 300, NOW + HOUR),
            "plan_type": "plus",
        })
    };
    append(
        &newer,
        &codex_line(NOW - 60, (1, 0, 0), (1, 0, 0), limits(50., 3.)),
    );
    append(
        &older,
        &codex_line(NOW - HOUR, (1, 0, 0), (1, 0, 0), limits(21., 2.)),
    );
    let mut scanner = Scanner::new(h.claude.clone(), h.codex.clone());
    scanner.scan(NOW);
    let limits = scanner.codex_limits().expect("limits");
    assert_eq!(limits.five_hour.unwrap().used_percent, 50.);
    assert_eq!(limits.weekly.unwrap().used_percent, 3.);
    assert_eq!(limits.plan.as_deref(), Some("plus"));
    assert_eq!(limits.updated_at, NOW - 60);
}

#[test]
fn scanner_reads_appends_incrementally_and_rescans_truncated_files() {
    let h = homes();
    let rollout = h.codex.join("sessions/2026/10/07/rollout-a.jsonl");
    let none = serde_json::Value::Null;
    let mut scanner = Scanner::new(h.claude.clone(), h.codex.clone());
    let week = |s: &Scanner| s.tokens(Tool::Codex, &everything()).week;

    let line = codex_line(NOW - HOUR, (100, 0, 0), (100, 0, 0), none.clone());
    let (head, tail) = line.split_at(line.len() / 2);
    append(&rollout, head);
    scanner.scan(NOW);
    assert_eq!(week(&scanner), 0, "a half-written line waits");
    append(&rollout, tail);
    scanner.scan(NOW);
    assert_eq!(week(&scanner), 100);
    scanner.scan(NOW);
    assert_eq!(week(&scanner), 100, "nothing new, nothing added");
    append(
        &rollout,
        &codex_line(NOW - HOUR, (150, 0, 0), (50, 0, 0), none.clone()),
    );
    scanner.scan(NOW);
    assert_eq!(week(&scanner), 150);

    std::fs::write(&rollout, codex_line(NOW - HOUR, (7, 0, 0), (7, 0, 0), none)).unwrap();
    scanner.scan(NOW);
    assert_eq!(week(&scanner), 7);
}

#[test]
fn scanner_ignores_files_untouched_for_a_week() {
    let h = homes();
    let a = h.claude.join("projects/-work/a.jsonl");
    append(&a, &claude_line(NOW - HOUR, None, 5, 0, 0));
    let old = SystemTime::now() - Duration::from_secs(8 * 24 * 3600);
    std::fs::File::options()
        .write(true)
        .open(&a)
        .unwrap()
        .set_modified(old)
        .unwrap();
    let now = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64;
    let mut scanner = Scanner::new(h.claude.clone(), h.codex.clone());
    scanner.scan(now);
    assert_eq!(
        scanner
            .tokens(Tool::Claude, &Windows::new(now, now - HOUR, None))
            .week,
        0
    );
}

#[test]
fn statusline_saves_claude_limits() {
    let d = tempfile::tempdir().unwrap();
    let input = serde_json::json!({
        "model": {"display_name": "Opus"},
        "rate_limits": {
            "five_hour": {"used_percentage": 23.4, "resets_at": NOW + HOUR},
            "seven_day": {"used_percentage": 91.0, "resets_at": NOW + 3 * 24 * HOUR},
        }
    });
    statusline(input.to_string().as_bytes(), d.path(), NOW);
    let limits = claude_limits(d.path()).expect("saved");
    assert_eq!(limits.updated_at, NOW);
    let five = limits.five_hour.unwrap();
    assert_eq!(
        (five.used_percent, five.window_minutes, five.resets_at),
        (23.4, 300, NOW + HOUR)
    );
    assert_eq!(limits.weekly.unwrap().window_minutes, 7 * 24 * 60);

    // Without limits (API billing, or before the first reply) the last figures stay.
    statusline(br#"{"model":{}}"#, d.path(), NOW + 60);
    statusline(b"garbage", d.path(), NOW + 60);
    assert_eq!(claude_limits(d.path()).unwrap().updated_at, NOW);
}

#[test]
fn statusline_command_is_silent_and_always_succeeds() {
    let d = tempfile::tempdir().unwrap();
    let run = |input: &str| {
        let mut child = std::process::Command::new(env!("CARGO_BIN_EXE_waystation"))
            .arg("__statusline")
            .arg(d.path())
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .unwrap();
        child
            .stdin
            .take()
            .unwrap()
            .write_all(input.as_bytes())
            .unwrap();
        child.wait_with_output().unwrap()
    };
    // Claude Code shows no status line when the command prints nothing.
    let out = run(r#"{"rate_limits":{"five_hour":{"used_percentage":5,"resets_at":9999999999}}}"#);
    assert!(
        out.status.success() && out.stderr.is_empty() && out.stdout.is_empty(),
        "{out:?}"
    );
    assert_eq!(
        claude_limits(d.path())
            .unwrap()
            .five_hour
            .unwrap()
            .used_percent,
        5.
    );
    let out = run("not json");
    assert!(
        out.status.success() && out.stderr.is_empty() && out.stdout.is_empty(),
        "{out:?}"
    );
}

#[test]
fn statusline_install_edits_only_its_own_key() {
    let d = tempfile::tempdir().unwrap();
    let claude_home = d.path().join("claude");
    let settings = claude_home.join("settings.json");
    let state = Path::new("/home/me/.local/state/way station");
    let launcher = Path::new("/opt/waystation");
    let ours = "'/opt/waystation' __statusline '/home/me/.local/state/way station'";

    assert_eq!(
        statusline_status(&claude_home, launcher, state),
        StatusLine::Missing
    );
    install_statusline(&claude_home, launcher, state).unwrap();
    assert_eq!(
        statusline_status(&claude_home, launcher, state),
        StatusLine::Installed
    );
    let value: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&settings).unwrap()).unwrap();
    assert_eq!(value["statusLine"]["command"], ours);
    assert_eq!(value["statusLine"]["type"], "command");

    std::fs::write(
        &settings,
        r#"{"model":"opus","hooks":{"SessionStart":[]},"zeta":1}"#,
    )
    .unwrap();
    install_statusline(&claude_home, launcher, state).unwrap();
    install_statusline(&claude_home, launcher, state).unwrap();
    let text = std::fs::read_to_string(&settings).unwrap();
    let order =
        ["\"model\"", "\"hooks\"", "\"zeta\"", "\"statusLine\""].map(|k| text.find(k).unwrap());
    assert!(order.windows(2).all(|w| w[0] < w[1]), "{text}");

    let moved = Path::new("/usr/bin/waystation");
    assert_eq!(
        statusline_status(&claude_home, moved, state),
        StatusLine::Stale
    );
    install_statusline(&claude_home, moved, state).unwrap();
    assert_eq!(
        statusline_status(&claude_home, moved, state),
        StatusLine::Installed
    );

    // Someone else's status line is never replaced.
    let foreign = r#"{"statusLine":{"type":"command","command":"~/bin/my-line"}}"#;
    std::fs::write(&settings, foreign).unwrap();
    assert_eq!(
        statusline_status(&claude_home, launcher, state),
        StatusLine::Other("~/bin/my-line".into())
    );
    assert!(install_statusline(&claude_home, launcher, state).is_err());
    assert_eq!(std::fs::read_to_string(&settings).unwrap(), foreign);

    std::fs::write(&settings, "{ nope").unwrap();
    assert!(install_statusline(&claude_home, launcher, state).is_err());
    assert_eq!(std::fs::read_to_string(&settings).unwrap(), "{ nope");
}

#[test]
fn token_counts_are_short() {
    assert_eq!(format_tokens(0), "0");
    assert_eq!(format_tokens(950), "950");
    assert_eq!(format_tokens(41_049), "41k");
    assert_eq!(format_tokens(410_000), "410k");
    assert_eq!(format_tokens(1_234_567), "1.2M");
    assert_eq!(format_tokens(18_900_000), "18.9M");
    assert_eq!(format_tokens(2_500_000_000), "2.5B");
}

#[test]
fn a_status_line_script_that_calls_waystation_counts_as_chained() {
    let d = tempfile::tempdir().unwrap();
    let claude_home = d.path().join("claude");
    std::fs::create_dir_all(&claude_home).unwrap();
    let script = claude_home.join("statusline-command.sh");
    std::fs::write(
        &script,
        "#!/usr/bin/env bash\ninput=$(cat)\nprintf '%s' \"$input\" | '/opt/waystation' __statusline '/state'\n",
    )
    .unwrap();
    let settings = claude_home.join("settings.json");
    let chained = serde_json::json!({"statusLine": {"type": "command", "command": format!("bash {}", script.display())}});
    std::fs::write(&settings, chained.to_string()).unwrap();
    let (launcher, state) = (Path::new("/opt/waystation"), Path::new("/state"));
    assert_eq!(
        statusline_status(&claude_home, launcher, state),
        StatusLine::Chained
    );
    // Waystation never rewrites a status line that already feeds it.
    assert!(install_statusline(&claude_home, launcher, state).is_err());
    assert_eq!(
        std::fs::read_to_string(&settings).unwrap(),
        chained.to_string()
    );

    std::fs::write(&script, "#!/bin/sh\necho mine\n").unwrap();
    assert!(matches!(
        statusline_status(&claude_home, launcher, state),
        StatusLine::Other(_)
    ));
}
