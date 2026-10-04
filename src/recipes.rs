//! UI-created recipes live beside the config, leaving hand-written TOML intact.
use crate::config::{Config, TaskRecipe};
use anyhow::{Result, ensure};
use std::{
    fs::{self, OpenOptions},
    os::unix::fs::OpenOptionsExt,
    path::Path,
};

pub fn load(config: &Path) -> Result<Vec<TaskRecipe>> {
    let dir = config.with_extension("tasks");
    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(vec![]),
        Err(e) => return Err(e.into()),
    };
    let mut paths = entries
        .map(|e| e.map(|e| e.path()))
        .collect::<std::io::Result<Vec<_>>>()?;
    paths.sort();
    let mut recipes = vec![];
    for path in paths {
        if path.extension().is_some_and(|s| s == "json") {
            recipes.push(serde_json::from_slice(&fs::read(path)?)?);
        }
    }
    Ok(recipes)
}
pub fn save(config: &Path, home: &Path, recipe: &TaskRecipe) -> Result<Config> {
    let id = uuid::Uuid::parse_str(&recipe.id)?;
    ensure!(!recipe.label.trim().is_empty(), "Enter a task name");
    ensure!(
        recipe.cwd.is_absolute() && recipe.cwd.is_dir(),
        "Choose an existing project directory"
    );
    let dir = config.with_extension("tasks");
    fs::create_dir_all(&dir)?;
    let lock = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .mode(0o600)
        .open(dir.join("lock"))?;
    lock.lock()?;
    let mut current = Config::load(config, home)?;
    ensure!(
        !current.tasks.iter().any(|r| r.id == recipe.id),
        "Recipe already exists"
    );
    current.tasks.push(recipe.clone());
    current.validate()?;
    crate::store::atomic_json(&dir.join(format!("{id}.json")), recipe)?;
    Ok(current)
}
