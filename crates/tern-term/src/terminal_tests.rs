#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use super::*;

fn term(cols: u16, rows: u16) -> Terminal {
    Terminal::new(cols, rows)
}

/// The bytes a `process` call sends back to the remote.
fn output(notices: &[Notice]) -> Vec<Vec<u8>> {
    notices
        .iter()
        .filter_map(|n| match n {
            Notice::Event(TerminalEvent::Output(b)) => Some(b.clone()),
            _ => None,
        })
        .collect()
}

#[test]
fn plain_text_lands_on_grid_and_moves_cursor() {
    let mut t = term(20, 5);
    t.process(b"one\r\ntwo");
    assert_eq!(t.row_text(0), "one");
    assert_eq!(t.row_text(1), "two");
    let c = t.cursor().unwrap();
    assert_eq!((c.row, c.col), (1, 3));
}

#[test]
fn dsr_cursor_report_is_written_back() {
    let mut t = term(20, 5);
    assert!(output(&t.process(b"\x1b[2;3H")).is_empty());
    assert_eq!(output(&t.process(b"\x1b[6n")), [b"\x1b[2;3R".to_vec()]);
}

#[test]
fn sgr_colors_and_attributes() {
    let mut t = term(40, 4);
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
    let mut t = term(20, 2);
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
    let mut t = term(20, 2);
    let w = output(&t.process(b"\x1b]11;?\x07"));
    assert_eq!(w.len(), 1);
    let s = String::from_utf8_lossy(&w[0]).to_string();
    assert!(s.starts_with("\x1b]11;rgb:1616/1818/1a1a"), "{s:?}");
}

#[test]
fn app_cursor_mode_toggles() {
    let mut t = term(10, 2);
    assert!(!t.mode().contains(TermMode::APP_CURSOR));
    t.process(b"\x1b[?1h");
    assert!(t.mode().contains(TermMode::APP_CURSOR));
}

#[test]
fn resize_changes_grid_and_remote_callback_reports_pixels() {
    let mut t = term(20, 5);
    assert!(!t.resize(20, 5, 8.0, 16.0));
    assert!(t.resize(30, 10, 8.0, 16.0));
    assert_eq!((t.cols(), t.rows()), (30, 10));
    assert_eq!(
        t.resized_event(),
        TerminalEvent::Resized {
            cols: 30,
            rows: 10,
            pixel_width: 240,
            pixel_height: 160
        }
    );
}

#[test]
fn selection_yields_text() {
    let mut t = term(20, 3);
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
    let mut t = term(10, 3);
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
    let mut t = term(10, 2);
    let b = "é".as_bytes();
    t.process(&b[..1]);
    t.process(&b[1..]);
    assert_eq!(t.row_text(0), "é");
}
