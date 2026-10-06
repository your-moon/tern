//! The find bar's key-driven query buffer. The view draws it; this decides what
//! a keystroke means. There is no text-input widget here (tern-term cannot
//! depend on the app crate's), only typing, backspace, Enter and Escape.

use gpui::Keystroke;

/// What a keystroke did to the find bar.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FindKey {
    /// Escape: close the bar and drop the highlights.
    Close,
    /// Enter: the next match (older output).
    Next,
    /// Shift+Enter: the previous match (newer output).
    Prev,
    /// The query text changed: search again.
    Edited,
    /// Nothing for the bar; the key is still swallowed so it never reaches
    /// the remote while the bar has focus.
    Ignored,
}

/// Apply `keystroke` to `query`.
pub(crate) fn apply_key(query: &mut String, keystroke: &Keystroke) -> FindKey {
    let mods = &keystroke.modifiers;
    match keystroke.key.as_str() {
        "escape" => return FindKey::Close,
        "enter" => {
            return if mods.shift {
                FindKey::Prev
            } else {
                FindKey::Next
            };
        }
        "backspace" => {
            return if query.pop().is_some() {
                FindKey::Edited
            } else {
                FindKey::Ignored
            };
        }
        _ => {}
    }
    if mods.platform || mods.control {
        return FindKey::Ignored;
    }
    match keystroke.key_char.as_deref() {
        Some(text) if !text.is_empty() && !text.chars().any(char::is_control) => {
            query.push_str(text);
            FindKey::Edited
        }
        _ => FindKey::Ignored,
    }
}

/// Append pasted text: first line only, since a query is one line.
pub(crate) fn paste_into(query: &mut String, text: &str) -> FindKey {
    let line = text.lines().next().unwrap_or("");
    if line.is_empty() {
        return FindKey::Ignored;
    }
    query.push_str(line);
    FindKey::Edited
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use super::*;

    fn key(source: &str) -> Keystroke {
        Keystroke::parse(source).unwrap()
    }

    /// A printable keystroke as the platform delivers it: key plus typed text.
    fn typed(key: &str, text: &str) -> Keystroke {
        Keystroke {
            key_char: Some(text.into()),
            ..self::key(key)
        }
    }

    #[test]
    fn typing_builds_the_query_and_backspace_removes() {
        let mut q = String::new();
        assert_eq!(apply_key(&mut q, &typed("f", "f")), FindKey::Edited);
        assert_eq!(apply_key(&mut q, &typed("shift-o", "O")), FindKey::Edited);
        assert_eq!(q, "fO");
        assert_eq!(apply_key(&mut q, &key("backspace")), FindKey::Edited);
        assert_eq!(q, "f");
        apply_key(&mut q, &key("backspace"));
        assert_eq!(apply_key(&mut q, &key("backspace")), FindKey::Ignored);
        assert_eq!(q, "");
    }

    #[test]
    fn enter_and_escape_are_navigation_not_text() {
        let mut q = "x".to_string();
        assert_eq!(apply_key(&mut q, &key("enter")), FindKey::Next);
        assert_eq!(apply_key(&mut q, &key("shift-enter")), FindKey::Prev);
        assert_eq!(apply_key(&mut q, &key("escape")), FindKey::Close);
        assert_eq!(q, "x");
    }

    #[test]
    fn command_chords_do_not_type() {
        let mut q = String::new();
        let cmd_a = Keystroke {
            key_char: Some("a".into()),
            ..key("cmd-a")
        };
        let ctrl_c = Keystroke {
            key_char: Some("\u{3}".into()),
            ..key("ctrl-c")
        };
        assert_eq!(apply_key(&mut q, &cmd_a), FindKey::Ignored);
        assert_eq!(apply_key(&mut q, &ctrl_c), FindKey::Ignored);
        assert_eq!(q, "");
    }

    #[test]
    fn paste_takes_the_first_line() {
        let mut q = "a".to_string();
        assert_eq!(paste_into(&mut q, "bc\nrm -rf"), FindKey::Edited);
        assert_eq!(q, "abc");
        assert_eq!(paste_into(&mut q, ""), FindKey::Ignored);
    }
}
