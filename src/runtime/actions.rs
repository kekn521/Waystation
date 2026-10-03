use crate::{
    app::Action,
    config::{Config, ToolCommand},
    runtime::command::{CommandSpec, executable},
};
use anyhow::{Context, Result, bail, ensure};
use std::{ffi::OsString, path::Path};
fn tool(c: &Config, name: &str) -> ToolCommand {
    c.tools.get(name).cloned().unwrap_or(ToolCommand {
        program: name.into(),
        args: vec![],
    })
}
pub fn resolve(action: &Action, c: &Config, workspace: &Path) -> Result<CommandSpec> {
    ensure!(
        workspace.is_dir(),
        "Workspace unavailable: {}",
        workspace.display()
    );
    let editor = || {
        c.editor
            .clone()
            .or_else(|| {
                ["hx", "nvim", "vim", "vi"]
                    .iter()
                    .find(|p| executable(p.as_ref()).is_some())
                    .map(|p| ToolCommand {
                        program: (*p).into(),
                        args: vec![],
                    })
            })
            .context("No editor found; set [editor] program and args in config.toml")
    };
    let mut extra: Vec<OsString> = vec![];
    let command = match action {
        Action::Editor => editor()?,
        Action::OpenPath(p) => {
            extra.push(
                p.canonicalize()
                    .context("File unavailable")?
                    .into_os_string(),
            );
            editor()?
        }
        Action::Shell => ToolCommand {
            program: std::env::var("SHELL").unwrap_or("/bin/bash".into()),
            args: vec![],
        },
        Action::Herdr => tool(c, "herdr"),
        Action::Tool(name) => tool(c, name),
        Action::Git => {
            if c.tools.contains_key("git-ui") {
                tool(c, "git-ui")
            } else if executable("lazygit".as_ref()).is_some() {
                tool(c, "lazygit")
            } else {
                bail!(
                    "No Git UI configured. Branch, changes and worktrees are shown in Workspaces. Set [tools.git-ui] to enable editing."
                )
            }
        }
        Action::Connect(host) => {
            ensure!(
                !host.is_empty()
                    && !host.starts_with('-')
                    && !host.chars().any(|c| c.is_whitespace() || c.is_control()),
                "Invalid SSH alias"
            );
            extra.push(host.into());
            tool(c, "ssh")
        }
        Action::Attach(id) => {
            ensure!(
                id.strip_prefix('$')
                    .is_some_and(|s| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit())),
                "Invalid tmux session ID"
            );
            extra.extend([
                if std::env::var_os("TMUX").is_some() {
                    "switch-client"
                } else {
                    "attach-session"
                }
                .into(),
                "-t".into(),
                id.into(),
            ]);
            tool(c, "tmux")
        }
        _ => bail!("This action does not launch a program"),
    };
    let program =
        super::command::executable_in(command.program.as_ref(), workspace).with_context(|| {
            format!(
                "{} is not installed; configure it in config.toml",
                command.program
            )
        })?;
    let mut args = command
        .args
        .into_iter()
        .map(OsString::from)
        .collect::<Vec<_>>();
    args.extend(extra);
    Ok(CommandSpec {
        program: program.into_os_string(),
        args,
        cwd: workspace.to_path_buf(),
    })
}
pub fn copy_path(path: &Path) -> Result<()> {
    use std::{
        io::Write,
        os::unix::ffi::OsStrExt,
        process::{Command, Stdio},
        time::{Duration, Instant},
    };
    let (program, args) = if let Some(p) = executable("wl-copy".as_ref()) {
        (p, vec![])
    } else if let Some(p) = executable("xclip".as_ref()) {
        (p, vec!["-selection", "clipboard"])
    } else {
        bail!("Clipboard unavailable: install wl-copy or xclip")
    };
    let mut child = Command::new(program)
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()?;
    if let Some(mut input) = child.stdin.take() {
        input.write_all(path.as_os_str().as_bytes())?;
    }
    let until = Instant::now() + Duration::from_secs(2);
    loop {
        if let Some(status) = child.try_wait()? {
            ensure!(status.success(), "Clipboard command failed: {status}");
            return Ok(());
        }
        if Instant::now() > until {
            let _ = child.kill();
            let _ = child.wait();
            bail!("Clipboard timed out")
        };
        std::thread::sleep(Duration::from_millis(10));
    }
}
