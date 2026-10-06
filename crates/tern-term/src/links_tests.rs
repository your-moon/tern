#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use super::*;
use crate::terminal::Terminal;

fn url_in(text: &str, needle: &str) -> Option<String> {
    let chars: Vec<char> = text.chars().collect();
    let at = text.find(needle).unwrap();
    let at = text[..at].chars().count();
    find_url(&chars, at).map(|r| chars[r].iter().collect())
}

#[test]
fn trailing_punctuation_is_not_part_of_the_url() {
    for (text, want) in [
        ("see https://a.test/x.", "https://a.test/x"),
        ("see https://a.test/x, then", "https://a.test/x"),
        ("is it https://a.test/x?", "https://a.test/x"),
        ("https://a.test/x?q=1&b=2;", "https://a.test/x?q=1&b=2"),
        ("go to 'https://a.test/x'", "https://a.test/x"),
    ] {
        assert_eq!(url_in(text, "https").as_deref(), Some(want), "{text}");
    }
}

#[test]
fn parentheses_balance() {
    // Wrapped in prose: the closing paren and the period are not the URL's.
    assert_eq!(
        url_in("(see https://a.test/x).", "https").as_deref(),
        Some("https://a.test/x")
    );
    // Part of the URL: kept.
    assert_eq!(
        url_in("https://en.wikipedia.org/wiki/Rust_(language).", "https").as_deref(),
        Some("https://en.wikipedia.org/wiki/Rust_(language)")
    );
    assert_eq!(
        url_in("[https://a.test/x]", "https").as_deref(),
        Some("https://a.test/x")
    );
}

#[test]
fn boundaries_and_hit_testing() {
    let text = "a https://one.test b http://two.test/p";
    assert_eq!(url_in(text, "one").as_deref(), Some("https://one.test"));
    assert_eq!(url_in(text, "two").as_deref(), Some("http://two.test/p"));
    // Whitespace between them is not on either link.
    assert_eq!(url_in(text, " b "), None);
    // A scheme glued to a word is not a link start; a bare scheme has no body.
    assert_eq!(url_in("xhttps://a.test", "a.test"), None);
    assert_eq!(url_in("see https:// now", "https"), None);
    assert_eq!(
        url_in("mail me: mailto:bob@a.test now", "bob").as_deref(),
        Some("mailto:bob@a.test")
    );
}

#[test]
fn only_safe_schemes_open() {
    assert!(is_openable("https://a.test"));
    assert!(is_openable("HTTP://a.test"));
    assert!(is_openable("mailto:a@b.test"));
    assert!(!is_openable("file:///etc/passwd"));
    assert!(!is_openable("x-apple.systempreferences:"));
    assert!(!is_openable("javascript:alert(1)"));
}

#[test]
fn link_wrapped_over_two_rows_is_one_link_with_two_segments() {
    // 12 columns: the URL starts at column 4 and wraps.
    let mut t = Terminal::new(12, 4);
    t.process(b"go: https://example.test/path.");
    let first = t.link_at(0, 6).unwrap();
    assert_eq!(first.uri, "https://example.test/path");
    assert_eq!(first.segments, [(0, 4, 12), (1, 0, 12), (2, 0, 5)]);
    // Hovering the wrapped part finds the same link.
    assert_eq!(t.link_at(1, 3).unwrap(), first);
    // The label before it and the trailing period are not the link.
    assert_eq!(t.link_at(0, 1), None);
    assert_eq!(t.link_at(2, 5), None);
}

#[test]
fn osc8_hyperlink_covers_its_own_text_only() {
    let mut t = Terminal::new(40, 3);
    t.process(b"a \x1b]8;;https://docs.test/x\x1b\\the docs\x1b]8;;\x1b\\ b");
    let link = t.link_at(0, 5).unwrap();
    assert_eq!(link.uri, "https://docs.test/x");
    assert_eq!(link.segments, [(0, 2, 10)]);
    assert_eq!(t.link_at(0, 0), None);
    assert_eq!(t.link_at(0, 11), None);
}

#[test]
fn osc8_with_an_unsafe_target_is_not_clickable() {
    let mut t = Terminal::new(40, 3);
    t.process(b"\x1b]8;;file:///etc/passwd\x1b\\click\x1b]8;;\x1b\\");
    assert_eq!(t.link_at(0, 1), None);
}
