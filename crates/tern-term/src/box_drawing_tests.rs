#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use super::*;

#[test]
fn box_segments_cover_the_box_drawing_block_only() {
    for ch in ['─', '│', '┌', '┼', '╬', '╿'] {
        assert!(
            get_box_segments(ch).is_some(),
            "{ch:?} should be drawn as a path"
        );
    }
    for ch in ['A', ' ', '█'] {
        assert!(
            get_box_segments(ch).is_none(),
            "{ch:?} should be left to the font"
        );
    }
}

#[test]
fn test_get_box_segments_horizontal() {
    let seg = get_box_segments('─').unwrap();
    assert_eq!(seg.left, Some(Light));
    assert_eq!(seg.right, Some(Light));
    assert_eq!(seg.top, None);
    assert_eq!(seg.bottom, None);
}

#[test]
fn test_get_box_segments_vertical() {
    let seg = get_box_segments('│').unwrap();
    assert_eq!(seg.left, None);
    assert_eq!(seg.right, None);
    assert_eq!(seg.top, Some(Light));
    assert_eq!(seg.bottom, Some(Light));
}

#[test]
fn test_get_box_segments_corner() {
    let seg = get_box_segments('┌').unwrap();
    assert_eq!(seg.left, None);
    assert_eq!(seg.right, Some(Light));
    assert_eq!(seg.top, None);
    assert_eq!(seg.bottom, Some(Light));
}

#[test]
fn test_get_box_segments_cross() {
    let seg = get_box_segments('┼').unwrap();
    assert_eq!(seg.left, Some(Light));
    assert_eq!(seg.right, Some(Light));
    assert_eq!(seg.top, Some(Light));
    assert_eq!(seg.bottom, Some(Light));
}

#[test]
fn test_get_box_segments_double() {
    let seg = get_box_segments('═').unwrap();
    assert_eq!(seg.left, Some(Double));
    assert_eq!(seg.right, Some(Double));
    assert_eq!(seg.top, None);
    assert_eq!(seg.bottom, None);
}

#[test]
fn test_get_box_segments_invalid() {
    assert!(get_box_segments('A').is_none());
    assert!(get_box_segments(' ').is_none());
}
