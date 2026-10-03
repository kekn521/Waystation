use anyhow::{Context, Result};
use std::{
    fs,
    path::Path,
    time::{Duration, Instant},
};
#[derive(Clone, Debug)]
pub struct Process {
    pub pid: u32,
    pub name: String,
    pub memory: u64,
}
#[derive(Clone, Debug, Default)]
pub struct SystemStats {
    pub cpu: Option<f64>,
    pub memory_used: u64,
    pub memory_total: u64,
    pub disk_used: u64,
    pub disk_total: u64,
    pub network_rate: Option<(f64, f64)>,
    pub gpu: Option<String>,
    pub processes: Vec<Process>,
}
#[derive(Default)]
pub struct SystemSampler {
    previous: Option<(u64, u64, u64, u64, Instant)>,
}
impl SystemSampler {
    pub fn sample(&mut self, proc_root: &Path, home: &Path) -> Result<SystemStats> {
        let stat = fs::read_to_string(proc_root.join("stat"))?;
        let nums = stat
            .lines()
            .next()
            .context("Missing CPU counters")?
            .split_whitespace()
            .skip(1)
            .take(8)
            .map(|s| s.parse::<u64>().unwrap_or(0))
            .collect::<Vec<_>>();
        let total = nums.iter().sum::<u64>();
        let idle = nums.get(3).unwrap_or(&0) + nums.get(4).unwrap_or(&0);
        let mem = fs::read_to_string(proc_root.join("meminfo"))?;
        let field = |name: &str| {
            mem.lines()
                .find_map(|s| s.strip_prefix(name))
                .and_then(|s| s.split_whitespace().next())
                .and_then(|s| s.parse::<u64>().ok())
                .unwrap_or(0)
                * 1024
        };
        let memory_total = field("MemTotal:");
        let memory_used = memory_total.saturating_sub(field("MemAvailable:"));
        let mut rx = 0u64;
        let mut tx = 0u64;
        let net = fs::read_to_string(proc_root.join("net/dev"))?;
        for line in net.lines() {
            if let Some((name, values)) = line.split_once(':') {
                if name.trim() == "lo" {
                    continue;
                }
                let n = values.split_whitespace().collect::<Vec<_>>();
                rx = rx.saturating_add(n.first().and_then(|s| s.parse::<u64>().ok()).unwrap_or(0));
                tx = tx.saturating_add(n.get(8).and_then(|s| s.parse::<u64>().ok()).unwrap_or(0));
            }
        }
        let now = Instant::now();
        let mut cpu = None;
        let mut network_rate = None;
        if let Some((pt, pi, pr, px, time)) = self.previous {
            let seconds = now.duration_since(time).as_secs_f64();
            if total > pt && idle >= pi && idle - pi <= total - pt {
                cpu = Some(100. * (total - pt - (idle - pi)) as f64 / (total - pt) as f64)
            }
            if seconds > 0. && rx >= pr && tx >= px {
                network_rate = Some(((rx - pr) as f64 / seconds, (tx - px) as f64 / seconds));
            }
        }
        self.previous = Some((total, idle, rx, tx, now));
        let disk = nix::sys::statvfs::statvfs(home)?;
        let disk_total = disk.blocks() * disk.fragment_size();
        let disk_used = disk_total.saturating_sub(disk.blocks_available() * disk.fragment_size());
        let mut processes = vec![];
        for e in fs::read_dir(proc_root)?.take(16384).flatten() {
            let Some(pid) = e.file_name().to_str().and_then(|s| s.parse().ok()) else {
                continue;
            };
            if let Ok(s) = fs::read_to_string(e.path().join("status")) {
                let name = s
                    .lines()
                    .find_map(|l| l.strip_prefix("Name:"))
                    .unwrap_or("?")
                    .trim()
                    .into();
                let memory = s
                    .lines()
                    .find_map(|l| l.strip_prefix("VmRSS:"))
                    .and_then(|s| s.split_whitespace().next())
                    .and_then(|s| s.parse::<u64>().ok())
                    .unwrap_or(0)
                    * 1024;
                processes.push(Process { pid, name, memory });
            }
        }
        processes.sort_by_key(|p| std::cmp::Reverse(p.memory));
        processes.truncate(100);
        let gpu = if proc_root == Path::new("/proc") {
            use crate::runtime::command::{CommandRunner, CommandSpec};
            CommandRunner
                .capture(
                    &CommandSpec {
                        program: "nvidia-smi".into(),
                        args: [
                            "--query-gpu=name,utilization.gpu,memory.used,memory.total",
                            "--format=csv,noheader,nounits",
                        ]
                        .into_iter()
                        .map(Into::into)
                        .collect(),
                        cwd: home.into(),
                    },
                    Duration::from_millis(700),
                    4096,
                )
                .ok()
                .filter(|o| o.status.success())
                .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        } else {
            None
        };
        Ok(SystemStats {
            cpu,
            memory_used,
            memory_total,
            disk_used,
            disk_total,
            network_rate,
            gpu,
            processes,
        })
    }
}
