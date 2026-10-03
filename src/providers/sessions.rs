use crate::runtime::command::{CommandRunner, CommandSpec};
use anyhow::{Result, ensure};
use std::{path::PathBuf, time::Duration};
#[derive(Clone, Debug)]
pub struct TmuxSession {
    pub id: String,
    pub name: String,
    pub cwd: PathBuf,
}
pub fn parse(s: &str) -> Vec<TmuxSession> {
    s.lines()
        .take(1000)
        .filter_map(|l| {
            let mut fields = l.splitn(3, '\t');
            let id = fields.next()?;
            if !id
                .strip_prefix('$')
                .is_some_and(|s| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit()))
            {
                return None;
            }
            Some(TmuxSession {
                id: id.into(),
                name: fields.next()?.into(),
                cwd: fields.next()?.into(),
            })
        })
        .collect()
}
pub fn list(runner: &CommandRunner) -> Result<Vec<TmuxSession>> {
    let o = runner.capture(
        &CommandSpec {
            program: "tmux".into(),
            args: [
                "list-sessions",
                "-F",
                "#{session_id}\t#{session_name}\t#{session_path}",
            ]
            .into_iter()
            .map(Into::into)
            .collect(),
            cwd: std::env::temp_dir(),
        },
        Duration::from_secs(2),
        65536,
    )?;
    ensure!(
        o.status.success(),
        "tmux: {}",
        String::from_utf8_lossy(&o.stderr)
    );
    Ok(parse(&String::from_utf8_lossy(&o.stdout)))
}
