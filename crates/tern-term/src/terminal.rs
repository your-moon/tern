// Adapted from zeron crates/ui/src/terminal/emulator.rs (MIT) and zed
// crates/terminal/src/terminal.rs (GPL-3.0-or-later).
//
//! The terminal model: `alacritty_terminal`'s `Term` + vte's ANSI `Processor`,
//! driven by bytes (no PTY, no event loop thread).
//!
//! Bytes in via [`Terminal::feed`]; grid snapshots out via [`Terminal::lines`] /
//! [`Terminal::cursor`]. Replies the emulator wants sent to the remote side
//! (DSR, DA, color/size queries) leave through the `write` callback given to
//! [`Terminal::new`].

use std::cell::RefCell;
use std::rc::Rc;

use alacritty_terminal::event::{Event, EventListener, WindowSize};
use alacritty_terminal::grid::{Dimensions, Scroll};
use alacritty_terminal::index::{Column, Line, Point};
use alacritty_terminal::selection::{Selection, SelectionRange};
use alacritty_terminal::term::cell::Flags;
use alacritty_terminal::term::{Config, Term, TermMode};
use alacritty_terminal::vte::ansi::{
    Color as AnsiColor, CursorShape, NamedColor, Processor, Rgb as AnsiRgb,
};
use gpui::{Context, EventEmitter};

use crate::theme::TerminalTheme;

pub use alacritty_terminal::index::Side;
pub use alacritty_terminal::selection::SelectionType;

/// Scrollback history kept client-side (lines).
pub const SCROLLBACK_LINES: usize = 10_000;

/// Emitted by [`Terminal`] (it is an `EventEmitter`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TerminalEvent {
    /// OSC 0/2 title change. An empty string means "reset to default".
    TitleChanged(String),
    Bell,
}

/// Viewport dimensions in cells.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct GridSize {
    cols: u16,
    rows: u16,
}

impl GridSize {
    fn new(cols: u16, rows: u16) -> Self {
        Self {
            cols: cols.max(2),
            rows: rows.max(1),
        }
    }
}

impl Dimensions for GridSize {
    fn total_lines(&self) -> usize {
        self.rows as usize
    }
    fn screen_lines(&self) -> usize {
        self.rows as usize
    }
    fn columns(&self) -> usize {
        self.cols as usize
    }
}

/// A cell's paint color, decoupled from the palette; the element resolves
/// these against the [`TerminalTheme`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CellColor {
    Foreground,
    Background,
    /// 0-15 ANSI, 16-231 color cube, 232-255 grayscale ramp.
    Indexed(u8),
    Rgb(u8, u8, u8),
}

fn map_color(color: AnsiColor) -> CellColor {
    match color {
        AnsiColor::Spec(AnsiRgb { r, g, b }) => CellColor::Rgb(r, g, b),
        AnsiColor::Indexed(ix) => CellColor::Indexed(ix),
        AnsiColor::Named(named) => {
            let ix = named as usize;
            if ix < 16 {
                return CellColor::Indexed(ix as u8);
            }
            match named {
                NamedColor::Background => CellColor::Background,
                // Dim named colors fold onto their base index; DIM still
                // travels on the cell for paint-time dimming.
                NamedColor::DimBlack
                | NamedColor::DimRed
                | NamedColor::DimGreen
                | NamedColor::DimYellow
                | NamedColor::DimBlue
                | NamedColor::DimMagenta
                | NamedColor::DimCyan
                | NamedColor::DimWhite => {
                    CellColor::Indexed((ix - NamedColor::DimBlack as usize) as u8)
                }
                _ => CellColor::Foreground,
            }
        }
    }
}

/// One rendered cell: char + colors + the flags paint cares about.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CellSnapshot {
    pub ch: char,
    pub fg: CellColor,
    pub bg: CellColor,
    pub bold: bool,
    pub dim: bool,
    pub italic: bool,
    pub underline: bool,
    pub strikeout: bool,
    pub inverse: bool,
    pub hidden: bool,
    /// Double-width char (occupies this cell plus the next spacer cell).
    pub wide: bool,
    /// Spacer half of a wide char: never shaped, only background-painted.
    pub wide_spacer: bool,
    pub selected: bool,
}

impl CellSnapshot {
    /// Effective (fg, bg) after INVERSE/HIDDEN resolution.
    pub fn display_colors(&self) -> (CellColor, CellColor) {
        let (fg, bg) = if self.inverse {
            (self.bg, self.fg)
        } else {
            (self.fg, self.bg)
        };
        if self.hidden { (bg, bg) } else { (fg, bg) }
    }
}

/// Cursor position in viewport coordinates (row 0 = top of the visible grid).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CursorSnapshot {
    pub row: usize,
    pub col: usize,
    pub shape: CursorShape,
}

/// Captures `Term` callbacks. `EventListener::send_event` takes `&self`;
/// single-threaded (the model lives inside a gpui entity).
#[derive(Default, Clone)]
struct EventCapture {
    events: Rc<RefCell<Vec<Event>>>,
}

impl EventListener for EventCapture {
    fn send_event(&self, event: Event) {
        self.events.borrow_mut().push(event);
    }
}

/// Side effects of a [`Terminal::process`] call that need a gpui context.
#[derive(Debug, PartialEq, Eq)]
enum Notice {
    Event(TerminalEvent),
    ClipboardStore(String),
}

pub struct Terminal {
    term: Term<EventCapture>,
    parser: Processor,
    capture: EventCapture,
    write: Box<dyn Fn(Vec<u8>) + 'static>,
    resize: Box<dyn Fn(u16, u16, u16, u16) + 'static>,
    theme: TerminalTheme,
    /// Cell size in pixels, for text-area size queries and the resize callback.
    cell_px: (f32, f32),
}

impl std::fmt::Debug for Terminal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Terminal")
            .field("cols", &self.cols())
            .field("rows", &self.rows())
            .field("history", &self.history_size())
            .finish_non_exhaustive()
    }
}

impl EventEmitter<TerminalEvent> for Terminal {}

impl Terminal {
    pub fn new(
        cols: u16,
        rows: u16,
        write: Box<dyn Fn(Vec<u8>) + 'static>,
        resize: Box<dyn Fn(u16, u16, u16, u16) + 'static>,
    ) -> Self {
        let capture = EventCapture::default();
        let config = Config {
            scrolling_history: SCROLLBACK_LINES,
            ..Config::default()
        };
        let term = Term::new(config, &GridSize::new(cols, rows), capture.clone());
        Self {
            term,
            parser: Processor::new(),
            capture,
            write,
            resize,
            theme: TerminalTheme::default(),
            cell_px: (0.0, 0.0),
        }
    }

    /// Advance the parser over bytes from the remote side, write any replies
    /// back through `write`, and repaint.
    pub fn feed(&mut self, bytes: &[u8], cx: &mut Context<Self>) {
        for notice in self.process(bytes) {
            match notice {
                Notice::Event(event) => cx.emit(event),
                Notice::ClipboardStore(text) => {
                    cx.write_to_clipboard(gpui::ClipboardItem::new_string(text))
                }
            }
        }
        cx.notify();
    }

    /// The context-free core of [`Self::feed`]: runs the parser, sends replies
    /// through `write`, and returns what still needs a gpui context.
    fn process(&mut self, bytes: &[u8]) -> Vec<Notice> {
        self.parser.advance(&mut self.term, bytes);
        let mut reply = Vec::new();
        let mut notices = Vec::new();
        let events: Vec<Event> = self.capture.events.borrow_mut().drain(..).collect();
        for event in events {
            match event {
                Event::PtyWrite(text) => reply.extend_from_slice(text.as_bytes()),
                Event::Title(title) => {
                    notices.push(Notice::Event(TerminalEvent::TitleChanged(title)))
                }
                Event::ResetTitle => {
                    notices.push(Notice::Event(TerminalEvent::TitleChanged(String::new())))
                }
                Event::Bell => notices.push(Notice::Event(TerminalEvent::Bell)),
                Event::ClipboardStore(_, text) => notices.push(Notice::ClipboardStore(text)),
                // OSC 10/11/12 and OSC 4 queries: vim/neovim use these to pick
                // a light or dark scheme.
                Event::ColorRequest(index, format) => {
                    if let Some((r, g, b)) = self.theme.rgb_for_request(index) {
                        reply.extend_from_slice(format(AnsiRgb { r, g, b }).as_bytes());
                    }
                }
                Event::TextAreaSizeRequest(format) => {
                    let size = WindowSize {
                        num_lines: self.rows() as u16,
                        num_cols: self.cols() as u16,
                        cell_width: self.cell_px.0 as u16,
                        cell_height: self.cell_px.1 as u16,
                    };
                    reply.extend_from_slice(format(size).as_bytes());
                }
                // ClipboardLoad is deliberately ignored: letting the remote
                // read the local clipboard is a data leak.
                _ => {}
            }
        }
        if !reply.is_empty() {
            (self.write)(reply);
        }
        notices
    }

    /// Set the palette used to answer color queries.
    pub fn set_theme(&mut self, theme: &TerminalTheme) {
        self.theme = theme.clone();
    }

    /// Send bytes to the remote side (keyboard, paste, mouse reports).
    pub fn write(&self, bytes: Vec<u8>) {
        (self.write)(bytes);
    }

    /// Resize the local grid. Returns whether the size changed. The remote
    /// side is told separately via [`Self::notify_remote_resize`] so callers
    /// can debounce it.
    pub fn resize(&mut self, cols: u16, rows: u16, cell_w: f32, cell_h: f32) -> bool {
        self.cell_px = (cell_w, cell_h);
        let size = GridSize::new(cols, rows);
        if size.cols as usize == self.cols() && size.rows as usize == self.rows() {
            return false;
        }
        self.term.resize(size);
        true
    }

    /// Invoke the `resize` callback with the current size (cols, rows,
    /// pixel width, pixel height).
    pub fn notify_remote_resize(&self) {
        let (cols, rows) = (self.cols() as u16, self.rows() as u16);
        let px_w = (self.cell_px.0 * cols as f32) as u16;
        let px_h = (self.cell_px.1 * rows as f32) as u16;
        (self.resize)(cols, rows, px_w, px_h);
    }

    pub fn cols(&self) -> usize {
        self.term.columns()
    }

    pub fn rows(&self) -> usize {
        self.term.screen_lines()
    }

    pub fn mode(&self) -> TermMode {
        *self.term.mode()
    }

    /// Lines scrolled back into history (0 = pinned to the live bottom).
    pub fn display_offset(&self) -> usize {
        self.term.grid().display_offset()
    }

    /// Lines available above the viewport.
    pub fn history_size(&self) -> usize {
        self.term.grid().history_size()
    }

    /// Scroll the view: positive = up into history, negative = toward live.
    pub fn scroll(&mut self, delta: i32) {
        self.term.scroll_display(Scroll::Delta(delta));
    }

    pub fn scroll_to_bottom(&mut self) {
        self.term.scroll_display(Scroll::Bottom);
    }

    // ---- selection ----
    //
    // `Term` owns the selection: it rotates the anchors when output scrolls
    // the grid and drops them on clear/resize.

    /// Grid point under a viewport cell (row 0 = top of the visible area).
    pub fn grid_point(&self, viewport_row: usize, col: usize) -> Point {
        Point::new(
            Line(viewport_row as i32 - self.display_offset() as i32),
            Column(col.min(self.cols().saturating_sub(1))),
        )
    }

    pub fn start_selection(&mut self, ty: SelectionType, point: Point, side: Side) {
        self.term.selection = Some(Selection::new(ty, point, side));
    }

    pub fn update_selection(&mut self, point: Point, side: Side) {
        if let Some(selection) = self.term.selection.as_mut() {
            selection.update(point, side);
        }
    }

    pub fn clear_selection(&mut self) {
        self.term.selection = None;
    }

    /// Selected text; `None` for no selection or an empty one.
    pub fn selection_text(&self) -> Option<String> {
        self.term.selection_to_string().filter(|s| !s.is_empty())
    }

    pub fn has_selection(&self) -> bool {
        self.selection_range().is_some()
    }

    fn selection_range(&self) -> Option<SelectionRange> {
        self.term
            .selection
            .as_ref()
            .and_then(|selection| selection.to_range(&self.term))
    }

    // ---- snapshots ----

    fn line_inner(
        &self,
        viewport_row: usize,
        selection: Option<SelectionRange>,
    ) -> Vec<CellSnapshot> {
        let offset = self.display_offset() as i32;
        let line = Line(viewport_row as i32 - offset);
        let row = &self.term.grid()[line];
        (0..self.cols())
            .map(|col| {
                let cell = &row[Column(col)];
                CellSnapshot {
                    ch: cell.c,
                    fg: map_color(cell.fg),
                    bg: map_color(cell.bg),
                    bold: cell.flags.intersects(Flags::BOLD),
                    dim: cell.flags.intersects(Flags::DIM),
                    italic: cell.flags.intersects(Flags::ITALIC),
                    underline: cell.flags.intersects(Flags::ALL_UNDERLINES),
                    strikeout: cell.flags.intersects(Flags::STRIKEOUT),
                    inverse: cell.flags.intersects(Flags::INVERSE),
                    hidden: cell.flags.intersects(Flags::HIDDEN),
                    wide: cell.flags.intersects(Flags::WIDE_CHAR),
                    wide_spacer: cell
                        .flags
                        .intersects(Flags::WIDE_CHAR_SPACER | Flags::LEADING_WIDE_CHAR_SPACER),
                    selected: selection
                        .is_some_and(|range| range.contains(Point::new(line, Column(col)))),
                }
            })
            .collect()
    }

    /// One viewport row (0 = top), honoring the scrollback offset.
    pub fn line(&self, viewport_row: usize) -> Vec<CellSnapshot> {
        self.line_inner(viewport_row, self.selection_range())
    }

    /// All viewport rows, top to bottom.
    pub fn lines(&self) -> Vec<Vec<CellSnapshot>> {
        // Resolve the selection once: `to_range` re-walks the grid for
        // semantic and line selections.
        let selection = self.selection_range();
        (0..self.rows())
            .map(|r| self.line_inner(r, selection))
            .collect()
    }

    /// Cursor in viewport coordinates; `None` when hidden or scrolled out.
    pub fn cursor(&self) -> Option<CursorSnapshot> {
        let content = self.term.renderable_content();
        if content.cursor.shape == CursorShape::Hidden {
            return None;
        }
        let Point { line, column } = content.cursor.point;
        let row = line.0 + self.display_offset() as i32;
        if row < 0 || row >= self.rows() as i32 {
            return None;
        }
        Some(CursorSnapshot {
            row: row as usize,
            col: column.0,
            shape: content.cursor.shape,
        })
    }

    /// A viewport row as trimmed text (wide-char spacers skipped).
    pub fn row_text(&self, viewport_row: usize) -> String {
        let mut text: String = self
            .line(viewport_row)
            .iter()
            .filter(|c| !c.wide_spacer)
            .map(|c| c.ch)
            .collect();
        while text.ends_with(' ') {
            text.pop();
        }
        text
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use super::*;

    type Sink = Rc<RefCell<Vec<Vec<u8>>>>;
    type Resizes = Rc<RefCell<Vec<(u16, u16, u16, u16)>>>;

    fn term(cols: u16, rows: u16) -> (Terminal, Sink, Resizes) {
        let written: Sink = Rc::default();
        let resized: Resizes = Rc::default();
        let (w, r) = (written.clone(), Rc::clone(&resized));
        let t = Terminal::new(
            cols,
            rows,
            Box::new(move |b| w.borrow_mut().push(b)),
            Box::new(move |c, ro, pw, ph| r.borrow_mut().push((c, ro, pw, ph))),
        );
        (t, written, resized)
    }

    #[test]
    fn plain_text_lands_on_grid_and_moves_cursor() {
        let (mut t, _, _) = term(20, 5);
        t.process(b"one\r\ntwo");
        assert_eq!(t.row_text(0), "one");
        assert_eq!(t.row_text(1), "two");
        let c = t.cursor().unwrap();
        assert_eq!((c.row, c.col), (1, 3));
    }

    #[test]
    fn dsr_cursor_report_is_written_back() {
        let (mut t, written, _) = term(20, 5);
        t.process(b"\x1b[2;3H");
        assert!(written.borrow().is_empty());
        t.process(b"\x1b[6n");
        let w = written.borrow();
        assert_eq!(w.len(), 1);
        assert_eq!(w[0], b"\x1b[2;3R");
    }

    #[test]
    fn sgr_colors_and_attributes() {
        let (mut t, _, _) = term(40, 4);
        t.process(b"\x1b[1;31mred\x1b[0m \x1b[4mu\x1b[0m\x1b[38;2;10;20;30mT");
        let line = t.line(0);
        assert_eq!(line[0].fg, CellColor::Indexed(1));
        assert!(line[0].bold);
        assert_eq!(line[3].fg, CellColor::Foreground);
        assert!(line[4].underline);
        assert_eq!(line[5].fg, CellColor::Rgb(10, 20, 30));
    }

    #[test]
    fn title_and_bell_become_events() {
        let (mut t, _, _) = term(20, 2);
        let n = t.process(b"\x1b]0;my title\x07\x07");
        assert_eq!(
            n,
            vec![
                Notice::Event(TerminalEvent::TitleChanged("my title".into())),
                Notice::Event(TerminalEvent::Bell),
            ]
        );
    }

    #[test]
    fn osc11_background_query_answers_with_theme_color() {
        let (mut t, written, _) = term(20, 2);
        t.process(b"\x1b]11;?\x07");
        let w = written.borrow();
        assert_eq!(w.len(), 1);
        let s = String::from_utf8_lossy(&w[0]).to_string();
        assert!(s.starts_with("\x1b]11;rgb:1616/1818/1a1a"), "{s:?}");
    }

    #[test]
    fn app_cursor_mode_toggles() {
        let (mut t, _, _) = term(10, 2);
        assert!(!t.mode().contains(TermMode::APP_CURSOR));
        t.process(b"\x1b[?1h");
        assert!(t.mode().contains(TermMode::APP_CURSOR));
    }

    #[test]
    fn resize_changes_grid_and_remote_callback_reports_pixels() {
        let (mut t, _, resized) = term(20, 5);
        assert!(!t.resize(20, 5, 8.0, 16.0));
        assert!(t.resize(30, 10, 8.0, 16.0));
        assert_eq!((t.cols(), t.rows()), (30, 10));
        t.notify_remote_resize();
        assert_eq!(resized.borrow()[0], (30, 10, 240, 160));
    }

    #[test]
    fn selection_yields_text() {
        let (mut t, _, _) = term(20, 3);
        t.process(b"hello world");
        let start = t.grid_point(0, 0);
        t.start_selection(SelectionType::Simple, start, Side::Left);
        let end = t.grid_point(0, 4);
        t.update_selection(end, Side::Right);
        assert_eq!(t.selection_text().as_deref(), Some("hello"));
        assert!(t.line(0)[..5].iter().all(|c| c.selected));
        assert!(!t.line(0)[5].selected);
    }

    #[test]
    fn scrollback_scrolls_and_clamps() {
        let (mut t, _, _) = term(10, 3);
        for i in 1..=8 {
            t.process(format!("line{i}\r\n").as_bytes());
        }
        assert_eq!(t.row_text(0), "line7");
        t.scroll(100);
        assert_eq!(t.display_offset(), t.history_size());
        assert_eq!(t.row_text(0), "line1");
        t.scroll_to_bottom();
        assert_eq!(t.display_offset(), 0);
    }

    #[test]
    fn utf8_split_across_feeds_reassembles() {
        let (mut t, _, _) = term(10, 2);
        let b = "é".as_bytes();
        t.process(&b[..1]);
        t.process(&b[1..]);
        assert_eq!(t.row_text(0), "é");
    }
}
