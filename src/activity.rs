//! What each agent session is doing, as reported by its hooks.
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::{Path, PathBuf};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ActivityState {
    /// Started, or interrupted, and waiting for a prompt.
    Ready,
    Running,
    /// Waiting on the user mid-task: a permission prompt or a question.
    NeedsInput,
    /// Finished its turn.
    Done,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Activity {
    pub state: ActivityState,
    /// One line: the command, question or reply the state is about.
    pub detail: String,
    /// Epoch seconds.
    pub at: i64,
}

/// Where a session's activity is kept, beside its record.
pub fn path(manifest: &Path) -> PathBuf {
    manifest.with_extension("activity.json")
}
pub fn read(manifest: &Path) -> Option<Activity> {
    serde_json::from_slice(&std::fs::read(path(manifest)).ok()?).ok()
}

const DETAIL_CHARS: usize = 200;
/// The first non-blank line, with control characters replaced, at most `DETAIL_CHARS` long.
fn line(text: &str) -> String {
    let first = text
        .lines()
        .map(str::trim)
        .find(|l| !l.is_empty())
        .unwrap_or("");
    let clean = first
        .chars()
        .map(|c| match c {
            '\t' => ' ',
            c if c.is_control() => '�',
            c => c,
        })
        .collect::<String>();
    if clean.chars().count() > DETAIL_CHARS {
        clean.chars().take(DETAIL_CHARS - 1).chain(['…']).collect()
    } else {
        clean
    }
}
/// The last two components of a path: enough to recognise the file.
fn short_path(path: &str) -> String {
    let parts = path
        .rsplit('/')
        .filter(|p| !p.is_empty())
        .take(2)
        .collect::<Vec<_>>();
    parts.into_iter().rev().collect::<Vec<_>>().join("/")
}
/// What a tool call is doing, as a person would say it.
fn describe_tool(event: &Value) -> String {
    let name = event["tool_name"].as_str().unwrap_or("tool");
    let input = &event["tool_input"];
    let command = input["command"].as_str();
    if name == "apply_patch" {
        let file = command
            .and_then(|patch| {
                patch.lines().find_map(|l| {
                    ["*** Update File: ", "*** Add File: ", "*** Delete File: "]
                        .iter()
                        .find_map(|p| l.strip_prefix(p))
                })
            })
            .map(short_path);
        return line(&format!("Patch {}", file.unwrap_or_default()));
    }
    if let Some(command) = command {
        return line(&format!("$ {}", line(command)));
    }
    if let Some(file) = input["file_path"]
        .as_str()
        .or(input["notebook_path"].as_str())
    {
        return line(&format!("{name} {}", short_path(file)));
    }
    line(name)
}
/// The first output line of a finished tool call, if it reported any.
fn tool_output(event: &Value) -> Option<String> {
    let response = event.get("tool_response").or(event.get("tool_result"))?;
    let text = match response {
        Value::String(s) => Some(s.as_str()),
        Value::Object(o) => ["stdout", "output", "stderr"]
            .iter()
            .find_map(|k| o.get(*k)?.as_str().filter(|s| !s.trim().is_empty())),
        _ => None,
    }?;
    let first = line(text);
    (!first.is_empty()).then(|| format!("⎿ {first}"))
}

/// What a hook event says about its agent, or `None` for events that change nothing.
pub fn from_event(event: &Value, now: i64) -> Option<Activity> {
    use ActivityState::*;
    let (state, detail) = match event["hook_event_name"].as_str()? {
        "SessionStart" => (Ready, String::new()),
        "UserPromptSubmit" => (
            Running,
            line(&format!(
                "› {}",
                line(event["prompt"].as_str().unwrap_or(""))
            )),
        ),
        "PreToolUse" => (Running, describe_tool(event)),
        "PostToolUse" => (
            Running,
            tool_output(event).unwrap_or_else(|| describe_tool(event)),
        ),
        "PostToolUseFailure" => (
            Running,
            line(&format!(
                "✗ {}",
                line(event["error"].as_str().unwrap_or("failed"))
            )),
        ),
        "PermissionRequest" => (
            NeedsInput,
            line(&format!("Allow {}?", describe_tool(event))),
        ),
        "Notification" => match event["notification_type"].as_str()? {
            "permission_prompt"
            | "elicitation_dialog"
            | "elicitation_url_dialog"
            | "agent_needs_input" => (NeedsInput, line(event["message"].as_str().unwrap_or(""))),
            _ => return None,
        },
        "Stop" => (
            Done,
            event["last_assistant_message"]
                .as_str()
                .map(line)
                .filter(|l| !l.is_empty())
                .unwrap_or_else(|| "Finished".into()),
        ),
        "Interrupt" => (Ready, "Interrupted".into()),
        _ => return None,
    };
    Some(Activity {
        state,
        detail,
        at: now,
    })
}
