use anyhow::{Result, ensure};
use std::{
    collections::BTreeSet,
    io::Read,
    path::{Path, PathBuf},
};
fn words(s: &str) -> Vec<String> {
    let mut result = vec![];
    let mut word = String::new();
    let mut quote = None;
    let mut escaped = false;
    for c in s.chars() {
        if escaped {
            word.push(c);
            escaped = false;
            continue;
        }
        if c == '\\' {
            escaped = true;
            continue;
        }
        if let Some(q) = quote {
            if c == q {
                quote = None
            } else {
                word.push(c)
            }
            continue;
        }
        match c {
            '"' | '\'' => quote = Some(c),
            '#' => break,
            c if c.is_whitespace() || c == '=' => {
                if !word.is_empty() {
                    result.push(std::mem::take(&mut word))
                }
            }
            _ => word.push(c),
        }
    }
    if !word.is_empty() {
        result.push(word)
    }
    result
}
pub fn aliases(path: &Path) -> Result<Vec<String>> {
    struct Reader {
        seen: BTreeSet<PathBuf>,
        aliases: BTreeSet<String>,
        bytes: usize,
        base: PathBuf,
    }
    impl Reader {
        fn read(&mut self, path: &Path, depth: usize) -> Result<()> {
            if depth > 8 || self.seen.len() >= 32 {
                return Ok(());
            }
            let canon = match path.canonicalize() {
                Ok(p) => p,
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
                Err(e) => return Err(e.into()),
            };
            if !self.seen.insert(canon.clone()) {
                return Ok(());
            }
            let mut text = String::new();
            std::fs::File::open(canon)?
                .take((1024 * 1024 - self.bytes + 1) as u64)
                .read_to_string(&mut text)?;
            self.bytes += text.len();
            ensure!(self.bytes <= 1024 * 1024, "SSH config exceeds 1 MiB");
            for line in text.lines() {
                let parts = words(line);
                let Some(key) = parts.first() else { continue };
                if key.eq_ignore_ascii_case("Host") {
                    for alias in &parts[1..] {
                        if !alias.starts_with('-')
                            && !alias.contains(['*', '?', '!'])
                            && !alias.chars().any(|c| c.is_control() || c.is_whitespace())
                        {
                            self.aliases.insert(alias.clone());
                        }
                    }
                } else if key.eq_ignore_ascii_case("Include") {
                    for include in &parts[1..] {
                        let p = if let Some(p) = include.strip_prefix("~/") {
                            PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(p)
                        } else {
                            self.base.join(include)
                        };
                        for p in glob::glob(&p.to_string_lossy())?.take(32).flatten() {
                            self.read(&p, depth + 1)?
                        }
                    }
                }
            }
            Ok(())
        }
    }
    let mut reader = Reader {
        seen: BTreeSet::new(),
        aliases: BTreeSet::new(),
        bytes: 0,
        base: path.parent().unwrap_or(Path::new(".")).into(),
    };
    reader.read(path, 0)?;
    Ok(reader.aliases.into_iter().collect())
}
