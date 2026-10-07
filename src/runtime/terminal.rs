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
    pub fn run_foreground(
        &mut self,
        spec: &super::command::CommandSpec,
    ) -> Result<std::process::ExitStatus> {
        self.run(spec, false).map(|(status, _)| status)
    }
    /// Like `run_foreground`, also returning what the child wrote to stderr, which the
    /// alternate screen would otherwise clear before anyone reads it.
    pub fn run_foreground_stderr(
        &mut self,
        spec: &super::command::CommandSpec,
    ) -> Result<(std::process::ExitStatus, String)> {
        self.run(spec, true)
    }
    fn run(
        &mut self,
        spec: &super::command::CommandSpec,
        capture_stderr: bool,
    ) -> Result<(std::process::ExitStatus, String)> {
        self.restore()?;
        use nix::sys::signal::{SaFlags, SigAction, SigHandler, SigSet, Signal, sigaction};
        use std::os::unix::process::CommandExt;
        // Ignore terminal interrupts in the hub while the foreground child owns them.
        let ignore = SigAction::new(SigHandler::SigIgn, SaFlags::empty(), SigSet::empty());
        let previous = unsafe { sigaction(Signal::SIGINT, &ignore) }?;
        let mut cmd = std::process::Command::new(&spec.program);
        cmd.args(&spec.args).current_dir(&spec.cwd);
        unsafe {
            cmd.pre_exec(|| {
                let default = SigAction::new(SigHandler::SigDfl, SaFlags::empty(), SigSet::empty());
                sigaction(Signal::SIGINT, &default).map_err(std::io::Error::from)?;
                Ok(())
            });
        }
        if capture_stderr {
            cmd.stderr(std::process::Stdio::piped());
        }
        let result = cmd.spawn().and_then(|mut child| {
            // Drained on a thread so a chatty child never blocks on a full pipe.
            let reader = child.stderr.take().map(|mut e| {
                std::thread::spawn(move || {
                    use std::io::Read;
                    let mut buf = vec![];
                    let _ = (&mut e).take(64 * 1024).read_to_end(&mut buf);
                    let _ = io::copy(&mut e, &mut io::sink());
                    String::from_utf8_lossy(&buf).into_owned()
                })
            });
            let status = child.wait()?;
            let stderr = reader.and_then(|r| r.join().ok()).unwrap_or_default();
            Ok((status, stderr))
        });
        let signal_restored = unsafe { sigaction(Signal::SIGINT, &previous) };
        signal_restored?;
        let restored = self.resume();
        restored?;
        Ok(result?)
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
