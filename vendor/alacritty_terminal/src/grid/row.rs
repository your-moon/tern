//! Defines the Row type which makes up lines in the grid.

use std::cmp::{max, min};
use std::ops::{Index, IndexMut, Range, RangeFrom, RangeFull, RangeTo, RangeToInclusive};
use std::{ptr, slice};

#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

use crate::grid::GridCell;
use crate::index::Column;
use crate::term::cell::ResetDiscriminant;

/// A row in the grid.
#[derive(Default, Clone, Debug)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct Row<T> {
    /// Stored cells. A row in history keeps only `occ + 1` of them (see `missing`).
    inner: Vec<T>,

    /// Number of cells dropped from the end of a trimmed row, 0 for a full one. Each of them
    /// equals the last stored cell, so the row is `inner.len() + missing` columns wide.
    // tern: trimmed history rows.
    #[cfg_attr(feature = "serde", serde(default))]
    missing: usize,

    /// Maximum number of occupied entries.
    ///
    /// This is the upper bound on the number of elements in the row, which have been modified
    /// since the last reset. All cells after this point are guaranteed to be equal.
    pub(crate) occ: usize,
}

impl<T: PartialEq> PartialEq for Row<T> {
    fn eq(&self, other: &Self) -> bool {
        if self.len() != other.len() {
            return false;
        }
        if self.inner.len() == other.inner.len() {
            return self.inner == other.inner;
        }
        // One side is trimmed: compare the logical cells.
        let stored = max(self.inner.len(), other.inner.len());
        (0..stored).all(|i| self.cell(i) == other.cell(i))
    }
}

impl<T: Default> Row<T> {
    /// Create a new terminal row.
    ///
    /// Ideally the `template` should be `Copy` in all performance sensitive scenarios.
    pub fn new(columns: usize) -> Row<T> {
        debug_assert!(columns >= 1);

        let mut inner: Vec<T> = Vec::with_capacity(columns);

        // This is a slightly optimized version of `std::vec::Vec::resize`.
        unsafe {
            let mut ptr = inner.as_mut_ptr();

            for _ in 1..columns {
                ptr::write(ptr, T::default());
                ptr = ptr.offset(1);
            }
            ptr::write(ptr, T::default());

            inner.set_len(columns);
        }

        Row { inner, missing: 0, occ: 0 }
    }

    /// Increase the number of columns in the row.
    #[inline]
    pub fn grow(&mut self, columns: usize)
    where
        T: Clone + PartialEq,
    {
        if self.len() >= columns {
            return;
        }

        if self.is_trimmed() {
            // The new cells are default; the row's blank fill must match them to stay trimmed.
            if self.inner[self.inner.len() - 1] == T::default() {
                self.missing += columns - self.len();
                return;
            }
            self.expand();
        }

        self.inner.resize_with(columns, T::default);
    }

    /// Reduce the number of columns in the row.
    ///
    /// This will return all non-empty cells that were removed.
    pub fn shrink(&mut self, columns: usize) -> Option<Vec<T>>
    where
        T: GridCell + Clone,
    {
        if self.len() <= columns {
            return None;
        }

        if self.is_trimmed() {
            if columns >= self.inner.len() {
                // Only blank fill is removed.
                let removed = self.len() - columns;
                self.missing = columns - self.inner.len();
                self.occ = min(self.occ, columns);
                let fill = &self.inner[self.inner.len() - 1];
                return if fill.is_empty() { None } else { Some(vec![fill.clone(); removed]) };
            }
            self.expand();
        }

        // Split off cells for a new row.
        let mut new_row = self.inner.split_off(columns);
        let index = new_row.iter().rposition(|c| !c.is_empty()).map_or(0, |i| i + 1);
        new_row.truncate(index);

        self.occ = min(self.occ, columns);

        if new_row.is_empty() { None } else { Some(new_row) }
    }

    /// Reset all cells in the row to the `template` cell.
    #[inline]
    pub fn reset<D>(&mut self, template: &T)
    where
        T: ResetDiscriminant<D> + GridCell + Clone,
        D: PartialEq,
    {
        debug_assert!(!self.inner.is_empty());

        // A trimmed row is about to become a screen row again; from here on it is the row
        // upstream would have, cells and `occ` alike.
        if self.is_trimmed() {
            self.expand();
        }

        // Mark all cells as dirty if template cell changed.
        let len = self.inner.len();
        if self.inner[len - 1].discriminant() != template.discriminant() {
            self.occ = len;
        }

        // Reset every dirty cell in the row.
        for item in &mut self.inner[0..self.occ] {
            item.reset(template);
        }

        self.occ = 0;
    }
}

impl<T> Row<T> {
    /// Cell `i` of a possibly trimmed row, `i` below `len()`.
    #[inline]
    fn cell(&self, i: usize) -> &T {
        match self.inner.get(i) {
            Some(cell) => cell,
            None => &self.inner[self.inner.len() - 1],
        }
    }

    #[inline]
    fn is_trimmed(&self) -> bool {
        self.missing > 0
    }

    /// Number of cells actually stored, `len()` unless the row is trimmed.
    #[doc(hidden)]
    #[inline]
    pub fn stored_len(&self) -> usize {
        self.inner.len()
    }

    /// Drop the run of equal cells at the end of a row that is moving into history, keeping one.
    ///
    /// Looks at the cells, not at `occ`: upstream's `occ` bound is not always tight (and `grow`
    /// can leave it stale), so trusting it could change what the row reads. `occ` is untouched.
    // tern: trimmed history rows.
    #[inline]
    pub(crate) fn trim(&mut self)
    where
        T: PartialEq,
    {
        let Some(fill) = self.inner.last() else {
            return;
        };
        let keep = self.inner.iter().rposition(|cell| cell != fill).map_or(1, |i| i + 2);
        if keep < self.inner.len() {
            self.missing += self.inner.len() - keep;
            self.inner.truncate(keep);
            self.inner.shrink_to_fit();
        }
    }
}

impl<T: Clone> Row<T> {
    /// Give a trimmed row all of its cells back.
    #[inline]
    pub(crate) fn expand(&mut self) {
        if !self.is_trimmed() {
            return;
        }
        if let Some(fill) = self.inner.last().cloned() {
            self.inner.resize(self.len(), fill);
            self.missing = 0;
        }
    }

    /// Mutable access to every cell: a trimmed row is expanded first.
    #[inline]
    fn writable(&mut self) -> &mut Vec<T> {
        if self.is_trimmed() {
            self.expand();
        }
        &mut self.inner
    }
}

#[allow(clippy::len_without_is_empty)]
impl<T> Row<T> {
    #[inline]
    pub fn from_vec(vec: Vec<T>, occ: usize) -> Row<T> {
        Row { inner: vec, missing: 0, occ }
    }

    #[inline]
    pub fn len(&self) -> usize {
        self.inner.len() + self.missing
    }

    #[inline]
    pub fn last(&self) -> Option<&T> {
        self.inner.last()
    }

    /// Check if all cells in the row are empty.
    #[inline]
    pub fn is_clear(&self) -> bool
    where
        T: GridCell,
    {
        // The last stored cell is the blank fill of a trimmed row, so this covers it.
        self.inner.iter().all(GridCell::is_empty)
    }
}

impl<T: Clone> Row<T> {
    #[inline]
    pub fn last_mut(&mut self) -> Option<&mut T> {
        self.occ = self.len();
        self.writable().last_mut()
    }

    #[inline]
    pub fn append(&mut self, vec: &mut Vec<T>)
    where
        T: GridCell,
    {
        self.occ += vec.len();
        self.writable().append(vec);
    }

    #[inline]
    pub fn append_front(&mut self, mut vec: Vec<T>) {
        self.occ += vec.len();

        vec.append(self.writable());
        self.inner = vec;
    }

    #[inline]
    pub fn front_split_off(&mut self, at: usize) -> Vec<T> {
        self.occ = self.occ.saturating_sub(at);

        let mut split = self.writable().split_off(at);
        std::mem::swap(&mut split, &mut self.inner);
        split
    }
}

/// Iterator over the cells of a possibly trimmed row: the stored cells, then the fill.
pub type Iter<'a, T> =
    std::iter::Chain<slice::Iter<'a, T>, std::iter::Flatten<std::iter::RepeatN<Option<&'a T>>>>;

impl<'a, T> IntoIterator for &'a Row<T> {
    type IntoIter = Iter<'a, T>;
    type Item = &'a T;

    #[inline]
    fn into_iter(self) -> Iter<'a, T> {
        let missing = self.missing;
        self.inner.iter().chain(std::iter::repeat_n(self.inner.last(), missing).flatten())
    }
}

impl<'a, T: Clone> IntoIterator for &'a mut Row<T> {
    type IntoIter = slice::IterMut<'a, T>;
    type Item = &'a mut T;

    #[inline]
    fn into_iter(self) -> slice::IterMut<'a, T> {
        self.occ = self.len();
        self.writable().iter_mut()
    }
}

impl<T> Index<Column> for Row<T> {
    type Output = T;

    #[inline]
    fn index(&self, index: Column) -> &T {
        match self.inner.get(index.0) {
            Some(cell) => cell,
            None => {
                assert!(index.0 < self.len(), "column {} out of range for {} columns", index.0, self.len());
                &self.inner[self.inner.len() - 1]
            },
        }
    }
}

impl<T: Clone> IndexMut<Column> for Row<T> {
    #[inline]
    fn index_mut(&mut self, index: Column) -> &mut T {
        self.occ = max(self.occ, *index + 1);
        &mut self.writable()[index.0]
    }
}

/// A slice of a trimmed row can only be handed out for the cells that are stored.
#[inline]
fn stored<T>(row: &Row<T>, end: usize) -> &[T] {
    assert!(end <= row.inner.len(), "slice of a trimmed row past its stored cells");
    &row.inner[..]
}

impl<T> Index<Range<Column>> for Row<T> {
    type Output = [T];

    #[inline]
    fn index(&self, index: Range<Column>) -> &[T] {
        &stored(self, index.end.0)[(index.start.0)..(index.end.0)]
    }
}

impl<T: Clone> IndexMut<Range<Column>> for Row<T> {
    #[inline]
    fn index_mut(&mut self, index: Range<Column>) -> &mut [T] {
        self.occ = max(self.occ, *index.end);
        &mut self.writable()[(index.start.0)..(index.end.0)]
    }
}

impl<T> Index<RangeTo<Column>> for Row<T> {
    type Output = [T];

    #[inline]
    fn index(&self, index: RangeTo<Column>) -> &[T] {
        &stored(self, index.end.0)[..(index.end.0)]
    }
}

impl<T: Clone> IndexMut<RangeTo<Column>> for Row<T> {
    #[inline]
    fn index_mut(&mut self, index: RangeTo<Column>) -> &mut [T] {
        self.occ = max(self.occ, *index.end);
        &mut self.writable()[..(index.end.0)]
    }
}

impl<T> Index<RangeFrom<Column>> for Row<T> {
    type Output = [T];

    #[inline]
    fn index(&self, index: RangeFrom<Column>) -> &[T] {
        &stored(self, self.len())[(index.start.0)..]
    }
}

impl<T: Clone> IndexMut<RangeFrom<Column>> for Row<T> {
    #[inline]
    fn index_mut(&mut self, index: RangeFrom<Column>) -> &mut [T] {
        self.occ = self.len();
        &mut self.writable()[(index.start.0)..]
    }
}

impl<T> Index<RangeFull> for Row<T> {
    type Output = [T];

    #[inline]
    fn index(&self, _: RangeFull) -> &[T] {
        &stored(self, self.len())[..]
    }
}

impl<T: Clone> IndexMut<RangeFull> for Row<T> {
    #[inline]
    fn index_mut(&mut self, _: RangeFull) -> &mut [T] {
        self.occ = self.len();
        &mut self.writable()[..]
    }
}

impl<T> Index<RangeToInclusive<Column>> for Row<T> {
    type Output = [T];

    #[inline]
    fn index(&self, index: RangeToInclusive<Column>) -> &[T] {
        &stored(self, index.end.0 + 1)[..=(index.end.0)]
    }
}

impl<T: Clone> IndexMut<RangeToInclusive<Column>> for Row<T> {
    #[inline]
    fn index_mut(&mut self, index: RangeToInclusive<Column>) -> &mut [T] {
        self.occ = max(self.occ, *index.end + 1);
        &mut self.writable()[..=(index.end.0)]
    }
}
