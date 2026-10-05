//! Configuration loading for Waystation.
//!
//! Values are read verbatim - program arguments are never shell-split, and
//! paths are expanded here so the rest of the app only sees absolute or
//! workspace-relative forms.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

/// An external program plus its literal arguments (no shell involved).
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ToolCommand {
    pub program: String,
    #[serde(default)]
    pub args: Vec<String>,
}

/// A repeatable command run inside a chosen workspace.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TaskRecipe {
    pub id: String,
    pub label: String,
    pub command: ToolCommand,
    /// `.` stays relative to the selected workspace; `~/` expands to home.
    #[serde(default = "default_cwd", with = "crate::path_serde")]
    pub cwd: PathBuf,
    #[serde(default)]
    pub required_ports: Vec<u16>,
}

/// An SSH port-forward declaration.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TunnelRecipe {
    pub id: String,
    pub host: String,
    #[serde(default = "default_bind")]
    pub bind: String,
    pub local_port: u16,
    pub remote_host: String,
    pub remote_port: u16,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ThemeConfig {
    #[serde(default = "default_accent")]
    pub accent: String,
    #[serde(default)]
    pub compact: bool,
}

impl Default for ThemeConfig {
    fn default() -> Self {
        ThemeConfig {
            accent: default_accent(),
            compact: false,
        }
    }
}

/// Parse-time mirror of [`Config`] that records whether `project_roots` was
/// actually present in the file (absent means "use defaults", an explicit
/// empty list means "no roots").
#[derive(Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct RawConfig {
    project_roots: Option<Vec<PathBuf>>,
    pinned_projects: Vec<PathBuf>,
    editor: Option<ToolCommand>,
    tools: BTreeMap<String, ToolCommand>,
    // Accept legacy task configuration without exposing or loading removed recipes.
    #[serde(rename = "tasks")]
    _legacy_tasks: Option<serde::de::IgnoredAny>,
    tunnels: Vec<TunnelRecipe>,
    theme: ThemeConfig,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    pub project_roots: Vec<PathBuf>,
    pub pinned_projects: Vec<PathBuf>,
    pub editor: Option<ToolCommand>,
    pub tools: BTreeMap<String, ToolCommand>,
    pub tunnels: Vec<TunnelRecipe>,
    pub theme: ThemeConfig,
}

fn default_cwd() -> PathBuf {
    PathBuf::from(".")
}
fn default_bind() -> String {
    "127.0.0.1".into()
}
fn default_accent() -> String {
    "mauve".into()
}

impl Default for Config {
    fn default() -> Self {
        Config {
            project_roots: Vec::new(),
            pinned_projects: Vec::new(),
            editor: None,
            tools: BTreeMap::new(),
            tunnels: Vec::new(),
            theme: ThemeConfig {
                accent: default_accent(),
                compact: false,
            },
        }
    }
}

/// Expand a leading `~/` (or bare `~`) against `home`.
fn expand_tilde(p: &Path, home: &Path) -> PathBuf {
    match p.to_str() {
        Some("~/") | Some("~") => home.to_path_buf(),
        Some(s) if s.starts_with("~/") => home.join(&s[2..]),
        _ => p.to_path_buf(),
    }
}

fn expand_path_field(p: PathBuf, home: &Path, base: Option<&Path>) -> PathBuf {
    let p = expand_tilde(&p, home);
    match base {
        Some(base) if p.is_relative() => base.join(p),
        _ => p,
    }
}

/// Reject empty strings and control characters in identifiers and hosts.
fn check_id(kind: &str, id: &str) -> Result<()> {
    if id.is_empty() {
        bail!("{kind} id must not be empty");
    }
    if id.chars().any(char::is_control) {
        bail!("{kind} id {id:?} contains control characters");
    }
    Ok(())
}

fn check_nonempty(kind: &str, value: &str) -> Result<()> {
    if value.is_empty() {
        bail!("{kind} must not be empty");
    }
    Ok(())
}

fn check_ids(items: &[String]) -> Result<()> {
    let mut seen: Vec<&str> = Vec::new();
    for id in items {
        if seen.contains(&id.as_str()) {
            bail!("duplicate id {id:?}");
        }
        seen.push(id);
    }
    Ok(())
}

fn valid_port(port: u16, kind: &str) -> Result<()> {
    if port == 0 {
        bail!("{kind} port must not be 0");
    }
    Ok(())
}

impl Config {
    /// Built-in defaults for a fresh install; optional paths are included
    /// only when they already exist.
    pub fn defaults(home: &Path) -> Self {
        let mut cfg = Config::default();
        let roots = [home.join("code"), home.join("dotfiles")];
        if roots[0].is_dir() {
            cfg.project_roots.push(roots[0].clone());
        }
        if roots[1].is_dir() {
            cfg.pinned_projects.push(roots[1].clone());
        }
        cfg
    }

    /// Load from `path`. A missing file yields [`Config::defaults`] without
    /// creating anything on disk.
    pub fn load(path: &Path, home: &Path) -> Result<Self> {
        let raw = match fs::read_to_string(path) {
            Ok(s) => s,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                let cfg = Config::defaults(home);
                cfg.validate()?;
                return Ok(cfg);
            }
            Err(e) => return Err(e).with_context(|| format!("reading config {}", path.display())),
        };
        let parsed: RawConfig =
            toml::from_str(&raw).with_context(|| format!("parsing {}", path.display()))?;

        let defaults = Config::defaults(home);
        let mut cfg = Config {
            project_roots: parsed.project_roots.unwrap_or(defaults.project_roots),
            pinned_projects: parsed.pinned_projects,
            editor: parsed.editor,
            tools: parsed.tools,
            tunnels: parsed.tunnels,
            theme: parsed.theme,
        };

        let base = path.parent();
        cfg.project_roots = cfg
            .project_roots
            .into_iter()
            .map(|p| expand_path_field(p, home, base))
            .collect();
        cfg.pinned_projects = cfg
            .pinned_projects
            .into_iter()
            .map(|p| expand_path_field(p, home, base))
            .collect();
        cfg.validate()?;
        Ok(cfg)
    }

    pub(crate) fn validate(&self) -> Result<()> {
        if let Some(editor) = &self.editor {
            check_nonempty("editor program", &editor.program)?;
        }
        for (name, tool) in &self.tools {
            check_id("tool", name)?;
            check_nonempty("tool program", &tool.program)?;
        }
        check_ids(
            &self
                .tunnels
                .iter()
                .map(|t| t.id.clone())
                .collect::<Vec<_>>(),
        )?;
        for tunnel in &self.tunnels {
            check_id("tunnel", &tunnel.id)?;
            check_nonempty("tunnel host", &tunnel.host)?;
            check_nonempty("tunnel remote host", &tunnel.remote_host)?;
            valid_port(tunnel.local_port, "tunnel local")?;
            valid_port(tunnel.remote_port, "tunnel remote")?;
        }
        match self.theme.accent.as_str() {
            "mauve" | "blue" => {}
            other => bail!("invalid theme accent {other:?} (expected mauve or blue)"),
        }
        Ok(())
    }
}

/// Where Waystation keeps its files.
#[derive(Clone, Debug)]
pub struct Paths {
    pub home: PathBuf,
    pub config: PathBuf,
    pub state: PathBuf,
}

fn xdg_dir(env: &str, home: &Path, fallback: &str) -> PathBuf {
    match std::env::var(env).ok().filter(|s| !s.is_empty()) {
        Some(dir) if Path::new(&dir).is_absolute() => PathBuf::from(dir),
        _ => home.join(fallback),
    }
}

fn prefer_current(current: PathBuf, legacy: PathBuf) -> Result<PathBuf> {
    if current.try_exists()? || !legacy.try_exists()? {
        Ok(current)
    } else {
        Ok(legacy)
    }
}

impl Paths {
    fn from_roots(home: PathBuf, config_root: PathBuf, state_root: PathBuf) -> Result<Self> {
        Ok(Self {
            config: prefer_current(
                config_root.join("waystation/config.toml"),
                config_root.join("station/config.toml"),
            )?,
            state: prefer_current(state_root.join("waystation"), state_root.join("station"))?,
            home,
        })
    }

    /// Resolve from `HOME` plus the XDG variables (absolute, non-empty only).
    pub fn discover() -> Result<Self> {
        let home = std::env::var("HOME")
            .context("HOME is not set; cannot locate the Waystation directories")?;
        let home = PathBuf::from(home);
        let config_root = xdg_dir("XDG_CONFIG_HOME", &home, ".config");
        let state_root = xdg_dir("XDG_STATE_HOME", &home, ".local/state");
        Self::from_roots(home, config_root, state_root)
    }
}

impl TaskRecipe {
    pub fn from_tunnel(t: &TunnelRecipe, cwd: PathBuf) -> Result<Self> {
        let valid = |s: &str| {
            !s.is_empty()
                && !s.starts_with('-')
                && !s.chars().any(|c| c.is_control() || c.is_whitespace())
        };
        anyhow::ensure!(
            valid(&t.host)
                && valid(&t.bind)
                && valid(&t.remote_host)
                && t.local_port > 0
                && t.remote_port > 0,
            "Invalid tunnel endpoints"
        );
        let endpoint = |s: &str| {
            if s.contains(':') && !s.starts_with('[') {
                format!("[{s}]")
            } else {
                s.into()
            }
        };
        Ok(Self {
            id: format!("tunnel:{}", t.id),
            label: format!("Tunnel {}", t.id),
            cwd,
            required_ports: vec![t.local_port],
            command: ToolCommand {
                program: "ssh".into(),
                args: vec![
                    "-N".into(),
                    "-o".into(),
                    "BatchMode=yes".into(),
                    "-o".into(),
                    "ExitOnForwardFailure=yes".into(),
                    "-L".into(),
                    format!(
                        "{}:{}:{}:{}",
                        endpoint(&t.bind),
                        t.local_port,
                        endpoint(&t.remote_host),
                        t.remote_port
                    ),
                    t.host.clone(),
                ],
            },
        })
    }
}

#[cfg(test)]
mod paths_tests {
    use super::Paths;
    use std::fs;

    #[test]
    fn fresh_install_uses_waystation_directories() {
        let root = tempfile::tempdir().unwrap();
        let home = root.path().join("home");
        let paths = Paths::from_roots(
            home.clone(),
            home.join(".config"),
            home.join(".local/state"),
        )
        .unwrap();
        assert_eq!(paths.config, home.join(".config/waystation/config.toml"));
        assert_eq!(paths.state, home.join(".local/state/waystation"));
    }

    #[test]
    fn existing_station_data_remains_visible_until_new_paths_exist() {
        let root = tempfile::tempdir().unwrap();
        let home = root.path().join("home");
        let config_root = home.join(".config");
        let state_root = home.join(".local/state");
        fs::create_dir_all(config_root.join("station")).unwrap();
        fs::write(config_root.join("station/config.toml"), "").unwrap();
        fs::create_dir_all(state_root.join("station/agents")).unwrap();

        let paths =
            Paths::from_roots(home.clone(), config_root.clone(), state_root.clone()).unwrap();
        assert_eq!(paths.config, config_root.join("station/config.toml"));
        assert_eq!(paths.state, state_root.join("station"));

        fs::create_dir_all(config_root.join("waystation")).unwrap();
        fs::write(config_root.join("waystation/config.toml"), "").unwrap();
        fs::create_dir_all(state_root.join("waystation")).unwrap();
        let paths = Paths::from_roots(home, config_root.clone(), state_root.clone()).unwrap();
        assert_eq!(paths.config, config_root.join("waystation/config.toml"));
        assert_eq!(paths.state, state_root.join("waystation"));
    }
}
