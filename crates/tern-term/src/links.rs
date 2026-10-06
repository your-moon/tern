//! Clickable links: URLs detected in output text, and OSC 8 hyperlinks.
//!
//! Detection is a hand-written scanner (no regex dependency) following the
//! behaviour of alacritty's built-in URL hint: a known scheme, then everything
//! up to whitespace, control characters or `<>"`{}|\^`, then trailing
//! punctuation and unbalanced closing brackets trimmed off. Only schemes a
//! click may safely hand to the OS are recognised, because the text comes from
//! a remote machine.

use std::ops::Range;

/// Schemes recognised in plain text. Each must be followed by at least one
/// body character.
const SCHEMES: [&str; 4] = ["https://", "http://", "ftp://", "mailto:"];

/// Whether a click may open `uri`. Applies to OSC 8 targets too, which a
/// remote program chooses freely (`file:`, `x-apple-...:` and the like would
/// reach local handlers).
pub(crate) fn is_openable(uri: &str) -> bool {
    let lower = uri.to_ascii_lowercase();
    ["https://", "http://", "ftp://", "ftps://", "mailto:"]
        .iter()
        .any(|scheme| lower.starts_with(scheme))
}

fn is_body(c: char) -> bool {
    !(c.is_control()
        || c.is_whitespace()
        || matches!(
            c,
            '<' | '>' | '"' | '`' | '{' | '|' | '}' | '\\' | '^' | '⟨' | '⟩'
        ))
}

fn starts_with_at(chars: &[char], at: usize, scheme: &str) -> bool {
    let mut it = chars[at..].iter();
    scheme
        .chars()
        .all(|s| it.next().is_some_and(|c| c.eq_ignore_ascii_case(&s)))
}

/// Drop trailing sentence punctuation and closing brackets with no opener in
/// the URL, so `(see https://a.test/x).` links `https://a.test/x` while
/// `https://en.wikipedia.org/wiki/Rust_(language)` keeps its parenthesis.
fn trim_end(chars: &[char], start: usize, mut end: usize) -> usize {
    while end > start {
        let url = &chars[start..end];
        let count = |c: char| url.iter().filter(|&&x| x == c).count();
        match url[url.len() - 1] {
            '.' | ',' | ':' | ';' | '?' | '!' | '\'' => end -= 1,
            ')' if count(')') > count('(') => end -= 1,
            ']' if count(']') > count('[') => end -= 1,
            _ => break,
        }
    }
    end
}

/// The URL in `chars` that contains index `at`, as a char range.
pub(crate) fn find_url(chars: &[char], at: usize) -> Option<Range<usize>> {
    let mut i = 0;
    while i < chars.len() {
        // A scheme only starts at a word boundary ("xhttp://" is not a link).
        let boundary = i == 0 || !chars[i - 1].is_alphanumeric();
        let scheme = SCHEMES
            .iter()
            .find(|s| boundary && starts_with_at(chars, i, s));
        let Some(scheme) = scheme else {
            i += 1;
            continue;
        };
        let body_start = i + scheme.len();
        let mut end = body_start;
        while end < chars.len() && is_body(chars[end]) {
            end += 1;
        }
        let end = trim_end(chars, i, end);
        if end > body_start && (i..end).contains(&at) {
            return Some(i..end);
        }
        i = end.max(i + 1);
    }
    None
}

/// A link under the pointer: where it is drawn and what it opens.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Link {
    pub uri: String,
    /// Viewport pieces `(row, start_col, end_col)`, end exclusive; more than
    /// one when the link wraps.
    pub segments: Vec<(usize, usize, usize)>,
}

#[cfg(test)]
mod tests {
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
}
