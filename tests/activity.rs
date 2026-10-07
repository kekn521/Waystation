use serde_json::json;
use waystation::activity::{Activity, ActivityState, from_event};

const NOW: i64 = 1_791_400_000;

fn event(value: serde_json::Value) -> Option<(ActivityState, String)> {
    from_event(&value, NOW).map(|Activity { state, detail, at }| {
        assert_eq!(at, NOW);
        (state, detail)
    })
}

#[test]
fn prompts_and_tools_mark_the_agent_running() {
    use ActivityState::*;
    assert_eq!(
        event(json!({"hook_event_name": "SessionStart", "source": "startup"})),
        Some((Ready, String::new()))
    );
    assert_eq!(
        event(
            json!({"hook_event_name": "UserPromptSubmit", "prompt": "\n  fix the flaky test\nthen push"})
        ),
        Some((Running, "› fix the flaky test".into()))
    );
    assert_eq!(
        event(json!({"hook_event_name": "PreToolUse", "tool_name": "Bash",
            "tool_input": {"command": "cargo test --test usage\necho done", "description": "Run tests"}})),
        Some((Running, "$ cargo test --test usage".into()))
    );
    assert_eq!(
        event(json!({"hook_event_name": "PreToolUse", "tool_name": "Edit",
            "tool_input": {"file_path": "/home/me/code/station/src/usage.rs"}})),
        Some((Running, "Edit src/usage.rs".into()))
    );
    assert_eq!(
        event(
            json!({"hook_event_name": "PreToolUse", "tool_name": "apply_patch",
            "tool_input": {"command": "*** Begin Patch\n*** Update File: src/app.rs\n@@"}})
        ),
        Some((Running, "Patch src/app.rs".into()))
    );
    assert_eq!(
        event(
            json!({"hook_event_name": "PreToolUse", "tool_name": "mcp__github__search", "tool_input": {}})
        ),
        Some((Running, "mcp__github__search".into()))
    );
}

#[test]
fn tool_results_show_their_first_output_line() {
    use ActivityState::*;
    // Claude reports Bash output as an object, Codex as a string.
    assert_eq!(
        event(
            json!({"hook_event_name": "PostToolUse", "tool_name": "Bash",
            "tool_input": {"command": "cargo test"},
            "tool_response": {"stdout": "\n\nrunning 11 tests\nok", "stderr": ""}})
        ),
        Some((Running, "⎿ running 11 tests".into()))
    );
    assert_eq!(
        event(
            json!({"hook_event_name": "PostToolUse", "tool_name": "Bash",
            "tool_input": {"command": "true"}, "tool_response": "Exit code: 0\nWall time: 1s"})
        ),
        Some((Running, "⎿ Exit code: 0".into()))
    );
    assert_eq!(
        event(
            json!({"hook_event_name": "PostToolUse", "tool_name": "Read",
            "tool_input": {"file_path": "/a/b/c.rs"}, "tool_response": {"type": "text"}})
        ),
        Some((Running, "Read b/c.rs".into()))
    );
    assert_eq!(
        event(
            json!({"hook_event_name": "PostToolUseFailure", "tool_name": "Bash",
            "tool_input": {"command": "false"}, "error": "Command failed with exit code 1"})
        ),
        Some((Running, "✗ Command failed with exit code 1".into()))
    );
}

#[test]
fn questions_and_permission_requests_need_input() {
    use ActivityState::*;
    assert_eq!(
        event(
            json!({"hook_event_name": "PermissionRequest", "tool_name": "Bash",
            "tool_input": {"command": "rm -rf target"}})
        ),
        Some((NeedsInput, "Allow $ rm -rf target?".into()))
    );
    assert_eq!(
        event(
            json!({"hook_event_name": "Notification", "notification_type": "permission_prompt",
            "message": "Claude needs your permission to use Bash"})
        ),
        Some((
            NeedsInput,
            "Claude needs your permission to use Bash".into()
        ))
    );
    // Idle reminders and other notifications change nothing.
    assert_eq!(
        event(
            json!({"hook_event_name": "Notification", "notification_type": "idle_prompt",
            "message": "Claude is waiting for your input"})
        ),
        None
    );
}

#[test]
fn finished_turns_show_the_reply() {
    use ActivityState::*;
    assert_eq!(
        event(
            json!({"hook_event_name": "Stop", "last_assistant_message": "The flake came from a shared temp dir.\n\nDetails…"})
        ),
        Some((Done, "The flake came from a shared temp dir.".into()))
    );
    assert_eq!(
        event(json!({"hook_event_name": "Stop", "last_assistant_message": null})),
        Some((Done, "Finished".into()))
    );
    assert_eq!(
        event(json!({"hook_event_name": "Interrupt", "turn_id": "t1"})),
        Some((Ready, "Interrupted".into()))
    );
    assert_eq!(event(json!({"hook_event_name": "PreCompact"})), None);
    assert_eq!(event(json!({"no": "event"})), None);
}

#[test]
fn details_are_one_clean_bounded_line() {
    let long = "x".repeat(500);
    let (_, detail) = event(
        json!({"hook_event_name": "UserPromptSubmit", "prompt": format!("a\u{1b}[31mb\t{long}")}),
    )
    .unwrap();
    assert!(detail.starts_with("› a�[31mb x"), "{detail}");
    assert_eq!(detail.chars().count(), 200);
    assert!(detail.ends_with('…'));
}
