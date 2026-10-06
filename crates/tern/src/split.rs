//! The panes of one tab as a binary tree: a leaf is a pane, a split holds two subtrees side
//! by side or one above the other. Closing a pane collapses its parent, so the sibling takes
//! the space. Pure data, so the rules are tested without a window.

/// How a split lays out its two halves.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Axis {
    /// First on the left, second on the right.
    Row,
    /// First on top, second below.
    Column,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    Left,
    Right,
    Up,
    Down,
}

pub type PaneId = u32;

#[derive(Debug, Clone, PartialEq)]
pub enum Node {
    Leaf(PaneId),
    Split {
        axis: Axis,
        first: Box<Node>,
        second: Box<Node>,
    },
}

/// A pane's place in the unit square, as fractions of the tab's area.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl Node {
    /// Every pane, left to right and top to bottom.
    pub fn leaves(&self) -> Vec<PaneId> {
        match self {
            Node::Leaf(id) => vec![*id],
            Node::Split { first, second, .. } => {
                let mut all = first.leaves();
                all.extend(second.leaves());
                all
            }
        }
    }

    pub fn contains(&self, id: PaneId) -> bool {
        match self {
            Node::Leaf(leaf) => *leaf == id,
            Node::Split { first, second, .. } => first.contains(id) || second.contains(id),
        }
    }

    /// Splits pane `target` in two along `axis`; `new` goes second (right or below). False
    /// when `target` is not in the tree.
    pub fn split(&mut self, target: PaneId, axis: Axis, new: PaneId) -> bool {
        match self {
            Node::Leaf(id) if *id == target => {
                *self = Node::Split {
                    axis,
                    first: Box::new(Node::Leaf(target)),
                    second: Box::new(Node::Leaf(new)),
                };
                true
            }
            Node::Leaf(_) => false,
            Node::Split { first, second, .. } => {
                first.split(target, axis, new) || second.split(target, axis, new)
            }
        }
    }

    /// Removes pane `id`; its sibling takes the parent's place. `None` when `id` was the only
    /// pane (the caller closes the tab), and the tree unchanged when `id` is not in it.
    pub fn remove(self, id: PaneId) -> Option<Node> {
        match self {
            Node::Leaf(leaf) if leaf == id => None,
            leaf @ Node::Leaf(_) => Some(leaf),
            Node::Split {
                axis,
                first,
                second,
            } => match (first.remove(id), second.remove(id)) {
                // Only one side can hold the pane; the other comes back whole, so a side that
                // disappeared leaves its sibling to take over.
                (None, Some(rest)) | (Some(rest), None) => Some(rest),
                (Some(first), Some(second)) => Some(Node::Split {
                    axis,
                    first: Box::new(first),
                    second: Box::new(second),
                }),
                (None, None) => None,
            },
        }
    }

    /// Where each pane sits when the whole tree fills `area`, every split cut in half.
    pub fn layout(&self, area: Rect) -> Vec<(PaneId, Rect)> {
        match self {
            Node::Leaf(id) => vec![(*id, area)],
            Node::Split {
                axis,
                first,
                second,
            } => {
                let (a, b) = match axis {
                    Axis::Row => {
                        let w = area.w / 2.0;
                        (
                            Rect { w, ..area },
                            Rect {
                                x: area.x + w,
                                w,
                                ..area
                            },
                        )
                    }
                    Axis::Column => {
                        let h = area.h / 2.0;
                        (
                            Rect { h, ..area },
                            Rect {
                                y: area.y + h,
                                h,
                                ..area
                            },
                        )
                    }
                };
                let mut all = first.layout(a);
                all.extend(second.layout(b));
                all
            }
        }
    }

    /// The pane next to `from` in `dir`: the one that touches its edge and shares the most of
    /// it. `None` at the edge of the tab.
    pub fn neighbour(&self, from: PaneId, dir: Direction) -> Option<PaneId> {
        const EPS: f32 = 1e-4;
        let all = self.layout(Rect {
            x: 0.0,
            y: 0.0,
            w: 1.0,
            h: 1.0,
        });
        let (_, me) = all.iter().find(|(id, _)| *id == from)?;
        let overlap = |a0: f32, a1: f32, b0: f32, b1: f32| (a1.min(b1) - a0.max(b0)).max(0.0);
        all.iter()
            .filter(|(id, _)| *id != from)
            .filter_map(|(id, r)| {
                let (touches, shared) = match dir {
                    Direction::Left => (
                        (r.x + r.w - me.x).abs() < EPS,
                        overlap(me.y, me.y + me.h, r.y, r.y + r.h),
                    ),
                    Direction::Right => (
                        (me.x + me.w - r.x).abs() < EPS,
                        overlap(me.y, me.y + me.h, r.y, r.y + r.h),
                    ),
                    Direction::Up => (
                        (r.y + r.h - me.y).abs() < EPS,
                        overlap(me.x, me.x + me.w, r.x, r.x + r.w),
                    ),
                    Direction::Down => (
                        (me.y + me.h - r.y).abs() < EPS,
                        overlap(me.x, me.x + me.w, r.x, r.x + r.w),
                    ),
                };
                (touches && shared > EPS).then_some((*id, shared))
            })
            // The most shared edge wins; on a tie the earlier pane (top or left) does.
            .fold(None::<(PaneId, f32)>, |best, (id, shared)| match best {
                Some((_, s)) if s >= shared - EPS => best,
                _ => Some((id, shared)),
            })
            .map(|(id, _)| id)
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    /// Pane 1 on the left; 2 top right; 3 bottom right.
    fn l_shape() -> Node {
        let mut t = Node::Leaf(1);
        assert!(t.split(1, Axis::Row, 2));
        assert!(t.split(2, Axis::Column, 3));
        t
    }

    #[test]
    fn splitting_puts_the_new_pane_second_and_keeps_order() {
        let t = l_shape();
        assert_eq!(t.leaves(), [1, 2, 3]);
        assert_eq!(
            t,
            Node::Split {
                axis: Axis::Row,
                first: Box::new(Node::Leaf(1)),
                second: Box::new(Node::Split {
                    axis: Axis::Column,
                    first: Box::new(Node::Leaf(2)),
                    second: Box::new(Node::Leaf(3)),
                }),
            }
        );
    }

    #[test]
    fn splitting_a_missing_pane_changes_nothing() {
        let mut t = l_shape();
        assert!(!t.split(9, Axis::Row, 4));
        assert_eq!(t.leaves(), [1, 2, 3]);
    }

    #[test]
    fn closing_a_pane_collapses_its_parent_into_the_sibling() {
        // Closing 3 leaves 2 to fill the right half: the column split is gone.
        let t = l_shape().remove(3).unwrap();
        assert_eq!(
            t,
            Node::Split {
                axis: Axis::Row,
                first: Box::new(Node::Leaf(1)),
                second: Box::new(Node::Leaf(2)),
            }
        );
        // Closing 1 leaves the column alone: it takes the whole tab.
        let t = l_shape().remove(1).unwrap();
        assert_eq!(
            t,
            Node::Split {
                axis: Axis::Column,
                first: Box::new(Node::Leaf(2)),
                second: Box::new(Node::Leaf(3)),
            }
        );
    }

    #[test]
    fn the_last_pane_cannot_be_removed_and_unknown_ones_change_nothing() {
        assert_eq!(Node::Leaf(1).remove(1), None);
        assert_eq!(l_shape().remove(9), Some(l_shape()));
    }

    #[test]
    fn focus_moves_to_the_pane_that_touches_the_edge() {
        let t = l_shape();
        assert_eq!(
            t.neighbour(1, Direction::Right),
            Some(2),
            "top wins the tie"
        );
        assert_eq!(t.neighbour(2, Direction::Left), Some(1));
        assert_eq!(t.neighbour(3, Direction::Left), Some(1));
        assert_eq!(t.neighbour(2, Direction::Down), Some(3));
        assert_eq!(t.neighbour(3, Direction::Up), Some(2));
    }

    #[test]
    fn there_is_no_neighbour_past_the_edge() {
        let t = l_shape();
        assert_eq!(t.neighbour(1, Direction::Left), None);
        assert_eq!(t.neighbour(1, Direction::Up), None);
        assert_eq!(t.neighbour(2, Direction::Up), None);
        assert_eq!(t.neighbour(3, Direction::Down), None);
        assert_eq!(t.neighbour(3, Direction::Right), None);
        assert_eq!(Node::Leaf(1).neighbour(1, Direction::Right), None);
    }

    fn leaf(id: PaneId) -> Box<Node> {
        Box::new(Node::Leaf(id))
    }

    fn pair(axis: Axis, first: Box<Node>, second: Box<Node>) -> Box<Node> {
        Box::new(Node::Split {
            axis,
            first,
            second,
        })
    }

    #[test]
    fn the_neighbour_sharing_most_of_the_edge_wins() {
        // Pane 1 is tall on the left. On the right 3 and 4 share the top half and 5 has the
        // bottom half. Going right from 1, pane 5 shares half of the edge, 3 and 4 a quarter.
        let right = pair(Axis::Column, pair(Axis::Column, leaf(3), leaf(4)), leaf(5));
        let mut t = *pair(Axis::Row, leaf(1), right);
        assert_eq!(t.leaves(), [1, 3, 4, 5]);
        assert_eq!(t.neighbour(1, Direction::Right), Some(5));
        assert_eq!(t.neighbour(3, Direction::Left), Some(1));
        // Cut pane 1 into two stacked halves: from 5 only the lower one touches; from 3 only
        // the upper one does.
        assert!(t.split(1, Axis::Column, 2));
        assert_eq!(t.neighbour(5, Direction::Left), Some(2));
        assert_eq!(t.neighbour(3, Direction::Left), Some(1));
    }
}
