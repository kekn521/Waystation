use anyhow::Result;
use std::path::{Path, PathBuf};
#[derive(Clone, Debug)]
pub struct FileEntry {
    pub path: PathBuf,
    pub is_dir: bool,
    pub label: String,
}
pub fn list(root: &Path, dir: &Path, hidden: bool) -> Result<Vec<FileEntry>> {
    let root = root.canonicalize()?;
    let dir = dir.canonicalize()?;
    anyhow::ensure!(
        dir.starts_with(&root),
        "Directory is outside the selected workspace"
    );
    let mut entries = vec![];
    if dir != root {
        entries.push(FileEntry {
            path: dir.parent().unwrap_or(&root).to_owned(),
            is_dir: true,
            label: "..".into(),
        })
    }
    for entry in std::fs::read_dir(&dir)?.take(10_000) {
        let entry = entry?;
        let name = entry.file_name();
        if !hidden && name.to_string_lossy().starts_with('.') {
            continue;
        }
        let path = entry.path();
        let Ok(canonical) = path.canonicalize() else {
            continue;
        };
        if !canonical.starts_with(&root) {
            continue;
        }
        let is_dir = canonical.is_dir();
        entries.push(FileEntry {
            path: canonical,
            is_dir,
            label: name.to_string_lossy().into_owned(),
        });
    }
    entries.sort_by(|a, b| {
        b.is_dir
            .cmp(&a.is_dir)
            .then_with(|| a.label.to_lowercase().cmp(&b.label.to_lowercase()))
    });
    Ok(entries)
}
