//! Terminal-size driven layout selection.

/// Which pane arrangement fits the current terminal.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum LayoutMode {
    Wide,
    Compact,
    Single,
    TooSmall,
}

/// Pick the layout for a terminal of `width` x `height` cells.
pub fn mode(width: u16, height: u16) -> LayoutMode {
    if width < 60 || height < 18 {
        LayoutMode::TooSmall
    } else if width < 90 || height < 30 {
        LayoutMode::Single
    } else if width >= 120 && height >= 38 {
        LayoutMode::Wide
    } else {
        LayoutMode::Compact
    }
}
