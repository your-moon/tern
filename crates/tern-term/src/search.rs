//! Find in scrollback. Written against `alacritty_terminal`'s search API
//! (`RegexSearch`, `RegexIter`), not adapted from Zed's `terminal.rs`: that
//! file is GPL-3.0-or-later and this module is meant to stay free of it.
//!
//! [`Search`] keeps the query, every match in the grid (history included, soft
//! wraps followed), and the current match. Matches are rescanned whenever new
//! output arrives, and the current match is tracked by its row counted from
//! the top of the buffer so it stays on the same text while history grows.

use alacritty_terminal::grid::Dimensions;
use alacritty_terminal::index::{Column, Direction, Point};
use alacritty_terminal::term::Term;
use alacritty_terminal::term::search::{Match, RegexIter, RegexSearch};

/// Cap on stored matches: a one-letter query over a full 10k-line history
/// would otherwise hold (and rescan, per chunk of output) a huge list.
const MAX_MATCHES: usize = 10_000;

/// What a cell is, as far as the search overlay is concerned.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SearchMark {
    #[default]
    None,
    /// Part of a match.
    Match,
    /// Part of the current match.
    Current,
}

/// A match start counted from the top of the buffer: (row, column). Stable
/// while history grows, because the line index drops by exactly what the
/// history gains.
type Anchor = (i64, usize);

fn anchor_of<T>(term: &Term<T>, m: &Match) -> Anchor {
    let start = m.start();
    (
        i64::from(start.line.0) + term.history_size() as i64,
        start.column.0,
    )
}

pub(crate) struct Search {
    query: String,
    regex: Option<RegexSearch>,
    matches: Vec<Match>,
    current: Option<usize>,
    anchor: Option<Anchor>,
}

/// Escape regex metacharacters so the query is matched literally.
fn escape(query: &str) -> String {
    let mut out = String::with_capacity(query.len());
    for c in query.chars() {
        if "\\.+*?()|[]{}^$#&-~".contains(c) {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

impl Search {
    /// Start a search. An empty query matches nothing. Matching is literal and
    /// smart-case (case-insensitive unless the query has an uppercase letter).
    pub(crate) fn new<T>(term: &Term<T>, query: &str) -> Self {
        let regex = if query.is_empty() {
            None
        } else {
            RegexSearch::new(&escape(query)).ok()
        };
        let mut search = Self {
            query: query.to_string(),
            regex,
            matches: Vec::new(),
            current: None,
            anchor: None,
        };
        search.matches = search.collect(term);
        // Start from the newest match: the thing just printed is the thing
        // usually looked for.
        search.select(term, search.matches.len().checked_sub(1));
        search
    }

    fn collect<T>(&mut self, term: &Term<T>) -> Vec<Match> {
        let Some(regex) = self.regex.as_mut() else {
            return Vec::new();
        };
        let start = Point::new(term.topmost_line(), Column(0));
        let end = Point::new(term.bottommost_line(), term.last_column());
        RegexIter::new(start, end, Direction::Right, term, regex)
            .take(MAX_MATCHES)
            .collect()
    }

    fn select<T>(&mut self, term: &Term<T>, index: Option<usize>) {
        self.current = index;
        self.anchor = index
            .and_then(|i| self.matches.get(i))
            .map(|m| anchor_of(term, m));
    }

    /// Rescan after the grid changed, keeping the current match on the same
    /// text where it still exists (else the next one after it).
    pub(crate) fn refresh<T>(&mut self, term: &Term<T>) {
        self.matches = self.collect(term);
        let index = match self.anchor {
            _ if self.matches.is_empty() => None,
            None => Some(self.matches.len() - 1),
            Some(anchor) => {
                let ix = self
                    .matches
                    .partition_point(|m| anchor_of(term, m) < anchor);
                Some(ix.min(self.matches.len() - 1))
            }
        };
        // The anchor stays where it was: re-anchoring would pin it to
        // whatever slid underneath once the history cap is reached.
        self.current = index;
        if self.anchor.is_none() {
            self.select(term, index);
        }
    }

    pub(crate) fn query(&self) -> &str {
        &self.query
    }

    pub(crate) fn count(&self) -> usize {
        self.matches.len()
    }

    /// Zero-based index of the current match.
    pub(crate) fn current_index(&self) -> Option<usize> {
        self.current
    }

    pub(crate) fn current(&self) -> Option<&Match> {
        self.current.and_then(|i| self.matches.get(i))
    }

    /// Move to the next match going up into older output (`older`) or down
    /// toward newer output, wrapping at either end.
    pub(crate) fn step<T>(&mut self, term: &Term<T>, older: bool) -> Option<&Match> {
        let n = self.matches.len();
        if n == 0 {
            return None;
        }
        let ix = self.current.unwrap_or(n - 1);
        let next = if older {
            (ix + n - 1) % n
        } else {
            (ix + 1) % n
        };
        self.select(term, Some(next));
        self.current()
    }

    /// Matches touching grid lines `top..=bottom`, each flagged whether it is
    /// the current one. Matches are sorted and disjoint, so the lower bound is
    /// binary-searched.
    pub(crate) fn visible(&self, top: i32, bottom: i32) -> Vec<(Match, bool)> {
        let first = self.matches.partition_point(|m| m.end().line.0 < top);
        self.matches[first..]
            .iter()
            .enumerate()
            .take_while(|(_, m)| m.start().line.0 <= bottom)
            .map(|(i, m)| (m.clone(), self.current == Some(first + i)))
            .collect()
    }
}

/// The mark for the cell at `point` given the visible matches.
pub(crate) fn mark_at(visible: &[(Match, bool)], point: Point) -> SearchMark {
    for (m, current) in visible {
        if m.contains(&point) {
            return if *current {
                SearchMark::Current
            } else {
                SearchMark::Match
            };
        }
    }
    SearchMark::None
}

#[cfg(test)]
#[path = "search_tests.rs"]
mod tests;
