//! Read-only Git context for a workspace: branch, change counts, and linked
//! worktrees. Every command runs with a bounded capture and a short deadline;
//! nothing here ever mutates a repository or touches the network.

use std::os::unix::ffi::OsStringExt;
use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{Context as _, Result, bail};

use crate::runtime::command::{CommandRunner, CommandSpec};

/// How long a single Git command may run before it is killed.
const GIT_TIMEOUT: Duration = Duration::from_secs(3);

/// Per-stream capture cap (git output for one repo is far below this).
const GIT_OUTPUT_LIMIT: usize = 1024 * 1024;

/// Snapshot of a repository at one point in time.
#[derive(Clone, Debug, Default)]
pub struct GitState {
    /// Current branch name; `Some("(detached)")` when HEAD is not on one;
    /// `None` only when the output carried no branch header at all.
    pub branch: Option<String>,
    /// Number of changed entries (modified, renamed, unmerged, untracked),
    /// counting each entry once regardless of how many columns changed.
    pub changed: usize,
    /// Commits ahead of the upstream, from `# branch.ab +A -B`.
    pub ahead: usize,
    /// Commits behind the upstream.
    pub behind: usize,
    /// Checked-out worktree paths, including the primary one.
    pub worktrees: Vec<PathBuf>,
}

/// Decode a label or path for display only; byte-exact paths go through
/// [`parse_worktrees`] instead.
fn label(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

/// Parse `git status --porcelain=v2 --branch -z` output (NUL-separated).
///
/// Header lines start with `# `; entry lines begin with `1`, `2`, `u`, or
/// `?`. Renames (`2`) keep the new path in the entry and the old path in the
/// following field, which must be consumed so it is not counted again.
pub fn parse_status(data: &[u8]) -> GitState {
    let mut state = GitState::default();
    let mut fields = data.split(|b| *b == b'\0').peekable();
    // split() always yields a trailing empty field after the final NUL.
    while let Some(field) = fields.next() {
        if field.is_empty() {
            continue;
        }
        if let Some(header) = field.strip_prefix(b"# ".as_slice()) {
            let mut parts = header.splitn(2, |b| *b == b' ');
            match (parts.next(), parts.next()) {
                (Some(b"branch.head"), Some(value)) => {
                    // `(detached)` (or `(detached: …)`) arrives verbatim from
                    // git and is surfaced as the branch label.
                    state.branch = Some(label(value));
                }
                (Some(b"branch.ab"), Some(value)) => {
                    // value is "+A -B"; malformed numbers stay zero.
                    let mut nums = value.split(|b| *b == b' ');
                    state.ahead = nums
                        .next()
                        .and_then(|a| a.strip_prefix(b"+".as_slice()))
                        .and_then(|a| std::str::from_utf8(a).ok())
                        .and_then(|a| a.parse().ok())
                        .unwrap_or(0);
                    state.behind = nums
                        .next()
                        .and_then(|b| b.strip_prefix(b"-".as_slice()))
                        .and_then(|b| std::str::from_utf8(b).ok())
                        .and_then(|b| b.parse().ok())
                        .unwrap_or(0);
                }
                _ => {}
            }
            continue;
        }
        match field.first() {
            Some(b'1' | b'u' | b'?') => state.changed += 1,
            Some(b'2') => {
                // Rename/copy: one entry, plus its preimage in the next field
                // (a path is never empty, so this never eats a separator).
                state.changed += 1;
                fields.next();
            }
            _ => {}
        }
    }
    state
}

/// Parse `git worktree list --porcelain -z` output, keeping paths byte-exact
/// (a non-UTF-8 path survives as-is via `OsStringExt`).
pub fn parse_worktrees(data: &[u8]) -> Vec<PathBuf> {
    const PREFIX: &[u8] = b"worktree ";
    data.split(|b| *b == b'\0')
        .filter_map(|field| field.strip_prefix(PREFIX))
        .map(|bytes| PathBuf::from(std::ffi::OsString::from_vec(bytes.to_vec())))
        .collect()
}

fn git_spec(path: &Path, args: &[&str]) -> CommandSpec {
    CommandSpec {
        program: "git".into(),
        args: args.iter().map(std::ffi::OsString::from).collect(),
        cwd: path.to_owned(),
    }
}

fn brief_error(stderr: &[u8]) -> String {
    let text = String::from_utf8_lossy(stderr);
    let first = text.lines().next().unwrap_or("").trim();
    if first.is_empty() {
        "(no stderr)".to_string()
    } else {
        first.to_string()
    }
}

/// Inspect the repository at `path` without modifying anything.
///
/// A failing `git status` is an error (the directory is not a usable
/// repository), never a silently clean state. Worktree listing failures
/// degrade to an empty list because they are supplementary.
pub fn inspect(path: &Path, runner: &CommandRunner) -> Result<GitState> {
    let status = runner
        .capture(
            &git_spec(
                path,
                &[
                    "--no-optional-locks",
                    "status",
                    "--porcelain=v2",
                    "--branch",
                    "-z",
                ],
            ),
            GIT_TIMEOUT,
            GIT_OUTPUT_LIMIT,
        )
        .with_context(|| format!("running git status in {}", path.display()))?;
    if !status.status.success() {
        bail!(
            "git status failed in {} ({}): {}; is it a git repository?",
            path.display(),
            status.status,
            brief_error(&status.stderr),
        );
    }
    let mut state = parse_status(&status.stdout);

    // Supplementary only: any failure yields no worktrees, not an error.
    state.worktrees = runner
        .capture(
            &git_spec(path, &["worktree", "list", "--porcelain", "-z"]),
            GIT_TIMEOUT,
            GIT_OUTPUT_LIMIT,
        )
        .map(|wt| parse_worktrees(&wt.stdout))
        .unwrap_or_default();
    Ok(state)
}
