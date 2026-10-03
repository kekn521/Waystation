use crate::{
    config::Config,
    model::{AppState, Section},
};
use ratatui::layout::Rect;
#[derive(Clone, Debug)]
pub enum Action {
    Nav(Section),
    Move(isize),
    FocusNext,
    Search,
    Insert(char),
    Backspace,
    Escape,
    Help,
    Quit,
    Activate,
    Editor,
    Shell,
    Git,
    Herdr,
    Files,
    Reload,
    Hidden,
    Copy,
    Stop,
    Rerun,
    Recipes,
    Project(usize),
    Tool(String),
}
#[derive(Clone, Debug)]
pub enum Effect {
    Quit,
}
#[derive(Clone, Debug)]
pub struct HitRegion {
    pub area: Rect,
    pub action: Action,
}
pub struct App {
    pub config: Config,
    pub state: AppState,
    pub section: Section,
    pub pane: usize,
    pub selection: usize,
    pub query: String,
    pub searching: bool,
    pub help: bool,
    pub message: Option<String>,
    pub hits: Vec<HitRegion>,
}
impl App {
    pub fn new(config: Config, state: AppState) -> Self {
        Self {
            config,
            state,
            section: Section::Overview,
            pane: 0,
            selection: 0,
            query: String::new(),
            searching: false,
            help: false,
            message: None,
            hits: vec![],
        }
    }
    pub fn update(&mut self, action: Action) -> Vec<Effect> {
        match action {
            Action::Nav(s) => {
                self.section = s;
                self.selection = 0;
                self.searching = false;
                self.message = None
            }
            Action::Search => {
                self.searching = true;
                self.query.clear()
            }
            Action::Insert(c) => self.query.push(c),
            Action::Backspace => {
                self.query.pop();
            }
            Action::Escape => {
                self.searching = false;
                self.query.clear();
                self.help = false;
                self.message = None
            }
            Action::Help => self.help = !self.help,
            Action::FocusNext => self.pane = (self.pane + 1) % 4,
            Action::Move(n) => self.selection = self.selection.saturating_add_signed(n),
            Action::Quit => return vec![Effect::Quit],
            _ => self.message = Some("This integration is being connected.".into()),
        };
        vec![]
    }
}
