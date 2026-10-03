use crate::model::{ActivityEntry, AppState};
use anyhow::{Context, Result, ensure};
use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    os::unix::fs::OpenOptionsExt,
    path::{Path, PathBuf},
};
pub struct Store {
    pub dir: PathBuf,
}
impl Store {
    pub fn open(dir: PathBuf) -> Result<Self> {
        fs::create_dir_all(&dir)?;
        Ok(Self { dir })
    }
    pub fn load(&self) -> Result<AppState> {
        let path = self.dir.join("state.json");
        match File::open(&path).and_then(|f| {
            let mut bytes = vec![];
            f.take(8 * 1024 * 1024 + 1).read_to_end(&mut bytes)?;
            Ok(bytes)
        }) {
            Ok(bytes) => {
                ensure!(bytes.len() < 8 * 1024 * 1024, "State file too large");
                let state: AppState =
                    serde_json::from_slice(&bytes).context("Reading Station state")?;
                ensure!(
                    state.schema_version == 1,
                    "Unsupported Station state version {}",
                    state.schema_version
                );
                Ok(state)
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(AppState::default()),
            Err(e) => Err(e.into()),
        }
    }
    pub fn merge(&self, state: &AppState) -> Result<()> {
        let lock = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .mode(0o600)
            .open(self.dir.join("state.lock"))?;
        lock.lock()?;
        let mut old = self.load()?;
        if state.selected_workspace.is_some() {
            old.selected_workspace = state.selected_workspace.clone();
        }
        for p in state.recent_workspaces.iter().rev() {
            old.recent_workspaces.retain(|x| x != p);
            old.recent_workspaces.insert(0, p.clone());
        }
        old.recent_workspaces.truncate(100);
        for entry in &state.activity {
            if !old.activity.iter().any(|x| x.id == entry.id) {
                old.activity.push(entry.clone())
            }
        }
        old.activity.sort_by_key(|e| e.at);
        if old.activity.len() > 1000 {
            old.activity.drain(..old.activity.len() - 1000);
        }
        atomic_json(&self.dir.join("state.json"), &old)?;
        Ok(())
    }
    pub fn record_activity(&self, entry: ActivityEntry) -> Result<()> {
        self.merge(&AppState {
            activity: vec![entry],
            ..AppState::default()
        })
    }
}
pub fn atomic_json(path: &Path, value: &impl serde::Serialize) -> Result<()> {
    let parent = path.parent().context("State path has no parent")?;
    fs::create_dir_all(parent)?;
    let tmp = parent.join(format!(".{}.tmp", uuid::Uuid::new_v4()));
    let result = (|| {
        let mut f = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&tmp)?;
        serde_json::to_writer(&mut f, value)?;
        f.write_all(b"\n")?;
        f.sync_all()?;
        fs::rename(&tmp, path)?;
        File::open(parent)?.sync_all()?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(tmp);
    }
    result
}
