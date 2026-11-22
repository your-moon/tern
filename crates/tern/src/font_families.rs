// Adapted from zeron apps/zgx/src/fonts.rs (MIT), at b289003b.
//! Installed families with Latin letters and their measured suitability for a terminal grid.

use std::collections::BTreeMap;
use std::sync::OnceLock;

static FAMILIES: OnceLock<Vec<(String, bool)>> = OnceLock::new();
const BUNDLED: [&str; 2] = [crate::theme::UI_FONT, crate::theme::MONO_FONT];

pub(super) fn cached() -> Option<&'static [(String, bool)]> {
    FAMILIES.get().map(Vec::as_slice)
}

pub(super) fn load() -> &'static [(String, bool)] {
    FAMILIES.get_or_init(installed)
}

/// The system's families (fontdb reads the directories zgx's text system
/// does), sorted by name, but the app's own and hidden ones (a leading `.`,
/// macOS's system UI faces).
fn installed() -> Vec<(String, bool)> {
    let mut database = fontdb::Database::new();
    database.load_system_fonts();
    let mut families: BTreeMap<String, bool> = BTreeMap::new();
    for face in database.faces() {
        let Some(Some(fixed)) = database.with_face_data(face.id, |data, index| {
            ttf_parser::Face::parse(data, index)
                .ok()
                .filter(|font| font.glyph_index('m').is_some())
                .map(|font| is_fixed_width(&font))
        }) else {
            continue;
        };
        for (name, _) in &face.families {
            // Choosing a family chooses all its faces: one proportional
            // italic is enough to break a grid of cells.
            families
                .entry(name.clone())
                .and_modify(|all| *all &= fixed)
                .or_insert(fixed);
        }
    }
    listed(families)
}

/// The families a picker offers: none hidden, none of the app's own.
fn listed(families: BTreeMap<String, bool>) -> Vec<(String, bool)> {
    families
        .into_iter()
        .filter(|(name, _)| {
            !name.is_empty() && !name.starts_with('.') && !BUNDLED.contains(&name.as_str())
        })
        .collect()
}

/// How far apart the sampled advances may be and still count as one width:
/// hinting leaves sub-unit differences in real monospaced faces, where a
/// proportional face's differ by far more.
const FIXED_WIDTH_TOLERANCE: f32 = 0.01;

/// Whether every Latin cell of `font` is the same width, measured (a font's
/// own "monospaced" flag is metadata it can get wrong), as the shipping
/// app's `face_is_fixed_width`.
fn is_fixed_width(font: &ttf_parser::Face) -> bool {
    let Some(advances) = ['i', 'm', 'W', '0']
        .iter()
        .map(|ch| {
            font.glyph_index(*ch)
                .and_then(|id| font.glyph_hor_advance(id))
        })
        .collect::<Option<Vec<_>>>()
    else {
        return false;
    };
    let min = advances.iter().copied().min().unwrap_or(0);
    let max = advances.iter().copied().max().unwrap_or(0);
    min > 0 && f32::from(max - min) <= FIXED_WIDTH_TOLERANCE * f32::from(min)
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    fn face(bytes: &[u8]) -> ttf_parser::Face<'_> {
        ttf_parser::Face::parse(bytes, 0).unwrap()
    }

    #[test]
    fn geist_mono_is_fixed_width_and_geist_is_not() {
        assert!(is_fixed_width(&face(include_bytes!(
            "../assets/fonts/GeistMono.ttf"
        ))));
        assert!(is_fixed_width(&face(include_bytes!(
            "../assets/fonts/GeistMono-Bold.ttf"
        ))));
        assert!(!is_fixed_width(&face(include_bytes!(
            "../assets/fonts/Geist.ttf"
        ))));
        assert!(!is_fixed_width(&face(include_bytes!(
            "../assets/fonts/InstrumentSerif-Regular.ttf"
        ))));
    }

    #[test]
    fn the_app_s_own_and_hidden_families_are_left_out() {
        let families = BTreeMap::from([
            ("Geist".to_string(), false),
            ("Geist Mono".to_string(), true),
            (".SF NS".to_string(), false),
            ("Inter".to_string(), false),
            ("JetBrains Mono".to_string(), true),
        ]);
        assert_eq!(
            listed(families),
            vec![
                ("Inter".to_string(), false),
                ("JetBrains Mono".to_string(), true)
            ]
        );
    }
}
