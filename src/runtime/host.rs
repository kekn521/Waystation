//! Runs a program on a PTY inside Waystation's own terminal emulator, so Waystation can draw
//! over it (the floating agent status box) while it runs.
//!
//! Keys and mouse reach the program as the raw bytes the outer terminal sends. Waystation
//! mirrors the program's input modes (cursor keys, bracketed paste, mouse, kitty keyboard
//! flags) onto the outer terminal so those bytes are encoded the way the program asked.
use super::command::CommandSpec;
use alacritty_terminal::{
    event::{Event, EventListener, WindowSize},
    grid::{Dimensions, Scroll},
    index::{Column, Line},
    term::{Config, Term, TermMode, cell::Flags},
    vte::ansi::{Color as AnsiColor, CursorShape, NamedColor, Processor, Rgb},
};
use anyhow::{Context, Result};
use ratatui::{
    backend::{Backend, ClearType, CrosstermBackend, WindowSize as BackendWindowSize},
    buffer::{Buffer, Cell},
    layout::{Position, Size as BackendSize},
    style::{Color, Modifier, Style},
};
use std::{
    io::Write,
    os::fd::{AsFd, AsRawFd, OwnedFd},
    sync::{
        Arc, Mutex, OnceLock,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    time::{Duration, Instant},
};

/// What a chunk of the user's input means to Waystation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Key {
    /// Belongs to the program.
    Forward,
    ToggleOverlay,
    /// Waystation's own key, repeated or released: dropped.
    Swallow,
    /// Pages of history to scroll: negative is back.
    Scroll(i32),
}

/// A `CSI <code> ; <mods>[:<event>] ~` key, as `(code, modifiers, event)`.
fn tilde_key(input: &[u8]) -> Option<(&[u8], &[u8], &[u8])> {
    let body = input.strip_prefix(b"\x1b[")?.strip_suffix(b"~")?;
    let mut parts = body.splitn(2, |&b| b == b';');
    let code = parts.next()?;
    let rest = parts.next().unwrap_or(b"1");
    let mut sub = rest.splitn(2, |&b| b == b':');
    let mods = sub.next()?;
    let event = sub.next().unwrap_or(b"1");
    Some((code, mods, event))
}
/// Classifies one chunk of input. `scrolling` is whether the history can be scrolled now (the
/// program is on its normal screen).
pub fn classify(input: &[u8], scrolling: bool) -> Key {
    let Some((code, mods, event)) = tilde_key(input) else {
        return Key::Forward;
    };
    match (code, mods) {
        (b"20", b"1") if event == b"1" => Key::ToggleOverlay,
        (b"20", b"1") => Key::Swallow,
        (b"5" | b"6", b"2") if scrolling => match event {
            b"1" | b"2" => Key::Scroll(if code == b"5" { -1 } else { 1 }),
            _ => Key::Swallow,
        },
        _ => Key::Forward,
    }
}

/// Splits SGR mouse reports out of `input` while Waystation owns the mouse: wheel notches
/// (negative is up) and the remaining non-mouse bytes. Other mouse events are dropped.
pub fn split_wheel(input: &[u8]) -> (i32, Vec<u8>) {
    let (mut delta, mut rest, mut i) = (0, vec![], 0);
    while i < input.len() {
        if input[i..].starts_with(b"\x1b[<")
            && let Some(end) = input[i + 3..].iter().position(|&b| b == b'M' || b == b'm')
        {
            let report = &input[i + 3..i + 3 + end];
            let button = report.split(|&b| b == b';').next().unwrap_or(b"");
            match button {
                b"64" => delta -= 1,
                b"65" => delta += 1,
                _ => {}
            }
            i += 3 + end + 1;
            continue;
        }
        rest.push(input[i]);
        i += 1;
    }
    (delta, rest)
}

const MIRRORED: [(TermMode, &str); 9] = [
    (TermMode::APP_CURSOR, "?1"),
    (TermMode::BRACKETED_PASTE, "?2004"),
    (TermMode::MOUSE_REPORT_CLICK, "?1000"),
    (TermMode::MOUSE_DRAG, "?1002"),
    (TermMode::MOUSE_MOTION, "?1003"),
    (TermMode::FOCUS_IN_OUT, "?1004"),
    (TermMode::UTF8_MOUSE, "?1005"),
    (TermMode::SGR_MOUSE, "?1006"),
    (TermMode::ALTERNATE_SCROLL, "?1007"),
];
fn kitty_flags(mode: TermMode) -> u32 {
    (mode & TermMode::KITTY_KEYBOARD_PROTOCOL).bits()
        >> TermMode::DISAMBIGUATE_ESC_CODES.bits().trailing_zeros()
}
/// Escape sequences moving the outer terminal's input modes from `old` to `new`.
pub fn mode_sequences(old: TermMode, new: TermMode) -> Vec<u8> {
    let mut out = String::new();
    for (flag, code) in MIRRORED {
        if old.contains(flag) != new.contains(flag) {
            out.push_str(&format!(
                "\x1b[{code}{}",
                if new.contains(flag) { 'h' } else { 'l' }
            ));
        }
    }
    if old.contains(TermMode::APP_KEYPAD) != new.contains(TermMode::APP_KEYPAD) {
        out.push_str(if new.contains(TermMode::APP_KEYPAD) {
            "\x1b="
        } else {
            "\x1b>"
        });
    }
    if kitty_flags(old) != kitty_flags(new) {
        out.push_str(&format!("\x1b[={};1u", kitty_flags(new)));
    }
    out.into_bytes()
}

#[derive(Clone, Default)]
struct Events(Arc<Mutex<Vec<Event>>>);
impl EventListener for Events {
    fn send_event(&self, event: Event) {
        if let Ok(mut events) = self.0.lock() {
            events.push(event);
        }
    }
}
struct Size {
    columns: usize,
    lines: usize,
}
impl Dimensions for Size {
    fn total_lines(&self) -> usize {
        self.lines
    }
    fn screen_lines(&self) -> usize {
        self.lines
    }
    fn columns(&self) -> usize {
        self.columns
    }
}

/// Named colours keep using the outer terminal's palette, so the user's theme applies.
fn color(color: AnsiColor) -> Color {
    match color {
        AnsiColor::Spec(Rgb { r, g, b }) => Color::Rgb(r, g, b),
        AnsiColor::Indexed(i) => Color::Indexed(i),
        AnsiColor::Named(named) => match named {
            NamedColor::Black | NamedColor::DimBlack => Color::Black,
            NamedColor::Red | NamedColor::DimRed => Color::Red,
            NamedColor::Green | NamedColor::DimGreen => Color::Green,
            NamedColor::Yellow | NamedColor::DimYellow => Color::Yellow,
            NamedColor::Blue | NamedColor::DimBlue => Color::Blue,
            NamedColor::Magenta | NamedColor::DimMagenta => Color::Magenta,
            NamedColor::Cyan | NamedColor::DimCyan => Color::Cyan,
            NamedColor::White | NamedColor::DimWhite => Color::Gray,
            NamedColor::BrightBlack => Color::DarkGray,
            NamedColor::BrightRed => Color::LightRed,
            NamedColor::BrightGreen => Color::LightGreen,
            NamedColor::BrightYellow => Color::LightYellow,
            NamedColor::BrightBlue => Color::LightBlue,
            NamedColor::BrightMagenta => Color::LightMagenta,
            NamedColor::BrightCyan => Color::LightCyan,
            NamedColor::BrightWhite => Color::White,
            _ => Color::Reset,
        },
    }
}

/// The hosted program's screen.
pub struct Screen {
    term: Term<Events>,
    parser: Processor,
    events: Events,
}
impl Screen {
    pub fn new(columns: u16, lines: u16) -> Self {
        let events = Events::default();
        let size = Size {
            columns: columns.max(2) as usize,
            lines: lines.max(1) as usize,
        };
        Self {
            term: Term::new(Config::default(), &size, events.clone()),
            parser: Processor::new(),
            events,
        }
    }
    pub fn feed(&mut self, bytes: &[u8]) {
        self.parser.advance(&mut self.term, bytes);
    }
    pub fn resize(&mut self, columns: u16, lines: u16) {
        self.term.resize(Size {
            columns: columns.max(2) as usize,
            lines: lines.max(1) as usize,
        });
    }
    pub fn mode(&self) -> TermMode {
        *self.term.mode()
    }
    /// Pages of history: negative scrolls back.
    pub fn scroll(&mut self, pages: i32) {
        let lines = self.term.screen_lines() as i32 * pages;
        self.term.scroll_display(Scroll::Delta(-lines));
    }
    pub fn scroll_lines(&mut self, lines: i32) {
        self.term.scroll_display(Scroll::Delta(-lines));
    }
    pub fn scroll_to_bottom(&mut self) {
        self.term.scroll_display(Scroll::Bottom);
    }
    fn take_events(&self) -> Vec<Event> {
        self.events
            .0
            .lock()
            .map(|mut e| std::mem::take(&mut *e))
            .unwrap_or_default()
    }
    /// Where the cursor should show, if anywhere.
    pub fn cursor(&self) -> Option<(u16, u16)> {
        let content = self.term.renderable_content();
        if !content.mode.contains(TermMode::SHOW_CURSOR)
            || content.display_offset != 0
            || content.cursor.shape == CursorShape::Hidden
        {
            return None;
        }
        let point = content.cursor.point;
        Some((point.column.0 as u16, point.line.0.max(0) as u16))
    }
    fn cursor_shape(&self) -> CursorShape {
        self.term.renderable_content().cursor.shape
    }
    /// Draws the visible screen (or the scrolled-back history) into `buf`.
    pub fn render(&self, buf: &mut Buffer) {
        let area = buf.area;
        let content = self.term.renderable_content();
        let offset = content.display_offset as i32;
        for cell in content.display_iter {
            if cell
                .flags
                .intersects(Flags::WIDE_CHAR_SPACER | Flags::LEADING_WIDE_CHAR_SPACER)
            {
                continue;
            }
            let y = cell.point.line.0 + offset;
            let x = cell.point.column.0 as u16;
            if y < 0 || y as u16 >= area.height || x >= area.width {
                continue;
            }
            let mut modifier = Modifier::empty();
            for (flag, m) in [
                (Flags::BOLD, Modifier::BOLD),
                (Flags::ITALIC, Modifier::ITALIC),
                (Flags::DIM, Modifier::DIM),
                (Flags::STRIKEOUT, Modifier::CROSSED_OUT),
                (Flags::HIDDEN, Modifier::HIDDEN),
                (Flags::INVERSE, Modifier::REVERSED),
            ] {
                if cell.flags.contains(flag) {
                    modifier |= m;
                }
            }
            if cell.flags.intersects(Flags::ALL_UNDERLINES) {
                modifier |= Modifier::UNDERLINED;
            }
            let mut symbol = cell.c.to_string();
            if let Some(extra) = cell.zerowidth() {
                symbol.extend(extra);
            }
            buf[(area.x + x, area.y + y as u16)]
                .set_symbol(&symbol)
                .set_style(
                    Style::default()
                        .fg(color(cell.fg))
                        .bg(color(cell.bg))
                        .add_modifier(modifier),
                );
        }
    }
    /// The last `n` non-empty lines of the visible screen, trimmed: what a program printed
    /// before exiting, such as an error.
    pub fn last_lines(&self, n: usize) -> String {
        let grid = self.term.grid();
        let mut lines = (0..self.term.screen_lines() as i32)
            .map(|y| {
                let row = &grid[Line(y)];
                (0..self.term.columns())
                    .map(|x| row[Column(x)].c)
                    .collect::<String>()
                    .trim_end()
                    .to_string()
            })
            .filter(|l| !l.trim().is_empty())
            .collect::<Vec<_>>();
        let keep = lines.len().saturating_sub(n);
        lines.drain(..keep);
        lines.join("\n")
    }
}

/// The outer terminal's default foreground and background, asked once so programs that
/// query them (to pick a light or dark theme) get the real answer.
static OUTER_COLORS: OnceLock<(Rgb, Rgb)> = OnceLock::new();
fn parse_rgb(reply: &str) -> Option<Rgb> {
    let hex = reply.split("rgb:").nth(1)?;
    let mut parts = hex.split(['/', '\x07', '\x1b']).take(3).map(|p| {
        let p = &p[..p.len().min(4)];
        u16::from_str_radix(p, 16).ok().map(|v| match p.len() {
            1 => (v * 17) as u8,
            2 => v as u8,
            3 => (v >> 4) as u8,
            _ => (v >> 8) as u8,
        })
    });
    Some(Rgb {
        r: parts.next()??,
        g: parts.next()??,
        b: parts.next()??,
    })
}
fn query_outer_colors() -> (Rgb, Rgb) {
    let fallback = (
        Rgb {
            r: 0xca,
            g: 0xd3,
            b: 0xf5,
        },
        Rgb {
            r: 0x24,
            g: 0x27,
            b: 0x3a,
        },
    );
    let mut out = std::io::stdout();
    // Primary device attributes ends the replies: every terminal answers it.
    if out
        .write_all(b"\x1b]10;?\x1b\\\x1b]11;?\x1b\\\x1b[c")
        .and_then(|_| out.flush())
        .is_err()
    {
        return fallback;
    }
    let stdin = std::io::stdin();
    let deadline = Instant::now() + Duration::from_millis(300);
    let mut reply = Vec::new();
    let answered = |reply: &[u8]| {
        reply
            .windows(3)
            .position(|w| w == b"\x1b[?")
            .is_some_and(|start| reply[start..].contains(&b'c'))
    };
    while Instant::now() < deadline && !answered(&reply) {
        let mut fds = [nix::poll::PollFd::new(
            stdin.as_fd(),
            nix::poll::PollFlags::POLLIN,
        )];
        if nix::poll::poll(&mut fds, nix::poll::PollTimeout::from(50u16)).unwrap_or(0) > 0 {
            let mut chunk = [0u8; 256];
            match nix::unistd::read(stdin.as_fd(), &mut chunk) {
                Ok(n) if n > 0 => reply.extend_from_slice(&chunk[..n]),
                _ => break,
            }
        }
    }
    let text = String::from_utf8_lossy(&reply);
    let pick = |code: &str| {
        text.split("\x1b]")
            .find(|s| s.starts_with(code))
            .and_then(parse_rgb)
    };
    (
        pick("10;").unwrap_or(fallback.0),
        pick("11;").unwrap_or(fallback.1),
    )
}
/// The xterm default for palette entry `index`.
fn palette(index: usize) -> Rgb {
    const BASE: [(u8, u8, u8); 16] = [
        (0, 0, 0),
        (205, 0, 0),
        (0, 205, 0),
        (205, 205, 0),
        (0, 0, 238),
        (205, 0, 205),
        (0, 205, 205),
        (229, 229, 229),
        (127, 127, 127),
        (255, 0, 0),
        (0, 255, 0),
        (255, 255, 0),
        (92, 92, 255),
        (255, 0, 255),
        (0, 255, 255),
        (255, 255, 255),
    ];
    match index {
        0..16 => {
            let (r, g, b) = BASE[index];
            Rgb { r, g, b }
        }
        16..232 => {
            let i = index - 16;
            let level = |v: usize| if v == 0 { 0 } else { (55 + v * 40) as u8 };
            Rgb {
                r: level(i / 36),
                g: level(i / 6 % 6),
                b: level(i % 6),
            }
        }
        _ => {
            let v = (8 + (index.min(255) - 232) * 10) as u8;
            Rgb { r: v, g: v, b: v }
        }
    }
}
fn base64(bytes: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let n = chunk
            .iter()
            .enumerate()
            .fold(0u32, |n, (i, &b)| n | (b as u32) << (16 - 8 * i));
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(TABLE[(n >> (18 - 6 * i) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

/// Reads `fd` on a thread until `stop` is set or the fd closes, sending each chunk.
fn pump(
    fd: OwnedFd,
    stop: Arc<AtomicBool>,
    send: impl Fn(Option<Vec<u8>>) -> bool + Send + 'static,
) -> std::thread::JoinHandle<()> {
    std::thread::spawn(move || {
        let mut buf = [0u8; 65536];
        while !stop.load(Ordering::Relaxed) {
            let mut fds = [nix::poll::PollFd::new(
                fd.as_fd(),
                nix::poll::PollFlags::POLLIN,
            )];
            match nix::poll::poll(&mut fds, nix::poll::PollTimeout::from(50u16)) {
                Ok(0) => continue,
                Err(nix::errno::Errno::EINTR) => continue,
                Err(_) => break,
                Ok(_) => {}
            }
            match nix::unistd::read(fd.as_fd(), &mut buf) {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    if !send(Some(buf[..n].to_vec())) {
                        return;
                    }
                }
            }
        }
        send(None);
    })
}

/// Draws through crossterm but never asks the terminal anything. While hosting, the keyboard
/// stream belongs to the program, so a query's reply would be read as the user's input and the
/// query itself would wait forever.
struct QuietBackend {
    inner: CrosstermBackend<std::io::Stdout>,
    cursor: Position,
}
impl Backend for QuietBackend {
    type Error = std::io::Error;
    fn draw<'a, I>(&mut self, content: I) -> std::io::Result<()>
    where
        I: Iterator<Item = (u16, u16, &'a Cell)>,
    {
        self.inner.draw(content)
    }
    fn hide_cursor(&mut self) -> std::io::Result<()> {
        self.inner.hide_cursor()
    }
    fn show_cursor(&mut self) -> std::io::Result<()> {
        self.inner.show_cursor()
    }
    fn get_cursor_position(&mut self) -> std::io::Result<Position> {
        Ok(self.cursor)
    }
    fn set_cursor_position<P: Into<Position>>(&mut self, position: P) -> std::io::Result<()> {
        self.cursor = position.into();
        self.inner.set_cursor_position(self.cursor)
    }
    fn clear(&mut self) -> std::io::Result<()> {
        self.inner.clear()
    }
    fn clear_region(&mut self, clear_type: ClearType) -> std::io::Result<()> {
        self.inner.clear_region(clear_type)
    }
    fn size(&self) -> std::io::Result<BackendSize> {
        self.inner.size()
    }
    fn window_size(&mut self) -> std::io::Result<BackendWindowSize> {
        self.inner.window_size()
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Backend::flush(&mut self.inner)
    }
}

enum Message {
    Output(Option<Vec<u8>>),
    Input(Vec<u8>),
}
/// How a hosted program ended.
pub struct Outcome {
    pub status: std::process::ExitStatus,
    /// The last lines it left on screen, for error messages.
    pub last_lines: String,
}

/// Runs `spec` hosted until it exits. `overlay` draws over the program's screen while
/// `visible`; F9 flips `visible`.
pub fn run(
    spec: &CommandSpec,
    visible: &mut bool,
    overlay: &mut dyn FnMut(&mut Buffer),
) -> Result<Outcome> {
    let (fg, bg) = *OUTER_COLORS.get_or_init(query_outer_colors);
    let mut terminal = ratatui::Terminal::new(QuietBackend {
        inner: CrosstermBackend::new(std::io::stdout()),
        cursor: Position::ORIGIN,
    })?;
    let size = terminal.size()?;
    let winsize = |columns: u16, lines: u16| nix::pty::Winsize {
        ws_row: lines,
        ws_col: columns,
        ws_xpixel: 0,
        ws_ypixel: 0,
    };
    let pty = nix::pty::openpty(Some(&winsize(size.width, size.height)), None)
        .context("Opening a terminal for the program")?;
    let slave = pty.slave.as_raw_fd();
    let mut command = std::process::Command::new(&spec.program);
    command
        .args(&spec.args)
        .current_dir(&spec.cwd)
        .env("TERM", "xterm-256color")
        .env("COLORTERM", "truecolor");
    // The program talks to Waystation's emulator, not kitty.
    for (key, _) in std::env::vars_os() {
        if key.to_string_lossy().starts_with("KITTY_") {
            command.env_remove(key);
        }
    }
    for stdio in 0..3 {
        let fd = pty.slave.try_clone()?;
        let stdio_fd = std::process::Stdio::from(fd);
        match stdio {
            0 => command.stdin(stdio_fd),
            1 => command.stdout(stdio_fd),
            _ => command.stderr(stdio_fd),
        };
    }
    unsafe {
        use std::os::unix::process::CommandExt;
        command.pre_exec(move || {
            nix::unistd::setsid().map_err(std::io::Error::from)?;
            if nix::libc::ioctl(slave, nix::libc::TIOCSCTTY as _, 0) != 0 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        });
    }
    let mut child = command
        .spawn()
        .with_context(|| format!("Starting {}", spec.program.to_string_lossy()))?;
    // The command holds copies of the slave; the master only sees EOF once all are closed.
    drop(command);
    drop(pty.slave);
    let master = std::fs::File::from(pty.master);
    let mut to_program = master.try_clone()?;

    let (tx, rx) = mpsc::channel();
    let stop = Arc::new(AtomicBool::new(false));
    {
        let tx = tx.clone();
        pump(
            OwnedFd::from(master.try_clone()?),
            stop.clone(),
            move |bytes| tx.send(Message::Output(bytes)).is_ok(),
        );
    }
    let keyboard = {
        let stdin = std::io::stdin().as_fd().try_clone_to_owned()?;
        pump(stdin, stop.clone(), move |bytes| match bytes {
            Some(bytes) => tx.send(Message::Input(bytes)).is_ok(),
            None => false,
        })
    };

    let mut screen = Screen::new(size.width, size.height);
    let mut stdout = std::io::stdout();
    let mut outer = TermMode::empty();
    let mut shape = None;
    let mut current = (size.width, size.height);
    let mut dirty = true;
    let mut last_draw = Instant::now() - Duration::from_secs(1);
    terminal.clear()?;
    let result = (|| -> Result<std::process::ExitStatus> {
        loop {
            match rx.recv_timeout(Duration::from_millis(16)) {
                Ok(Message::Output(Some(bytes))) => {
                    screen.feed(&bytes);
                    dirty = true;
                }
                Ok(Message::Input(bytes)) => {
                    let mode = screen.mode();
                    let normal = !mode.contains(TermMode::ALT_SCREEN);
                    let own_mouse = normal && !mode.intersects(TermMode::MOUSE_MODE);
                    let (wheel, bytes) = if own_mouse {
                        split_wheel(&bytes)
                    } else {
                        (0, bytes)
                    };
                    if wheel != 0 {
                        screen.scroll_lines(wheel * 3);
                        dirty = true;
                    }
                    if bytes.is_empty() {
                        continue;
                    }
                    match classify(&bytes, normal) {
                        Key::ToggleOverlay => {
                            *visible = !*visible;
                            dirty = true;
                        }
                        Key::Swallow => {}
                        Key::Scroll(pages) => {
                            screen.scroll(pages);
                            dirty = true;
                        }
                        Key::Forward => {
                            screen.scroll_to_bottom();
                            to_program.write_all(&bytes)?;
                            dirty = true;
                        }
                    }
                }
                Ok(Message::Output(None)) => {}
                Err(mpsc::RecvTimeoutError::Timeout) => {}
                Err(mpsc::RecvTimeoutError::Disconnected) => {}
            }
            for event in screen.take_events() {
                match event {
                    Event::PtyWrite(text) => to_program.write_all(text.as_bytes())?,
                    Event::Title(title) => {
                        write!(stdout, "\x1b]2;{}\x07", title.replace(['\x07', '\x1b'], ""))?
                    }
                    Event::ResetTitle => write!(stdout, "\x1b]2;\x07")?,
                    Event::Bell => stdout.write_all(b"\x07")?,
                    Event::ClipboardStore(_, text) => {
                        write!(stdout, "\x1b]52;c;{}\x07", base64(text.as_bytes()))?
                    }
                    Event::ColorRequest(index, format) => {
                        let rgb = match index {
                            10 | 12 => fg,
                            11 => bg,
                            i => palette(i),
                        };
                        to_program.write_all(format(rgb).as_bytes())?;
                    }
                    Event::TextAreaSizeRequest(format) => {
                        let pixels = crossterm::terminal::window_size().ok();
                        let size = WindowSize {
                            num_lines: current.1,
                            num_cols: current.0,
                            cell_width: pixels.as_ref().map_or(0, |p| p.width / current.0.max(1)),
                            cell_height: pixels.as_ref().map_or(0, |p| p.height / current.1.max(1)),
                        };
                        to_program.write_all(format(size).as_bytes())?;
                    }
                    _ => {}
                }
            }
            // The outer terminal reports what the program asked for, plus the wheel while
            // Waystation scrolls the history of a program that takes no mouse input.
            let mode = screen.mode();
            let mut want = mode;
            if !mode.contains(TermMode::ALT_SCREEN) && !mode.intersects(TermMode::MOUSE_MODE) {
                want |= TermMode::MOUSE_REPORT_CLICK | TermMode::SGR_MOUSE;
            }
            if want != outer {
                stdout.write_all(&mode_sequences(outer, want))?;
                outer = want;
            }
            let now = crossterm::terminal::size()?;
            if now != current {
                current = now;
                screen.resize(now.0, now.1);
                let ws = winsize(now.0, now.1);
                unsafe { nix::libc::ioctl(master.as_raw_fd(), nix::libc::TIOCSWINSZ, &ws) };
                terminal.autoresize()?;
                dirty = true;
            }
            if let Some(status) = child.try_wait()? {
                // Draw what the program left for anyone looking, then hand back.
                while let Ok(Message::Output(Some(bytes))) =
                    rx.recv_timeout(Duration::from_millis(20))
                {
                    screen.feed(&bytes);
                }
                return Ok(status);
            }
            // The box changes as agents do, even while the program is quiet.
            if *visible && last_draw.elapsed() >= Duration::from_secs(1) {
                dirty = true;
            }
            if dirty && last_draw.elapsed() >= Duration::from_millis(16) {
                dirty = false;
                last_draw = Instant::now();
                let new_shape = screen.cursor_shape();
                if shape != Some(new_shape) {
                    shape = Some(new_shape);
                    let style = match new_shape {
                        CursorShape::Underline => {
                            crossterm::cursor::SetCursorStyle::SteadyUnderScore
                        }
                        CursorShape::Beam => crossterm::cursor::SetCursorStyle::SteadyBar,
                        _ => crossterm::cursor::SetCursorStyle::DefaultUserShape,
                    };
                    crossterm::execute!(stdout, style)?;
                }
                terminal.draw(|frame| {
                    screen.render(frame.buffer_mut());
                    if *visible {
                        overlay(frame.buffer_mut());
                    }
                    if let Some(position) = screen.cursor() {
                        frame.set_cursor_position(position);
                    }
                })?;
            }
        }
    })();
    stop.store(true, Ordering::Relaxed);
    // Waystation reads the keyboard again once this returns.
    let _ = keyboard.join();
    let _ = stdout.write_all(&mode_sequences(outer, TermMode::empty()));
    let _ = crossterm::execute!(stdout, crossterm::cursor::SetCursorStyle::DefaultUserShape);
    let _ = stdout.flush();
    if result.is_err() {
        let _ = child.kill();
        let _ = child.wait();
    }
    Ok(Outcome {
        status: result?,
        last_lines: screen.last_lines(3),
    })
}
