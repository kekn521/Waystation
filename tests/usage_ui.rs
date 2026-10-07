use waystation::{
    app::{Action, App, Effect},
    config::Config,
    model::{AppState, Section},
    usage::{LimitWindow, Limits, StatusLine, TokenTotals, ToolUsage, Usage},
};

const NOW: i64 = 1_791_400_000;

fn tool(five: f64, week: f64, updated_ago: i64, tokens: (u64, u64, u64)) -> ToolUsage {
    ToolUsage {
        limits: Some(Limits {
            five_hour: Some(LimitWindow {
                used_percent: five,
                window_minutes: 300,
                resets_at: NOW + 3600,
            }),
            weekly: Some(LimitWindow {
                used_percent: week,
                window_minutes: 10080,
                resets_at: NOW + 3 * 86400,
            }),
            plan: None,
            updated_at: NOW - updated_ago,
        }),
        tokens: TokenTotals {
            five_hour: tokens.0,
            today: tokens.1,
            week: tokens.2,
        },
    }
}

fn usage(statusline: StatusLine) -> Usage {
    Usage {
        now: NOW,
        claude: Some(tool(23., 41., 240, (1_200_000, 3_400_000, 18_900_000))),
        codex: Some(tool(71., 11., 720, (30_000, 410_000, 2_100_000))),
        statusline,
    }
}

fn app(section: Section) -> App {
    let mut a = App::new(
        Config::default(),
        AppState {
            selected_workspace: Some("/tmp".into()),
            ..AppState::default()
        },
    );
    a.section = section;
    a
}

fn screen(app: &App, w: u16, h: u16) -> String {
    let mut t = ratatui::Terminal::new(ratatui::backend::TestBackend::new(w, h)).unwrap();
    t.draw(|f| {
        waystation::ui::draw(f, app);
    })
    .unwrap();
    t.backend()
        .buffer()
        .content
        .chunks(w as usize)
        .map(|row| row.iter().map(|c| c.symbol()).collect::<String>())
        .collect::<Vec<_>>()
        .join("\n")
}

fn has_row(app: &App, f: impl Fn(&Action) -> bool) -> bool {
    app.agent_items().iter().any(|(_, _, a)| f(a))
}

#[test]
fn agents_view_shows_limits_and_tokens_for_each_tool() {
    let mut a = app(Section::Agents);
    assert!(
        !screen(&a, 100, 32).contains("Usage"),
        "hidden until loaded"
    );
    a.usage = Some(usage(StatusLine::Installed));
    let full = screen(&a, 100, 32);
    for text in [
        "Usage",
        "Claude 4m ago",
        "Codex 12m ago",
        "77% left",
        "59% left",
        "29% left",
        "tokens 5h 1.2M · today 3.4M · 7d 18.9M",
        "tokens 5h 30k · today 410k · 7d 2.1M",
        "New agent session",
    ] {
        assert!(full.contains(text), "missing {text:?} in\n{full}");
    }

    let compact = screen(&a, 100, 18);
    assert!(
        compact.contains("Claude 5h 77% left · wk 59% left · 7d 18.9M")
            && compact.contains("Codex  5h 29% left · wk 89% left · 7d 2.1M"),
        "{compact}"
    );
    assert!(compact.contains("New agent session"), "{compact}");

    let tiny = screen(&a, 100, 12);
    assert!(!tiny.contains("Usage"), "{tiny}");
}

#[test]
fn reset_windows_and_missing_limits_are_explained() {
    let mut a = app(Section::Agents);
    let mut u = usage(StatusLine::Installed);
    u.claude
        .as_mut()
        .unwrap()
        .limits
        .as_mut()
        .unwrap()
        .five_hour = Some(LimitWindow {
        used_percent: 99.,
        window_minutes: 300,
        resets_at: NOW - 60,
    });
    u.codex = u.codex.map(|c| ToolUsage { limits: None, ..c });
    a.usage = Some(u);
    let full = screen(&a, 100, 32);
    assert!(full.contains("reset"), "{full}");
    assert!(!full.contains("1% left"), "{full}");
    assert!(full.contains("appear after Codex's next reply"), "{full}");
    let five = full.lines().find(|l| l.contains("Claude 5h")).unwrap();
    assert!(
        !five.contains("resets"),
        "a reset window has no upcoming reset: {five}"
    );

    let mut u = usage(StatusLine::Missing);
    u.claude = u.claude.map(|c| ToolUsage { limits: None, ..c });
    a.usage = Some(u);
    let full = screen(&a, 100, 32);
    assert!(full.contains("install the status line below"), "{full}");
}

#[test]
fn overview_summarises_limits_in_one_line() {
    let mut a = app(Section::Overview);
    a.usage = Some(usage(StatusLine::Installed));
    let text = screen(&a, 120, 38);
    assert!(
        text.contains("Left  Claude 5h 77% wk 59%") && text.contains("       Codex  5h 29% wk 89%"),
        "too narrow for one line, so one tool per row: {text}"
    );
    let wide = screen(&a, 160, 45);
    assert!(
        wide.contains("Left  Claude 5h 77% wk 59% · Codex 5h 29% wk 89%"),
        "{wide}"
    );
}

#[test]
fn statusline_row_asks_before_installing_and_respects_other_lines() {
    let mut a = app(Section::Agents);
    let install = |action: &Action| matches!(action, Action::InstallStatusLine);
    a.usage = Some(usage(StatusLine::Installed));
    assert!(!has_row(&a, install));

    a.usage = Some(usage(StatusLine::Missing));
    assert!(has_row(&a, install));
    assert!(a.update(Action::InstallStatusLine).is_empty());
    let title = &a.confirmation.as_ref().expect("asks first").title;
    assert!(title.contains("settings.json"), "{title}");
    let effects = a.update(Action::ConfirmChoice(1));
    assert!(
        matches!(effects[..], [Effect::InstallStatusLine]),
        "{effects:?}"
    );

    a.usage = Some(usage(StatusLine::Other("~/bin/my-line".into())));
    assert!(!has_row(&a, install));
    assert!(
        a.agent_items()
            .iter()
            .any(|(label, _, _)| label.contains("won't replace")),
    );

    // No Claude installed: nothing to offer.
    let mut u = usage(StatusLine::Missing);
    u.claude = None;
    a.usage = Some(u);
    assert!(!has_row(&a, install));
}
