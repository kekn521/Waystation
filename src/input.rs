use crate::app::{Action, App};
use crate::model::Section;
use crossterm::event::{Event, KeyCode, KeyEventKind, KeyModifiers, MouseEventKind};
pub fn translate(event: Event, app: &App) -> Option<Action> {
    match event {
        Event::Paste(text) if app.form.is_some() || app.searching => Some(Action::Paste(text)),
        Event::Mouse(m) if m.kind == MouseEventKind::Down(crossterm::event::MouseButton::Left) => {
            app.hits
                .iter()
                .rev()
                .find(|h| h.area.contains((m.column, m.row).into()))
                .map(|h| h.action.clone())
        }
        Event::Key(k) if k.kind != KeyEventKind::Release => {
            if app.form.is_some() {
                return match k.code {
                    KeyCode::Esc => Some(Action::Escape),
                    KeyCode::Tab | KeyCode::Down => Some(Action::FormField(1)),
                    KeyCode::BackTab | KeyCode::Up => Some(Action::FormField(-1)),
                    KeyCode::Left => Some(Action::FormCursor(-1)),
                    KeyCode::Right => Some(Action::FormCursor(1)),
                    KeyCode::Enter => Some(Action::Activate),
                    KeyCode::Backspace => Some(Action::Backspace),
                    KeyCode::Char('s') if k.modifiers.contains(KeyModifiers::CONTROL) => {
                        Some(Action::FormSave)
                    }
                    KeyCode::Char('u') if k.modifiers.contains(KeyModifiers::CONTROL) => {
                        Some(Action::FormClear)
                    }
                    KeyCode::Char('c') if k.modifiers.contains(KeyModifiers::CONTROL) => {
                        Some(Action::Escape)
                    }
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
            if k.modifiers.contains(KeyModifiers::CONTROL) && k.code == KeyCode::Char('c') {
                return Some(Action::Quit);
            }
            if app.confirmation.is_some() || app.detail.is_some() {
                return match k.code {
                    KeyCode::Esc => Some(Action::Escape),
                    KeyCode::Enter => Some(Action::Activate),
                    KeyCode::Up | KeyCode::Char('k') => Some(Action::Move(-1)),
                    KeyCode::Down | KeyCode::Char('j') => Some(Action::Move(1)),
                    KeyCode::Char('q') => Some(Action::Escape),
                    _ => None,
                };
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
                KeyCode::Char('1'..='8') => {
                    if let KeyCode::Char(c) = k.code {
                        Action::Nav(*Section::ALL.get(c as usize - '1' as usize)?)
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
                KeyCode::Char('x') => Action::Stop,
                KeyCode::Char('n') if app.section == Section::Agents => Action::NewAgent,
                KeyCode::Char('.') => Action::Hidden,
                KeyCode::Char('y') => Action::Copy,
                KeyCode::F(5) => Action::Reload,
                _ => return None,
            })
        }
        _ => None,
    }
}
