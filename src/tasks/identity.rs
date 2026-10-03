use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::{fs, path::Path};
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProcessIdentity {
    pub pid: u32,
    pub start_ticks: u64,
    pub boot_id: String,
}
pub fn read(root: &Path, pid: u32) -> Result<ProcessIdentity> {
    let stat = fs::read_to_string(root.join(pid.to_string()).join("stat"))?;
    let (_, rest) = stat.rsplit_once(") ").context("Malformed process stat")?;
    let start_ticks = rest
        .split_whitespace()
        .nth(19)
        .context("Missing start ticks")?
        .parse()?;
    let boot_id = fs::read_to_string(root.join("sys/kernel/random/boot_id"))?
        .trim()
        .into();
    Ok(ProcessIdentity {
        pid,
        start_ticks,
        boot_id,
    })
}
pub fn matches(identity: &ProcessIdentity) -> bool {
    read(Path::new("/proc"), identity.pid).is_ok_and(|current| current == *identity)
}
