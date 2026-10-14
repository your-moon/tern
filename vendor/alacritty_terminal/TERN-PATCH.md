# tern patch to alacritty_terminal 0.26.0

Upstream stores every scrollback row at full terminal width, 24 bytes per cell. This copy trims a
row to the cells it uses when the row moves into history. Everything else is upstream 0.26.0
(commit "vendor alacritty_terminal 0.26.0 unmodified" holds the pristine copy; diff against it).

Representation: a row in history keeps its cells up to the last one that differs from the final
cell, plus one more. That last stored cell stands for every dropped cell, which all equal it. Trimming
looks at the cells, never at `occ`: upstream's `occ` is not always tight (`grow` leaves it stale
when the old tail was not default), and trusting it changed what `reset` produced. `occ` is never
modified by trimming or expanding, so a row has the same cells and the same `occ` as upstream's.

## Differences

`src/grid/row.rs`
- New field `Row::missing`: the number of dropped trailing cells (0 for a full row; serde default 0).
  `len()` is `inner.len() + missing`, so the logical width never changes.
- New `trim()` (drops the run of equal cells at the end, `shrink_to_fit`), `expand()` (restores the tail from the last stored
  cell), `writable()` (expand if trimmed), `is_trimmed()`, doc(hidden) `stored_len()`.
- `Index<Column>`: a read past the stored cells returns the last stored cell.
- Every `IndexMut`, `last_mut`, `append`, `append_front`, `front_split_off` and `&mut Row` iteration
  expands first. They now need `T: Clone`.
- `Index<Range*>` / `Index<RangeFull>` (slices) panic on a trimmed row instead of returning a short
  slice; nothing in alacritty_terminal or tern reads a history row that way (see `cell.rs`).
- `&Row` iteration is a `Chain` of the stored cells and the repeated fill (type `row::Iter`).
- `PartialEq` compares logical cells. `grow` / `shrink` keep a trimmed row trimmed when only fill
  is added or removed, else expand first. `reset` expands a trimmed row, then runs upstream's code.

`src/grid/mod.rs`
- `Grid::scroll_up` (`region.start == 0`): trims the `positions` top rows before they rotate into
  history, when `max_scroll_limit > 0`.
- `impl Grid<T>` blocks, `reset_region`, `IndexMut<Point>` and `cursor_cell` gain `T: Clone`.

`src/grid/resize.rs`
- `resize`, after the line and column changes: expands every screen row (rows move between history
  and the screen on a taller screen and when reflow merges lines) and trims every history row
  (rows come out of reflow at full width).

`src/grid/storage.rs`
- `Storage::swap` is `Vec::swap`. Upstream swapped four raw words, which no longer covers a `Row`.

`src/term/cell.rs`
- `LineLength::line_length` iterates `self.into_iter().rev()` instead of slicing `self[..]`.

Not changed: public types and method signatures that tern-term calls, `Cargo.toml`, tests.
