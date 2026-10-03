//! Workspace discovery driven entirely by configured roots and pins.
//!
//! The scanner never runs subprocesses and never touches `$HOME` implicitly.
//! Explicit paths may be symlinks (canonicalization follows them), but the
//! scan itself never descends through symlinked directories.

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::Result;

use crate::config::Config;

/// A discovered or pinned project directory. `id` is the canonical path; a
/// pin whose directory has disappeared keeps its configured path so the UI
/// can mark it unavailable.
#[derive(Clone, Debug)]
pub struct Workspace {
    pub id: PathBuf,
    pub name: String,
}

/// Descend at most this many levels below an explicit root.
const MAX_DEPTH: u32 = 2;

/// Hard cap so a misconfigured root cannot explode the workspace list.
const MAX_WORKSPACES: usize = 1000;

/// A directory is a workspace when it contains any of these (`.git` counts
/// whether it is a directory or a gitfile).
const MARKERS: [&str; 5] = [
    ".git",
    "Cargo.toml",
    "package.json",
    "pyproject.toml",
    "Makefile",
];

/// Never descend into these; dot-prefixed names are skipped as well.
const SKIP_DIRS: [&str; 6] = [
    "node_modules",
    "target",
    "dist",
    "build",
    "venv",
    "__pycache__",
];

fn is_skipped(name: &str) -> bool {
    name.starts_with('.') || SKIP_DIRS.contains(&name)
}

fn has_marker(dir: &Path) -> bool {
    // symlink_metadata keeps a dangling symlink from failing the whole scan.
    MARKERS
        .iter()
        .any(|m| dir.join(m).symlink_metadata().is_ok())
}

fn workspace_name(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.to_string_lossy().into_owned())
}

/// Unicode case fold for stable alphabetical ordering.
fn fold(name: &str) -> String {
    name.chars().flat_map(char::to_lowercase).collect()
}

#[derive(Default)]
struct Scanner {
    /// Canonical paths (or original paths for missing pins) already recorded.
    seen: HashSet<PathBuf>,
    out: Vec<Workspace>,
}

impl Scanner {
    fn add(&mut self, id: PathBuf) {
        if self.out.len() >= MAX_WORKSPACES {
            return;
        }
        if self.seen.insert(id.clone()) {
            let name = workspace_name(&id);
            self.out.push(Workspace { id, name });
        }
    }

    /// Include an explicitly configured directory, following symlinks in the
    /// path. Silently ignores entries that are not (or no longer) directories.
    fn include(&mut self, dir: &Path) {
        if let Ok(canon) = fs::canonicalize(dir)
            && canon.is_dir()
        {
            self.add(canon);
        }
    }

    /// Scan a root: itself when eligible, otherwise plain descendants down to
    /// `MAX_DEPTH`. Symlinked children are never followed.
    fn scan(&mut self, dir: &Path, depth: u32) {
        if self.out.len() >= MAX_WORKSPACES {
            return;
        }
        if has_marker(dir) {
            self.include(dir);
            return;
        }
        if depth >= MAX_DEPTH {
            return;
        }
        let Ok(entries) = fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            if is_skipped(&entry.file_name().to_string_lossy()) {
                continue;
            }
            // DirEntry::file_type does not resolve symlinks, so linked
            // directories fail the is_dir test and are never scanned.
            if entry.file_type().is_ok_and(|t| t.is_dir()) {
                self.scan(&entry.path(), depth + 1);
                if self.out.len() >= MAX_WORKSPACES {
                    return;
                }
            }
        }
    }
}

/// Collect pinned projects first, then scan every root. Results are sorted
/// alphabetically (case-folded) with pins first, deduplicated by canonical
/// path, and capped at [`MAX_WORKSPACES`].
pub fn discover(config: &Config) -> Result<Vec<Workspace>> {
    let mut scanner = Scanner::default();
    let mut pinned: HashSet<PathBuf> = HashSet::new();

    for pin in &config.pinned_projects {
        let id = match fs::canonicalize(pin) {
            Ok(canon) => {
                // A pin only has to be a directory, not a marked workspace.
                if canon.is_dir() { canon } else { continue }
            }
            // Retained at its original path so the UI can mark it unavailable.
            Err(_) => pin.clone(),
        };
        if scanner.out.len() >= MAX_WORKSPACES {
            break;
        }
        if pinned.insert(id.clone()) {
            scanner.add(id);
        }
    }

    for root in &config.project_roots {
        scanner.scan(root, 0);
    }

    let mut found = scanner.out;
    found.sort_by(|a, b| {
        pinned
            .contains(&b.id)
            .cmp(&pinned.contains(&a.id))
            .then_with(|| fold(&a.name).cmp(&fold(&b.name)))
            .then_with(|| a.id.cmp(&b.id))
    });
    found.truncate(MAX_WORKSPACES);
    Ok(found)
}
