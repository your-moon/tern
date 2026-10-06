#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use alacritty_terminal::index::{Column, Line};

use super::*;
use crate::terminal::Terminal;

fn point(line: i32, col: usize) -> Point {
    Point::new(Line(line), Column(col))
}

/// The text of every cell marked `mark` in the viewport, row by row.
fn marked(t: &Terminal, mark: SearchMark) -> Vec<String> {
    t.lines()
        .iter()
        .map(|row| {
            row.iter()
                .filter(|c| c.search == mark)
                .map(|c| c.ch)
                .collect::<String>()
        })
        .filter(|s| !s.is_empty())
        .collect()
}

#[test]
fn match_spanning_a_soft_wrap_is_one_match() {
    // 10 columns: "hello" starts at column 7 and wraps onto the next row.
    let mut t = Terminal::new(10, 4);
    t.process(b"xxxxxxxhello world");
    assert_eq!(t.search("hello"), 1);
    let visible = t.visible_matches();
    assert_eq!(visible.len(), 1);
    let (m, current) = &visible[0];
    assert!(*current);
    assert_eq!((*m.start(), *m.end()), (point(0, 7), point(1, 1)));
    assert_eq!(marked(&t, SearchMark::Current), ["hel", "lo"]);
}

#[test]
fn hard_newline_does_not_join_a_match() {
    let mut t = Terminal::new(10, 4);
    t.process(b"xxxxxxxhel\r\nlo world");
    assert_eq!(t.search("hello"), 0);
}

#[test]
fn finds_matches_in_scrollback_and_scrolls_to_them() {
    let mut t = Terminal::new(20, 3);
    for i in 1..=9 {
        let tag = if i == 2 || i == 8 { "needle" } else { "hay" };
        t.process(format!("line{i} {tag}\r\n").as_bytes());
    }
    assert_eq!(t.search("needle"), 2);
    // Newest match first: line8 is on screen, nothing scrolled.
    assert_eq!(t.search_index(), Some(1));
    assert_eq!(t.display_offset(), 0);
    assert_eq!(marked(&t, SearchMark::Current), ["needle"]);
    // Older: line2 sits in history, so the view must scroll up to show it.
    assert!(t.search_step(true));
    assert_eq!(t.search_index(), Some(0));
    assert!(t.display_offset() > 0);
    assert_eq!(marked(&t, SearchMark::Current), ["needle"]);
    let row = t
        .lines()
        .iter()
        .position(|r| r.iter().any(|c| c.search == SearchMark::Current))
        .unwrap();
    assert!(t.row_text(row).starts_with("line2"), "{}", t.row_text(row));
    // The other match is not on screen, so it carries no mark.
    assert!(marked(&t, SearchMark::Match).is_empty());
}

#[test]
fn stepping_wraps_in_both_directions() {
    let mut t = Terminal::new(20, 6);
    t.process(b"a1\r\nb\r\na2\r\nb\r\na3");
    assert_eq!(t.search("a"), 3);
    assert_eq!(t.search_index(), Some(2));
    t.search_step(false);
    assert_eq!(t.search_index(), Some(0));
    t.search_step(true);
    assert_eq!(t.search_index(), Some(2));
    t.search_step(true);
    assert_eq!(t.search_index(), Some(1));
}

#[test]
fn query_is_literal_and_smart_case() {
    let mut t = Terminal::new(30, 3);
    t.process(b"axb a.b Foo foo");
    assert_eq!(t.search("a.b"), 1, "dot is not a wildcard");
    assert_eq!(t.search("foo"), 2, "lowercase query ignores case");
    assert_eq!(t.search("Foo"), 1, "uppercase query is exact");
    assert_eq!(t.search("(a"), 0, "unbalanced paren is not an error");
}

#[test]
fn new_output_refreshes_matches_and_keeps_the_current_one() {
    let mut t = Terminal::new(20, 3);
    t.process(b"err one\r\nok\r\nerr two\r\n");
    assert_eq!(t.search("err"), 2);
    t.search_step(true);
    assert_eq!(t.search_index(), Some(0));
    t.process(b"err three\r\nmore\r\n");
    assert_eq!(t.search_count(), 3);
    assert_eq!(t.search_index(), Some(0), "still on 'err one'");
}

#[test]
fn clearing_removes_marks() {
    let mut t = Terminal::new(20, 3);
    t.process(b"find me");
    assert_eq!(t.search("find"), 1);
    assert!(!marked(&t, SearchMark::Current).is_empty());
    t.clear_search();
    assert!(marked(&t, SearchMark::Current).is_empty());
    assert_eq!(t.search_count(), 0);
    assert_eq!(t.search(""), 0);
}
