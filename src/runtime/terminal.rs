use anyhow::Result;
use crossterm::{
    cursor::Show,
    event::{DisableMouseCapture, EnableMouseCapture},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use std::io::{self, Write};
pub struct TerminalSession {
    pub terminal: ratatui::DefaultTerminal,
    active: bool,
}
impl TerminalSession {
    pub fn enter() -> Result<Self> {
        let mut session = Self {
            terminal: ratatui::Terminal::new(
                ratatui::backend::CrosstermBackend::new(io::stdout()),
            )?,
            active: false,
        };
        session.resume()?;
        Ok(session)
    }
    pub fn resume(&mut self) -> Result<()> {
        enable_raw_mode()?;
        self.active = true;
        execute!(io::stdout(), EnterAlternateScreen, EnableMouseCapture)?;
        self.terminal.clear()?;
        Ok(())
    }
    pub fn restore(&mut self) -> Result<()> {
        if self.active {
            self.active = false;
            let a = disable_raw_mode();
            let b = execute!(
                io::stdout(),
                DisableMouseCapture,
                LeaveAlternateScreen,
                Show
            );
            let _ = io::stdout().flush();
            a?;
            b?;
        }
        Ok(())
    }
}
impl Drop for TerminalSession {
    fn drop(&mut self) {
        let _ = self.restore();
    }
}
