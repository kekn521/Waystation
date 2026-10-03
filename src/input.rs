use crate::app::{Action, App};
use crate::model::Section;
use crossterm::event::{Event, KeyCode, KeyEventKind, KeyModifiers, MouseEventKind};
pub fn translate(event: Event, app: &App) -> Option<Action> {
    match event {
        Event::Mouse(m) if m.kind == MouseEventKind::Down(crossterm::event::MouseButton::Left) => {
            app.hits
                .iter()
                .rev()
                .find(|h| h.area.contains((m.column, m.row).into()))
                .map(|h| h.action.clone())
        }
        Event::Key(k) if k.kind != KeyEventKind::Release => {
            if k.modifiers.contains(KeyModifiers::CONTROL) && k.code == KeyCode::Char('c') {
                return Some(Action::Quit);
            }
            if app.searching {
                return match k.code {
                    KeyCode::Esc => Some(Action::Escape),
                    KeyCode::Enter => Some(Action::Activate),
                    KeyCode::Backspace => Some(Action::Backspace),
                    KeyCode::Down => Some(Action::Move(1)),
                    KeyCode::Up => Some(Action::Move(-1)),
                    KeyCode::Char(c)
                        if !k
                            .modifiers
                            .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
                    {
                        Some(Action::Insert(c))
                    }
                    _ => None,
                };
            }
            Some(match k.code {
                KeyCode::Char('1'..='9') => {
                    if let KeyCode::Char(c) = k.code {
                        Action::Nav(Section::ALL[c as usize - '1' as usize])
                    } else {
                        return None;
                    }
                }
                KeyCode::Char('q') => Action::Quit,
                KeyCode::Char('/') => Action::Search,
                KeyCode::Char('?') => Action::Help,
                KeyCode::Esc => Action::Escape,
                KeyCode::Down | KeyCode::Char('j') => Action::Move(1),
                KeyCode::Up | KeyCode::Char('k') => Action::Move(-1),
                KeyCode::Tab => Action::FocusNext,
                KeyCode::Enter => Action::Activate,
                KeyCode::Char('e') => Action::Editor,
                KeyCode::Char('t') => Action::Shell,
                KeyCode::Char('g') => Action::Git,
                KeyCode::Char('h') => Action::Herdr,
                KeyCode::Char('f') => Action::Files,
                KeyCode::Char('r') => Action::Rerun,
                KeyCode::Char('x') => Action::Stop,
                KeyCode::Char('n') => Action::Recipes,
                KeyCode::Char('.') => Action::Hidden,
                KeyCode::Char('y') => Action::Copy,
                KeyCode::F(5) => Action::Reload,
                _ => return None,
            })
        }
        _ => None,
    }
}
