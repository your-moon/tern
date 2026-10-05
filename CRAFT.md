# Craft rules

tern is held to a higher bar than the projects it copies from. These rules are enforced by
`cargo clippy -D warnings`, `scripts/check-craft.sh` and CI, not by good intentions.

- **No panics in shipped code.** `unwrap`, `expect`, `panic!`, `todo!` are denied outside tests.
  Errors are typed (`thiserror`) in libraries and reported to the user, never swallowed.
- **No `unsafe`.** Forbidden workspace-wide.
- **No god-files.** Every source file stays under 800 lines. Split by responsibility, not by size.
- **Copy, don't invent — and say so.** Copied code keeps a `// Adapted from <repo> <path> (<license>).` header and is listed in `NOTICE.md`.
- **Small seams.** `tern-ssh` knows nothing about GPUI; `tern-term` knows nothing about SSH. The app wires them.
- **Idle means idle.** No per-frame loops; repaint only on new bytes, input or resize. Memory is a feature:
  idle RSS with one tab is measured on every release and must not regress.
- **Memory is designed, not hoped for.** See `docs/perf.md`.
  - Scrollback is bounded (default 5,000 lines) because it dominates: 24 B per cell, full-width rows.
  - Every channel between SSH and UI is **bounded**. A fast `cat` must apply backpressure to the
    socket, never pile up in an unbounded queue.
  - One tokio runtime, on one dedicated thread, for all SSH I/O. No thread per connection.
  - No per-frame allocation in paint: reuse buffers; don't clone the grid to draw it.
  - No new dependency without `default-features = false` and only the features used.
  - Optimise only with a measurement before and after, recorded in `docs/perf.md`.
- **Security is not optional.** Host-key changes are a hard stop. Secrets never reach logs.
- **Tests that can fail.** Risky logic (key encoding, host-key checks, parsing) gets tests that were
  mutation-checked: break the code, watch the test go red.
- **Rust practice.** Typed errors with `thiserror` in libraries, `anyhow` only in the binary.
  Borrow instead of clone, `&str`/`&[u8]` in APIs, `#[must_use]` on handles and results that matter.
  GPUI entities for UI state, never `Arc<Mutex<_>>`. `cargo deny` gates licenses (GPL-compatible),
  advisories and sources.
- **Verified by eye.** UI changes are checked by running the app and looking at a screenshot.
