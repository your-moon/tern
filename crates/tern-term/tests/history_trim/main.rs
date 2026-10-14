#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::print_stdout
)]
//! History rows are stored trimmed. These tests pin that to the observable grid.

mod scenario;

#[test]
fn matches_upstream_dumps() {
    let golden: Vec<(String, u64)> = include_str!("golden.txt")
        .lines()
        .map(|l| {
            let (label, hash) = l.rsplit_once(' ').unwrap();
            (label.to_string(), u64::from_str_radix(hash, 16).unwrap())
        })
        .collect();
    let got = scenario::all();
    assert_eq!(got.len(), golden.len());
    for ((label, hash), (want_label, want)) in got.iter().zip(&golden) {
        assert_eq!(label, want_label);
        assert_eq!(
            hash, want,
            "{label}: grid differs from upstream alacritty_terminal 0.26.0"
        );
    }
}

use alacritty_terminal::grid::Dimensions;
use alacritty_terminal::index::Line;
use alacritty_terminal::term::cell::Cell;
use scenario::Session;

/// (history rows stored trimmed, history rows, visible rows stored trimmed).
fn trim_counts(s: &Session) -> (usize, usize, usize) {
    let grid = s.term.grid();
    let in_history = (grid.topmost_line().0..0).map(|l| &grid[Line(l)]);
    let history = grid.history_size();
    let trimmed = in_history.filter(|r| r.stored_len() < r.len()).count();
    let visible = (0..grid.screen_lines() as i32)
        .filter(|&l| grid[Line(l)].stored_len() < grid[Line(l)].len())
        .count();
    (trimmed, history, visible)
}

#[test]
fn history_is_trimmed_and_the_screen_never_is() {
    let mut s = Session::new(3, 40, 10, scenario::HISTORY);
    let ops = s.random_ops(1500);
    s.feed(&ops);
    let (trimmed, history, visible) = trim_counts(&s);
    assert!(history >= 250, "history {history}");
    assert!(
        trimmed * 10 >= history * 4,
        "only {trimmed} of {history} history rows are trimmed"
    );
    assert_eq!(visible, 0, "screen rows must be full width");

    // Rows come back from history on a taller screen, and are rebuilt by every resize.
    for (cols, lines) in [(40, 18), (40, 7), (25, 7), (70, 12), (70, 3), (33, 14)] {
        s.resize(cols, lines);
        let (trimmed, history, visible) = trim_counts(&s);
        assert_eq!(visible, 0, "{cols}x{lines}: screen rows must be full width");
        assert!(
            trimmed * 10 >= history * 3,
            "{cols}x{lines}: {trimmed} of {history} trimmed"
        );
    }
}

/// Bytes the history keeps for cells: stored cells of every history row.
fn history_bytes(s: &Session) -> (usize, usize) {
    let grid = s.term.grid();
    let cell = std::mem::size_of::<Cell>();
    let rows = (grid.topmost_line().0..0).map(|l| &grid[Line(l)]);
    rows.fold((0, 0), |(stored, full), r| {
        (stored + r.stored_len() * cell, full + r.len() * cell)
    })
}

#[test]
fn ten_thousand_short_lines_cost_a_fraction_of_full_width_rows() {
    assert_eq!(std::mem::size_of::<Cell>(), 24);
    let mut s = Session::new(1, 200, 24, 10_000);
    let mut text = String::new();
    for i in 0..10_050 {
        text.push_str(&format!("{i}: {}\r\n", "x".repeat(i % 50)));
    }
    s.feed(text.as_bytes());
    let (stored, full) = history_bytes(&s);
    assert_eq!(s.term.grid().history_size(), 10_000);
    assert_eq!(full, 10_000 * 200 * 24);
    // About 30 characters a line out of 200 columns.
    assert!(
        stored * 5 < full,
        "history keeps {stored} of {full} bytes of cells"
    );
    println!("history cells: {stored} B stored of {full} B");

    let mut s = Session::new(1, 200, 24, 10_000);
    let mut text = String::new();
    for i in 0..10_050 {
        text.push_str(&format!("ok {i}\r\n"));
    }
    s.feed(text.as_bytes());
    let (stored, full) = history_bytes(&s);
    assert!(
        stored * 15 < full,
        "short lines keep {stored} of {full} bytes of cells"
    );
}

#[test]
fn rows_compare_by_their_logical_cells() {
    let mut s = Session::new(5, 80, 6, 100);
    s.feed(b"short\r\n\x1b[41mred\x1b[K\x1b[0m\r\n");
    s.feed("scroll\r\n".repeat(20).as_bytes());
    let grid = s.term.grid();
    let top = grid.topmost_line();
    let (first, second) = (&grid[top], &grid[Line(top.0 + 1)]);
    assert!(first.stored_len() < first.len() && second.stored_len() < second.len());

    let mut full = first.clone();
    let _ = &mut full[..];
    assert_eq!(full.stored_len(), 80);
    assert!(
        &full == first && first == &full,
        "a row equals its expanded copy"
    );
    assert!(
        first != second,
        "different content (plain vs coloured tail)"
    );

    let mut edited = full.clone();
    edited[alacritty_terminal::index::Column(79)].c = 'z';
    assert!(
        &edited != first && first != &edited,
        "a change in the dropped tail is a difference"
    );

    let narrower = {
        let mut r = first.clone();
        let _ = r.shrink(60);
        r
    };
    assert_eq!(narrower.len(), 60);
    assert!(&narrower != first, "rows of different width are different");
}

#[test]
fn screen_rows_are_full_width_after_every_few_operations() {
    for seed in 1..=4 {
        let mut s = Session::new(seed, 40, 10, scenario::HISTORY);
        let mut saw_trimmed = 0;
        for round in 0..300 {
            let ops = s.random_ops(5);
            s.feed(&ops);
            let (trimmed, _, visible) = trim_counts(&s);
            assert_eq!(
                visible, 0,
                "seed {seed} round {round}: a screen row is stored trimmed"
            );
            saw_trimmed += trimmed;
        }
        assert!(saw_trimmed > 0);
    }
}
