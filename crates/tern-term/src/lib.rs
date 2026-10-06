//! GPUI terminal view driven by an arbitrary byte stream (an SSH channel; there
//! is no local PTY). `Terminal` is the model, `TerminalView` the interactive
//! view, `TerminalTheme` the palette.

mod box_drawing;
mod element;
mod find_bar;
mod links;
mod mappings;
mod search;
mod terminal;
mod theme;
mod view;

pub use links::Link;
pub use search::SearchMark;
pub use terminal::{
    CellColor, CellSnapshot, CursorSnapshot, SCROLLBACK_LINES, Terminal, TerminalEvent,
};
pub use theme::TerminalTheme;
pub use view::{TerminalView, paste_bytes};
