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
#[path = "links_tests.rs"]
mod tests;
