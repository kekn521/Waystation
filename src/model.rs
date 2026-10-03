use serde::{Deserialize, Serialize};
use std::{path::PathBuf, time::SystemTime};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Section {
    Overview,
    Workspaces,
    Agents,
    Tasks,
    Services,
    Connections,
    Files,
    System,
    Activity,
}
impl Section {
    pub const ALL: [Self; 9] = [
        Self::Overview,
        Self::Workspaces,
        Self::Agents,
        Self::Tasks,
        Self::Services,
        Self::Connections,
        Self::Files,
        Self::System,
        Self::Activity,
    ];
    pub fn name(self) -> &'static str {
        match self {
            Self::Overview => "Overview",
            Self::Workspaces => "Workspaces",
            Self::Agents => "Agents",
            Self::Tasks => "Tasks",
            Self::Services => "Services",
            Self::Connections => "Connections",
            Self::Files => "Files",
            Self::System => "System",
            Self::Activity => "Activity",
        }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ActivityEntry {
    pub id: String,
    pub at: SystemTime,
    #[serde(with = "crate::path_serde::option")]
    pub workspace: Option<PathBuf>,
    pub kind: ActivityKind,
    pub outcome: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum ActivityKind {
    Launch(String),
    TaskRun(String),
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AppState {
    pub schema_version: u32,
    #[serde(with = "crate::path_serde::option")]
    pub selected_workspace: Option<PathBuf>,
    #[serde(with = "crate::path_serde::vec")]
    pub recent_workspaces: Vec<PathBuf>,
    pub activity: Vec<ActivityEntry>,
}
impl Default for AppState {
    fn default() -> Self {
        Self {
            schema_version: 1,
            selected_workspace: None,
            recent_workspaces: vec![],
            activity: vec![],
        }
    }
}
#[derive(Clone, Debug)]
pub enum Availability {
    Loading,
    Ready,
    Unavailable(String),
    Failed(String),
}
#[derive(Clone, Debug)]
pub struct Snapshot<T> {
    pub value: Option<T>,
    pub availability: Availability,
    pub observed_at: Option<SystemTime>,
    pub generation: u64,
}
impl<T> Default for Snapshot<T> {
    fn default() -> Self {
        Self {
            value: None,
            availability: Availability::Loading,
            observed_at: None,
            generation: 0,
        }
    }
}
impl<T> Snapshot<T> {
    pub fn ready(value: T, generation: u64) -> Self {
        Self {
            value: Some(value),
            availability: Availability::Ready,
            observed_at: Some(SystemTime::now()),
            generation,
        }
    }
}
