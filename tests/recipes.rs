use station::{
    config::{Config, TaskRecipe, ToolCommand},
    recipes,
};
#[test]
fn form_recipes_preserve_config_and_load_after_restart() {
    let d = tempfile::tempdir().unwrap();
    let p = d.path().join("config.toml");
    let original = "# keep my comments\nproject_roots = []\ntasks = []\n[theme]\naccent = 'blue'\n";
    std::fs::write(&p, original).unwrap();
    let r = TaskRecipe {
        id: uuid::Uuid::new_v4().to_string(),
        label: "Checks".into(),
        cwd: d.path().into(),
        required_ports: vec![],
        command: ToolCommand {
            program: "printf".into(),
            args: vec!["%s".into(), "literal $HOME".into()],
        },
    };
    let c = recipes::save(&p, d.path(), &r).unwrap();
    assert_eq!(c.tasks.len(), 1);
    assert_eq!(std::fs::read_to_string(&p).unwrap(), original);
    assert_eq!(
        Config::load(&p, d.path()).unwrap().tasks[0].command.args[1],
        "literal $HOME"
    );
    assert!(recipes::save(&p, d.path(), &r).is_err());
}
#[test]
fn saving_first_recipe_keeps_default_projects() {
    let d = tempfile::tempdir().unwrap();
    std::fs::create_dir(d.path().join("code")).unwrap();
    let p = d.path().join("config.toml");
    let r = TaskRecipe {
        id: uuid::Uuid::new_v4().to_string(),
        label: "Checks".into(),
        cwd: d.path().into(),
        required_ports: vec![],
        command: ToolCommand {
            program: "true".into(),
            args: vec![],
        },
    };
    let c = recipes::save(&p, d.path(), &r).unwrap();
    assert_eq!(c.project_roots, vec![d.path().join("code")]);
    assert_eq!(c.tasks.len(), 1);
    assert!(!p.exists());
}
