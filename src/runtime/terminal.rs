use anyhow::Result;
use crossterm::{
    cursor::Show,
    event::{DisableBracketedPaste, DisableMouseCapture, EnableBracketedPaste, EnableMouseCapture},
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
    /// Runs `spec` inside Waystation's terminal emulator, drawing `overlay` over it while
    /// `visible`. Waystation's own mouse and paste modes are off meanwhile: the hosted program
    /// chooses its own.
    pub fn host(
        &mut self,
        spec: &super::command::CommandSpec,
        visible: &mut bool,
        overlay: &mut dyn FnMut(&mut ratatui::buffer::Buffer),
    ) -> Result<super::host::Outcome> {
        execute!(io::stdout(), DisableMouseCapture, DisableBracketedPaste)?;
        let outcome = super::host::run(spec, visible, overlay);
        let restored = execute!(io::stdout(), EnableMouseCapture, EnableBracketedPaste)
            .map_err(anyhow::Error::from)
            .and_then(|_| Ok(self.terminal.clear()?));
        let outcome = outcome?;
        restored?;
        Ok(outcome)
    }
    pub fn resume(&mut self) -> Result<()> {
        enable_raw_mode()?;
        self.active = true;
        execute!(
            io::stdout(),
            EnterAlternateScreen,
            EnableMouseCapture,
            EnableBracketedPaste
        )?;
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
                DisableBracketedPaste,
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
