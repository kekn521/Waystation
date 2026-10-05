use waystation::providers::connections::aliases;
#[test]
fn includes_are_bounded_and_literal() {
    let d = tempfile::tempdir().unwrap();
    std::fs::write(d.path().join("config"),"Host good *.wild !bad -option\nInclude \"extra file\"\nMatch exec never-run\n  User ignored\nHost final\n").unwrap();
    std::fs::write(d.path().join("extra file"), "HOST other\nInclude config\n").unwrap();
    assert_eq!(
        aliases(&d.path().join("config")).unwrap(),
        vec!["final", "good", "other"]
    );
}
