# Memory and performance

## Where the memory goes

| Item | Cost | Source |
| --- | --- | --- |
| One terminal cell | 24 bytes | `alacritty_terminal` 0.26 `term/cell.rs:303` |
| One scrollback row | columns × 24 B, allocated full-width even when empty | `grid/storage.rs:133` |
| Scrollback growth | lazy, in chunks of ≥1,000 rows; freed when shrunk >1,000 below capacity | `grid/storage.rs:111,133` |
| Full scrollback, 200 cols | 10,000 lines = 48 MB/tab · 5,000 lines = 24 MB/tab | arithmetic |

So the default scrollback is **5,000 lines**, configurable. A fresh tab pays only for what it has printed.

## Budget

Idle RSS with one connected tab, release build, measured with `scripts/measure-rss.sh`.
The first measurement sets the baseline; every release records a new row and may not regress >10%.

| Date | Commit | Scenario | RSS |
| --- | --- | --- | --- |
| — | — | baseline not yet measured | — |
