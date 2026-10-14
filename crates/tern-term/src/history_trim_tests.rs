#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
//! Find and selection read history rows that are stored trimmed. They must give the answers the
//! same text gives when every row is full width.

use super::*;

const COLS: u16 = 100;

/// Short lines, coloured blank tails, a line that wraps with a match across the wrap, and wide
/// characters: enough history for every row kind to be trimmed.
fn terminal() -> Terminal {
    let mut t = Terminal::with_scrollback(COLS, 10, 2000);
    let mut bytes = String::new();
    for i in 0..700 {
        match i % 7 {
            0 => bytes.push_str(&format!("{i} needle {}\r\n", "ab ".repeat(i % 9))),
            1 => bytes.push_str("\x1b[41mred needle\x1b[K\x1b[0m\r\n"),
            2 => bytes.push_str(&format!(
                "{}needle tail {i}\r\n",
                "x".repeat(COLS as usize - 3)
            )),
            3 => bytes.push_str("\u{5b57}\u{5b57} needle \u{1f987}\r\n"),
            4 => bytes.push_str("\r\n"),
            5 => bytes.push_str(&format!("\x1b[1;32mbold {i}\x1b[0m   \t spaced\r\n")),
            _ => bytes.push_str(&format!("plain {i}\r\n")),
        }
    }
    t.process(bytes.as_bytes());
    t
}

fn expand_history(t: &mut Terminal) {
    let grid = t.term.grid_mut();
    for line in grid.topmost_line().0..0 {
        let _ = &mut grid[Line(line)][..];
    }
}

fn trimmed_rows(t: &Terminal) -> usize {
    let grid = t.term.grid();
    (grid.topmost_line().0..0)
        .filter(|&l| grid[Line(l)].stored_len() < grid[Line(l)].len())
        .count()
}

fn matches(t: &Terminal, query: &str) -> Vec<(Match, bool)> {
    let search = Search::new(&t.term, query);
    let grid = t.term.grid();
    search.visible(grid.topmost_line().0, grid.bottommost_line().0)
}

#[test]
fn search_over_trimmed_history_finds_what_full_width_rows_do() {
    let mut t = terminal();
    assert!(
        trimmed_rows(&t) > 400,
        "history is not trimmed: {}",
        trimmed_rows(&t)
    );
    let queries = [
        "needle",
        "Needle",
        "ab ab",
        "needle tail",
        "\u{5b57}",
        "plain 6",
        "xxxneedle",
    ];
    let trimmed: Vec<_> = queries.iter().map(|q| matches(&t, q)).collect();
    assert!(trimmed[0].len() > 300, "{} matches", trimmed[0].len());
    assert!(trimmed[3].len() > 50 && trimmed[4].len() > 50 && trimmed[6].len() > 50);

    expand_history(&mut t);
    assert_eq!(trimmed_rows(&t), 0);
    for (q, before) in queries.iter().zip(&trimmed) {
        assert_eq!(&matches(&t, q), before, "query {q:?}");
    }
}

#[test]
fn terminal_search_counts_and_steps_the_same() {
    let mut t = terminal();
    let count = t.search("needle");
    let mut order = Vec::new();
    for _ in 0..40 {
        t.search_step(true);
        order.push((t.search_index(), t.display_offset()));
    }
    expand_history(&mut t);
    let mut again = t.search("needle");
    assert_eq!(count, again);
    let mut order_full = Vec::new();
    for _ in 0..40 {
        t.search_step(true);
        order_full.push((t.search_index(), t.display_offset()));
    }
    again = t.search_count();
    assert_eq!(count, again);
    assert_eq!(order, order_full);
}

fn selections(t: &mut Terminal) -> Vec<Option<String>> {
    let mut out = Vec::new();
    let top = t.term.grid().topmost_line().0;
    for ty in [
        SelectionType::Simple,
        SelectionType::Block,
        SelectionType::Semantic,
        SelectionType::Lines,
    ] {
        for (from, to) in [(0, 40), (7, 60), (100, 180), (-3, 5), (350, 351)] {
            let start = Point::new(Line(top + from), Column(3));
            let end = Point::new(Line(top + to), Column(COLS as usize - 20));
            t.start_selection(ty, start, Side::Left);
            t.update_selection(end, Side::Right);
            out.push(t.selection_text());
        }
    }
    t.clear_selection();
    out
}

#[test]
fn selection_text_over_trimmed_history_matches_full_width() {
    let mut t = terminal();
    assert!(trimmed_rows(&t) > 400);
    let trimmed = selections(&mut t);
    assert!(trimmed.iter().filter(|s| s.is_some()).count() >= 15);
    assert!(trimmed.iter().flatten().any(|s| s.contains("needle")));

    expand_history(&mut t);
    assert_eq!(selections(&mut t), trimmed);
}

/// Known text, so the expected selection does not come from the code under test.
#[test]
fn selection_of_known_history_lines() {
    let mut t = Terminal::with_scrollback(80, 10, 500);
    let bytes: String = (0..100).map(|i| format!("L{i}\r\n")).collect();
    t.process(bytes.as_bytes());
    assert_eq!(t.history_size(), 91);
    t.start_selection(
        SelectionType::Lines,
        Point::new(Line(-3), Column(0)),
        Side::Left,
    );
    t.update_selection(Point::new(Line(-1), Column(0)), Side::Left);
    assert_eq!(t.selection_text().as_deref(), Some("L88\nL89\nL90\n"));

    t.start_selection(
        SelectionType::Simple,
        Point::new(Line(-2), Column(0)),
        Side::Left,
    );
    t.update_selection(Point::new(Line(-2), Column(79)), Side::Right);
    assert_eq!(t.selection_text().as_deref(), Some("L89"));
    assert!(t.term.grid()[Line(-2)].stored_len() < 80);
}
