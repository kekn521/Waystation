use anyhow::Result;
use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Seek, SeekFrom, Write},
    os::unix::fs::OpenOptionsExt,
    path::{Path, PathBuf},
};
const CAP: u64 = 5 * 1024 * 1024;
pub struct LogWriter {
    dir: PathBuf,
    file: File,
    size: u64,
}
impl LogWriter {
    pub fn new(dir: &Path) -> Result<Self> {
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .mode(0o600)
            .open(dir.join("output.log"))?;
        let size = file.metadata()?.len();
        Ok(Self {
            dir: dir.into(),
            file,
            size,
        })
    }
    pub fn append(&mut self, stream: &str, bytes: &[u8]) -> Result<()> {
        if bytes.is_empty() {
            return Ok(());
        }
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_secs();
        let header = format!("\n[{now} {stream}] ");
        for part in [header.as_bytes(), bytes] {
            for chunk in part.chunks(8192) {
                if self.size + chunk.len() as u64 > CAP {
                    let _ = fs::remove_file(self.dir.join("output.3.log"));
                    for i in (1..=2).rev() {
                        let old = self.dir.join(format!("output.{i}.log"));
                        if old.exists() {
                            fs::rename(old, self.dir.join(format!("output.{}.log", i + 1)))?
                        }
                    }
                    fs::rename(self.dir.join("output.log"), self.dir.join("output.1.log"))?;
                    self.file = OpenOptions::new()
                        .create_new(true)
                        .write(true)
                        .mode(0o600)
                        .open(self.dir.join("output.log"))?;
                    self.size = 0;
                }
                self.file.write_all(chunk)?;
                self.size += chunk.len() as u64;
            }
        }
        self.file.flush()?;
        Ok(())
    }
}
pub fn tail(dir: &Path, limit: usize) -> Result<String> {
    let limit = limit.min(64 * 1024);
    let mut file = File::open(dir.join("output.log"))?;
    let size = file.metadata()?.len();
    file.seek(SeekFrom::Start(size.saturating_sub(limit as u64)))?;
    let mut buf = vec![];
    file.take(limit as u64).read_to_end(&mut buf)?;
    let text = crate::ui::safe(&String::from_utf8_lossy(&buf));
    let marker = if size > limit as u64 || dir.join("output.1.log").exists() {
        "[tail · older output truncated or rotated]\n"
    } else {
        ""
    };
    let marker = if marker.len() < limit { marker } else { "" };
    let budget = limit.saturating_sub(marker.len());
    let mut start = text.len().saturating_sub(budget);
    while !text.is_char_boundary(start) {
        start += 1;
    }
    Ok(format!("{marker}{}", &text[start..]))
}
