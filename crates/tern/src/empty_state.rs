//! The look of an empty list (an icon and one short line) and the one decision every list
//! makes: nothing exists yet, nothing matches, or there is something to show. The primary
//! action is the caller's, because only the caller knows what it opens.

use gpui::{Div, ParentElement as _, SharedString, Styled as _, div, px};

use crate::icons::{self, icon};
use crate::theme::Theme;

/// What a list shows, given how many items exist and how many survive its filter.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Listing {
    /// Nothing exists yet: show the empty state with its primary action.
    Empty,
    /// Items exist but the filter hides them all: plain text, no action.
    NoMatch,
    List,
}

#[must_use]
pub fn listing(total: usize, shown: usize) -> Listing {
    if total == 0 {
        Listing::Empty
    } else if shown == 0 {
        Listing::NoMatch
    } else {
        Listing::List
    }
}

/// Icon over one muted line, centered. Add the action button as the next child.
pub fn view(t: &Theme, glyph: &'static str, line: impl Into<SharedString>) -> Div {
    div()
        .flex()
        .flex_col()
        .items_center()
        .gap(px(8.))
        .child(icon(glyph).size(px(24.)).text_color(t.faint))
        .child(
            div()
                .text_size(px(12.))
                .text_color(t.muted)
                .child(line.into()),
        )
}

pub const HOSTS: &str = icons::SERVER;
pub const SNIPPETS: &str = icons::FILE_CODE;
pub const FOLDER: &str = icons::FOLDER;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nothing_saved_is_the_empty_state_even_when_a_query_is_typed() {
        assert_eq!(listing(0, 0), Listing::Empty);
    }

    #[test]
    fn a_filter_that_hides_everything_is_not_the_empty_state() {
        assert_eq!(listing(3, 0), Listing::NoMatch);
    }

    #[test]
    fn items_that_survive_the_filter_are_listed() {
        assert_eq!(listing(3, 2), Listing::List);
        assert_eq!(listing(1, 1), Listing::List);
    }
}
