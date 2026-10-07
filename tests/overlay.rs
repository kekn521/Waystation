use ratatui::{buffer::Buffer, layout::Rect};
use std::collections::HashMap;
use waystation::{
    activity::{Activity, ActivityState},
    agents::{AgentSession, AgentStatus},
    ui::overlay::{render, rows},
};

const NOW: i64 = 1_791_400_000;

fn session(name: &str, tool: &str, status: AgentStatus, created: u64) -> AgentSession {
    let json = serde_json::json!({
        "id": uuid::Uuid::new_v4(),
        "name": name,
        "tool": tool,
        "workspace": "/tmp",
        "command": {"program": tool, "args": []},
        "executable": format!("/usr/bin/{tool}"),
        "created": {"secs_since_epoch": created, "nanos_since_epoch": 0},
    });
    let mut s: AgentSession = serde_json::from_value(json).unwrap();
    s.status = status;
    s
}

fn activity(state: ActivityState, detail: &str, ago: i64) -> Activity {
    Activity {
        state,
        detail: detail.into(),
        at: NOW - ago,
    }
}

fn screen(buf: &Buffer) -> String {
    let w = buf.area.width as usize;
    buf.content
        .chunks(w)
        .map(|row| row.iter().map(|c| c.symbol()).collect::<String>())
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn rows_combine_status_and_activity_and_put_waiting_agents_first() {
    let api = session("api", "claude", AgentStatus::Running, 1);
    let docs = session("docs", "codex", AgentStatus::Running, 2);
    let slides = session("slides", "claude", AgentStatus::Running, 3);
    let fresh = session("fresh", "codex", AgentStatus::Running, 4);
    let old = session("old", "claude", AgentStatus::Exited(1), 5);
    let saved = session("saved", "claude", AgentStatus::Saved, 6);
    let activities = HashMap::from([
        (api.id, activity(ActivityState::Running, "$ cargo test", 12)),
        (
            docs.id,
            activity(ActivityState::NeedsInput, "Allow Edit README.md?", 75),
        ),
        (
            slides.id,
            activity(ActivityState::Done, "Updated the deck title.", 130),
        ),
        // Exited and saved sessions ignore what their hooks last said.
        (old.id, activity(ActivityState::Running, "$ sleep", 5)),
    ]);
    let sessions = [api, docs, slides, fresh, old, saved];
    let rows = rows(&sessions, &activities, NOW);
    let summary = rows
        .iter()
        .map(|r| {
            (
                r.name.as_str(),
                r.state.as_str(),
                r.age.as_str(),
                r.detail.as_str(),
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(
        summary,
        [
            ("docs", "needs input", "1m", "Allow Edit README.md?"),
            ("api", "running", "12s", "$ cargo test"),
            ("slides", "done", "2m", "Updated the deck title."),
            // No activity at all: started before status tracking, so say so.
            ("fresh", "untracked", "", "r restarts with status"),
            ("old", "exited 1", "", ""),
            ("saved", "saved", "", ""),
        ]
    );
}

#[test]
fn box_floats_top_right_and_keeps_to_its_bounds() {
    let sessions = (0..7)
        .map(|i| session(&format!("agent-{i}"), "claude", AgentStatus::Running, i))
        .collect::<Vec<_>>();
    let activities = sessions
        .iter()
        .map(|s| (s.id, activity(ActivityState::Running, &"x".repeat(80), 3)))
        .collect::<HashMap<_, _>>();
    let rows = rows(&sessions, &activities, NOW);
    let mut buf = Buffer::empty(Rect::new(0, 0, 100, 30));
    for cell in buf.content.iter_mut() {
        cell.set_symbol("·");
    }
    render(&mut buf, &rows);
    let text = screen(&buf);
    let lines = text.lines().collect::<Vec<_>>();
    // Row 0 stays visible; the box starts on row 1, one column from the right edge.
    assert!(lines[0].chars().all(|c| c == '·'), "{text}");
    assert!(lines[1].contains("╭ Agents · F9"), "{text}");
    assert!(lines[1].ends_with("╮·"), "{text}");
    assert!(text.contains("+2 more"), "five agents at most: {text}");
    assert!(text.contains("x…"), "details are cut to fit: {text}");
    let width = lines[1].chars().filter(|&c| c != '·').count();
    assert!((30..=40).contains(&width), "{width}");

    // Too small to float anything: the program keeps the whole screen.
    let mut tiny = Buffer::empty(Rect::new(0, 0, 30, 5));
    render(&mut tiny, &rows);
    assert!(!screen(&tiny).contains("Agents"));
    let mut none = Buffer::empty(Rect::new(0, 0, 100, 30));
    render(&mut none, &[]);
    assert!(!screen(&none).contains("Agents"));
}
