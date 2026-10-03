use station::providers::services::*;
#[test]
fn parses_read_only_services() {
    let rows = parse_docker(
        "{\"ID\":\"abc\",\"Names\":\"web\",\"State\":\"running\",\"Ports\":\"8000/tcp\"}\n",
    )
    .unwrap();
    assert_eq!(rows[0].name, "web");
    let l = parse_listeners(
        "tcp LISTEN 0 128 127.0.0.1:8000 0.0.0.0:* users:((\"python\",pid=12,fd=3))\nudp UNCONN 0 0 [::]:53 [::]:*\n",
    );
    assert_eq!(l.len(), 2);
    assert_eq!(l[0].pid, Some(12));
    assert_eq!(l[1].port, 53);
    assert_eq!(l[1].pid, None);
    assert!(required_port_conflicts(&[], &l).is_empty());
    assert_eq!(required_port_conflicts(&[8000, 8000, 123], &l), vec![8000]);
    assert!(parse_docker("denied").is_err());
}
