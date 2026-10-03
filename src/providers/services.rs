use crate::{
    model::{Availability, Snapshot},
    runtime::command::{CommandRunner, CommandSpec},
};
use anyhow::{Result, ensure};
use std::{path::PathBuf, time::Duration};
#[derive(Clone, Debug)]
pub struct Container {
    pub id: String,
    pub name: String,
    pub state: String,
    pub ports: String,
}
#[derive(Clone, Debug)]
pub struct Listener {
    pub address: String,
    pub port: u16,
    pub protocol: String,
    pub pid: Option<u32>,
    pub command: Option<String>,
    pub cwd: Option<PathBuf>,
}
#[derive(Clone, Debug, Default)]
pub struct ServicesState {
    pub containers: Snapshot<Vec<Container>>,
    pub listeners: Snapshot<Vec<Listener>>,
}
pub fn parse_docker(text: &str) -> Result<Vec<Container>> {
    text.lines()
        .filter(|l| !l.trim().is_empty())
        .take(1000)
        .map(|l| {
            let v: serde_json::Value = serde_json::from_str(l)?;
            let field = |n: &str| v[n].as_str().unwrap_or("").to_string();
            Ok(Container {
                id: field("ID"),
                name: field("Names"),
                state: field("State"),
                ports: field("Ports"),
            })
        })
        .collect()
}
pub fn parse_listeners(text: &str) -> Vec<Listener> {
    text.lines()
        .take(2000)
        .filter_map(|l| {
            let cols = l.split_whitespace().collect::<Vec<_>>();
            if cols.len() < 6 {
                return None;
            }
            let addr = cols[4];
            let (_, port) = addr.rsplit_once(':')?;
            let port = port.parse().ok()?;
            let pid = l
                .split("pid=")
                .nth(1)
                .and_then(|s| s.split(|c: char| !c.is_ascii_digit()).next())
                .and_then(|s| s.parse().ok());
            let command = l
                .split("users:((\"")
                .nth(1)
                .and_then(|s| s.split('"').next())
                .map(Into::into);
            Some(Listener {
                address: addr.into(),
                port,
                protocol: cols[0].into(),
                pid,
                command,
                cwd: None,
            })
        })
        .collect()
}
pub fn required_port_conflicts(ports: &[u16], listeners: &[Listener]) -> Vec<u16> {
    let mut conflicts = ports
        .iter()
        .copied()
        .filter(|p| listeners.iter().any(|l| l.port == *p))
        .collect::<Vec<_>>();
    conflicts.sort_unstable();
    conflicts.dedup();
    conflicts
}
fn query(runner: &CommandRunner, program: &str, args: &[&str]) -> Result<String> {
    let o = runner.capture(
        &CommandSpec {
            program: program.into(),
            args: args.iter().map(Into::into).collect(),
            cwd: std::env::temp_dir(),
        },
        Duration::from_secs(2),
        1024 * 1024,
    )?;
    ensure!(
        o.status.success(),
        "{}: {}",
        program,
        String::from_utf8_lossy(&o.stderr)
    );
    Ok(String::from_utf8_lossy(&o.stdout).into())
}
fn snapshot<T>(r: Result<T>) -> Snapshot<T> {
    match r {
        Ok(v) => Snapshot::ready(v, 0),
        Err(e) => Snapshot {
            availability: Availability::Failed(e.to_string()),
            ..Snapshot::default()
        },
    }
}
pub fn collect(runner: &CommandRunner) -> ServicesState {
    let containers = snapshot(
        query(runner, "docker", &["ps", "--format", "{{json .}}"]).and_then(|s| parse_docker(&s)),
    );
    let listeners = snapshot(query(runner, "ss", &["-H", "-lntup"]).map(|s| {
        let mut rows = parse_listeners(&s);
        for l in &mut rows {
            if let Some(pid) = l.pid {
                l.cwd = std::fs::read_link(format!("/proc/{pid}/cwd")).ok();
            }
        }
        rows
    }));
    ServicesState {
        containers,
        listeners,
    }
}
pub fn logs(id: &str) -> Result<String> {
    ensure!(
        !id.is_empty() && id.bytes().all(|b| b.is_ascii_hexdigit()),
        "Invalid container ID"
    );
    query(&CommandRunner, "docker", &["logs", "--tail", "200", id]).map(|s| crate::ui::safe(&s))
}
