use alacritty_terminal::term::TermMode;
use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Color, Modifier},
};
use waystation::runtime::host::{Key, Screen, classify, mode_sequences, split_wheel};

#[test]
fn f9_toggles_the_box_in_legacy_and_kitty_encodings() {
    for press in [&b"\x1b[20~"[..], b"\x1b[20;1~", b"\x1b[20;1:1~"] {
        assert_eq!(classify(press, false), Key::ToggleOverlay, "{press:?}");
    }
    // Repeats and releases of F9 are swallowed rather than reaching the program.
    for other in [&b"\x1b[20;1:2~"[..], b"\x1b[20;1:3~"] {
        assert_eq!(classify(other, false), Key::Swallow, "{other:?}");
    }
    // F9 with modifiers, and everything else, belongs to the program.
    for forward in [
        &b"\x1b[20;5~"[..],
        b"\x1b[20;2:1~",
        b"a",
        b"\x1b[21~",
        b"\x1b[200~x\x1b[201~",
    ] {
        assert_eq!(classify(forward, false), Key::Forward, "{forward:?}");
    }
}

#[test]
fn shift_page_keys_scroll_history_only_on_the_normal_screen() {
    assert_eq!(classify(b"\x1b[5;2~", true), Key::Scroll(-1));
    assert_eq!(classify(b"\x1b[6;2~", true), Key::Scroll(1));
    assert_eq!(classify(b"\x1b[5;2:1~", true), Key::Scroll(-1));
    assert_eq!(classify(b"\x1b[5;2~", false), Key::Forward);
}

#[test]
fn input_modes_are_mirrored_and_reset() {
    let on = TermMode::BRACKETED_PASTE
        | TermMode::APP_CURSOR
        | TermMode::SGR_MOUSE
        | TermMode::MOUSE_REPORT_CLICK
        | TermMode::DISAMBIGUATE_ESC_CODES
        | TermMode::REPORT_EVENT_TYPES;
    let set = String::from_utf8(mode_sequences(TermMode::empty(), on)).unwrap();
    for seq in [
        "\x1b[?1h",
        "\x1b[?2004h",
        "\x1b[?1000h",
        "\x1b[?1006h",
        "\x1b[=3;1u",
    ] {
        assert!(set.contains(seq), "{set:?} lacks {seq:?}");
    }
    assert!(!set.contains("1003"), "{set:?}");
    let reset = String::from_utf8(mode_sequences(on, TermMode::empty())).unwrap();
    for seq in [
        "\x1b[?1l",
        "\x1b[?2004l",
        "\x1b[?1000l",
        "\x1b[?1006l",
        "\x1b[=0;1u",
    ] {
        assert!(reset.contains(seq), "{reset:?} lacks {seq:?}");
    }
    assert!(mode_sequences(on, on).is_empty());
}

#[test]
fn wheel_events_are_taken_and_other_input_is_kept() {
    // Two wheel-ups, a click (dropped while Waystation owns the mouse), then a key.
    let (delta, rest) = split_wheel(b"\x1b[<64;10;5M\x1b[<64;10;5M\x1b[<0;3;3Mq");
    assert_eq!((delta, rest.as_slice()), (-2, &b"q"[..]));
    let (delta, rest) = split_wheel(b"\x1b[<65;1;1M");
    assert_eq!((delta, rest.as_slice()), (1, &b""[..]));
}

fn draw(screen: &Screen, w: u16, h: u16) -> Buffer {
    let mut buf = Buffer::empty(Rect::new(0, 0, w, h));
    screen.render(&mut buf);
    buf
}

#[test]
fn screen_draws_text_colours_and_attributes() {
    let mut screen = Screen::new(20, 4);
    screen.feed(b"plain \x1b[31mred\x1b[0m \x1b[1;4mbold\x1b[0m\r\n\x1b[7minv\x1b[0m \x1b[38;2;1;2;3mrgb\x1b[0m wide:\xe5\xbd\xa2");
    let buf = draw(&screen, 20, 4);
    let row: String = (0..20).map(|x| buf[(x, 0)].symbol().to_string()).collect();
    assert_eq!(row.trim_end(), "plain red bold");
    assert_eq!(buf[(6, 0)].fg, Color::Red);
    assert_eq!(buf[(0, 0)].fg, Color::Reset);
    assert!(
        buf[(10, 0)]
            .modifier
            .contains(Modifier::BOLD | Modifier::UNDERLINED)
    );
    // Inverse swaps colours, using the terminal's own colours for defaults.
    assert!(buf[(0, 1)].modifier.contains(Modifier::REVERSED));
    assert_eq!(buf[(4, 1)].fg, Color::Rgb(1, 2, 3));
    assert_eq!(buf[(13, 1)].symbol(), "形");
    assert_eq!(screen.cursor(), Some((15, 1)));
}

#[test]
fn scrolling_back_shows_history_and_new_output_keeps_the_view() {
    let mut screen = Screen::new(10, 2);
    screen.feed(b"one\r\ntwo\r\nthree\r\nfour");
    let first = |s: &Screen| {
        let buf = draw(s, 10, 2);
        (0..10)
            .map(|x| buf[(x, 0)].symbol().to_string())
            .collect::<String>()
    };
    assert_eq!(first(&screen).trim_end(), "three");
    screen.scroll(-2);
    assert_eq!(first(&screen).trim_end(), "one");
    assert_eq!(screen.cursor(), None, "no cursor while scrolled back");
    screen.scroll_to_bottom();
    assert_eq!(first(&screen).trim_end(), "three");
}

#[test]
fn last_lines_summarise_the_screen() {
    let mut screen = Screen::new(30, 5);
    screen.feed(b"\r\nno server running on /tmp/x\r\n\r\n");
    assert_eq!(screen.last_lines(3), "no server running on /tmp/x");
}

/// Runs `__host` in a private tmux server so it has a real terminal, and drives it with keys.
struct Hosted {
    socket: String,
}
impl Hosted {
    fn start(state: &std::path::Path, script: &str) -> Self {
        let socket = format!("ws-host-{}", uuid::Uuid::new_v4());
        let exe = env!("CARGO_BIN_EXE_waystation");
        let command = format!(
            "'{exe}' __host --state-dir '{}' -- sh -c '{script}'; echo HOST_EXIT=$?; sleep 30",
            state.display()
        );
        let ok = std::process::Command::new("tmux")
            .args([
                "-L",
                &socket,
                "-f",
                "/dev/null",
                "new-session",
                "-d",
                "-x",
                "100",
                "-y",
                "30",
                &command,
            ])
            .env_remove("TMUX")
            .status()
            .unwrap()
            .success();
        assert!(ok);
        Self { socket }
    }
    fn tmux(&self, args: &[&str]) -> String {
        let out = std::process::Command::new("tmux")
            .args(["-L", &self.socket])
            .args(args)
            .output()
            .unwrap();
        String::from_utf8_lossy(&out.stdout).into_owned()
    }
    fn screen(&self) -> String {
        self.tmux(&["capture-pane", "-p"])
    }
    fn wait_for(&self, check: impl Fn(&str) -> bool) -> String {
        let mut text = String::new();
        for _ in 0..80 {
            text = self.screen();
            if check(&text) {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        text
    }
}
impl Drop for Hosted {
    fn drop(&mut self) {
        self.tmux(&["kill-server"]);
    }
}

#[test]
fn hosted_program_shows_output_takes_input_and_returns_its_status() {
    let d = tempfile::tempdir().unwrap();
    let host = Hosted::start(
        d.path(),
        r#"printf "HOSTED_OK\n"; read x; echo "got:$x"; sleep 1; exit 3"#,
    );
    let text = host.wait_for(|t| t.contains("HOSTED_OK"));
    assert!(text.contains("HOSTED_OK"), "{text}");
    host.tmux(&["send-keys", "-l", "typed here"]);
    host.tmux(&["send-keys", "Enter"]);
    // Checked while the program still runs: leaving the host restores the screen.
    let text = host.wait_for(|t| t.contains("got:"));
    assert!(text.contains("got:typed here"), "{text}");
    let text = host.wait_for(|t| t.contains("HOST_EXIT="));
    assert!(text.contains("HOST_EXIT=3"), "{text}");
}

#[test]
fn agent_box_floats_over_the_program_and_f9_toggles_it() {
    let d = tempfile::tempdir().unwrap();
    let agents = d.path().join("agents");
    std::fs::create_dir_all(&agents).unwrap();
    std::fs::write(
        agents.join("server.json"),
        format!("\"{}\"", uuid::Uuid::new_v4()),
    )
    .unwrap();
    let id = uuid::Uuid::new_v4();
    let record = serde_json::json!({
        "id": id, "name": "docs-agent", "tool": "claude", "workspace": "/tmp",
        "command": {"program": "claude", "args": []}, "executable": "/usr/bin/claude",
        "created": {"secs_since_epoch": 1, "nanos_since_epoch": 0},
    });
    std::fs::write(agents.join(format!("{id}.json")), record.to_string()).unwrap();
    let host = Hosted::start(d.path(), r#"printf "underneath\n"; sleep 30"#);
    let shown = host.wait_for(|t| t.contains("Agents · F9") && t.contains("docs-agent"));
    assert!(
        shown.contains("Agents · F9") && shown.contains("docs-agent"),
        "{shown}"
    );
    assert!(shown.contains("underneath"), "{shown}");
    host.tmux(&["send-keys", "F9"]);
    let hidden = host.wait_for(|t| !t.contains("Agents · F9"));
    assert!(!hidden.contains("docs-agent"), "{hidden}");
    host.tmux(&["send-keys", "F9"]);
    let back = host.wait_for(|t| t.contains("Agents · F9"));
    assert!(back.contains("docs-agent"), "{back}");
}

#[test]
fn hosted_program_follows_terminal_resizes() {
    let d = tempfile::tempdir().unwrap();
    let host = Hosted::start(d.path(), r#"printf "READY\n"; read x; stty size; sleep 1"#);
    host.wait_for(|t| t.contains("READY"));
    host.tmux(&["resize-window", "-x", "120", "-y", "40"]);
    std::thread::sleep(std::time::Duration::from_millis(200));
    host.tmux(&["send-keys", "Enter"]);
    let text = host.wait_for(|t| t.contains("40 120"));
    assert!(text.contains("40 120"), "{text}");
}
