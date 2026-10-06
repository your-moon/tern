//! Terminal colour schemes: the 725 of iTerm2-Color-Schemes (mbadolato, MIT), bundled in
//! Ghostty's format by `scripts/import-themes.sh` and parsed once on first use.

use std::sync::OnceLock;

/// The built-in scheme: zeron's dark palette, used when no other is chosen.
const DEFAULT_NAME: &str = "Zeron Dark";
const DEFAULT_NAME_LIGHT: &str = "Zeron Light";

/// The name shown for the built-in palette that follows the appearance.
pub fn default_name(light: bool) -> &'static str {
    if light {
        DEFAULT_NAME_LIGHT
    } else {
        DEFAULT_NAME
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Scheme {
    pub name: String,
    pub background: u32,
    pub foreground: u32,
    pub cursor: u32,
    pub selection: u32,
    pub ansi: [u32; 16],
}

static ALL: OnceLock<Vec<Scheme>> = OnceLock::new();

pub fn all() -> &'static [Scheme] {
    ALL.get_or_init(|| parse(include_str!("../assets/themes.txt")))
}

pub fn find(name: &str) -> Option<&'static Scheme> {
    all().iter().find(|s| s.name == name)
}

/// Parses `## Name` blocks of `key=value` lines. A block missing its background or
/// foreground, or with fewer than 16 palette entries, is skipped rather than half-applied.
pub fn parse(text: &str) -> Vec<Scheme> {
    let mut out = Vec::new();
    let mut lines = text.lines().peekable();
    while let Some(line) = lines.next() {
        let Some(name) = line.strip_prefix("## ") else {
            continue;
        };
        let mut ansi = [None; 16];
        let (mut bg, mut fg, mut cursor, mut selection) = (None, None, None, None);
        while let Some(next) = lines.peek() {
            if next.starts_with("## ") {
                break;
            }
            let entry = lines.next().unwrap_or_default();
            let Some((key, value)) = entry.split_once('=') else {
                continue;
            };
            match key {
                "palette" => {
                    if let Some((ix, hex)) = value.split_once('=')
                        && let (Ok(ix), Some(c)) = (ix.parse::<usize>(), color(hex))
                        && ix < 16
                    {
                        ansi[ix] = Some(c);
                    }
                }
                "background" => bg = color(value),
                "foreground" => fg = color(value),
                "cursor-color" => cursor = color(value),
                "selection-background" => selection = color(value),
                _ => {}
            }
        }
        let (Some(background), Some(foreground)) = (bg, fg) else {
            continue;
        };
        if ansi.iter().any(Option::is_none) {
            continue;
        }
        out.push(Scheme {
            name: name.to_owned(),
            background,
            foreground,
            cursor: cursor.unwrap_or(foreground),
            selection: selection.unwrap_or(ansi[8].unwrap_or(foreground)),
            ansi: ansi.map(|c| c.unwrap_or(0)),
        });
    }
    out
}

fn color(hex: &str) -> Option<u32> {
    u32::from_str_radix(hex.trim().trim_start_matches('#'), 16).ok()
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn the_bundle_parses_every_scheme() {
        let all = all();
        assert!(all.len() > 700, "{}", all.len());
        let dracula = find("Dracula").unwrap();
        assert_eq!(dracula.background, 0x282a36);
        assert_eq!(dracula.ansi[1], 0xff5555);
        assert_eq!(dracula.selection, 0x44475a);
    }

    #[test]
    fn incomplete_blocks_are_skipped_and_the_next_one_still_parses() {
        let text = "## Broken\nbackground=#000000\npalette=0=#111111\n\
                    ## Good\nbackground=#101010\nforeground=#eeeeee\n"
            .to_owned()
            + &(0..16)
                .map(|i| format!("palette={i}=#0000{i:02x}\n"))
                .collect::<String>();
        let parsed = parse(&text);
        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0].name, "Good");
        assert_eq!(parsed[0].ansi[15], 0x00000f);
        assert_eq!(parsed[0].cursor, 0xeeeeee);
    }
}
