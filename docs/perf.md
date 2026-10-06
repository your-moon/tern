# Memory and performance

## Where the memory goes

| Item | Cost | Source |
| --- | --- | --- |
| One terminal cell | 24 bytes | `alacritty_terminal` 0.26 `term/cell.rs:303` |
| One scrollback row | columns × 24 B, allocated full-width even when empty | `grid/storage.rs:133` |
| Scrollback growth | lazy, in chunks of ≥1,000 rows; freed when shrunk >1,000 below capacity | `grid/storage.rs:111,133` |
| Full scrollback, 200 cols | 10,000 lines = 48 MB/tab · 5,000 lines = 24 MB/tab | arithmetic |

## SSH session buffers (tern-ssh)

| Buffer | Bound | Source |
| --- | --- | --- |
| russh per-channel queue | 16 messages (russh default 100) | `session.rs` `CHANNEL_BUFFER` |
| Chunk being coalesced | < 64 KiB + one packet | `session.rs` `MAX_CHUNK` |
| Event queue to the UI | 8 chunks | `session.rs` `EVENT_QUEUE` |
| Input queue from the UI | 64 commands, full → `InputError::Busy` | `lib.rs` `COMMAND_QUEUE` |

When the UI falls behind, tern stops reading the channel, russh's queue fills, and russh stops
reading the socket, so the server slows down instead of tern's memory growing.

So the default scrollback is **5,000 lines**, configurable. A fresh tab pays only for what it has printed.

## Paint

| Item | Cost | Source |
| --- | --- | --- |
| Viewport snapshot (`Terminal::lines`), 200×50 | 22 µs/frame, 240 KB transient (freed each frame) | release-build timing, 2026-10-05, #3 |

That is 0.13% of a 60 fps frame, with no steady-state memory, so it stays (#3 closed wontfix).

## Budget

Idle memory with one connected tab, release build: RSS from `scripts/measure-rss.sh` (30 s
window, after connecting) and `footprint -p` (phys_footprint, what Activity Monitor shows; RSS
also counts shared framework pages).
The first measurement sets the baseline; every release records a new row and may not regress >10%.

| Date | Commit | Scenario | RSS |
| --- | --- | --- | --- |
| 2026-10-05 | tern-ssh (uncommitted → first commit) | `examples/connect` release, `seq 1 2000000` (16.9 MB) from strong-b, fast reader | 6 MB peak |
| 2026-10-05 | same | same, reader stalled 6 s (backpressure) | 6 MB peak |
| 2026-10-05 | tern-term | `local_demo` release, one idle local shell, 900×560 window (GPUI + fonts + Metal, no SSH) | 82 MB |
| 2026-10-05 | tern (window shell, #10) | `tern` release, empty main window 1320×880, no session | 66 MB |
| — | — | app with one idle tab | not yet measured |
| 2026-10-05 | 2fbea6e | no tab | 84.4 MB RSS · 44 MB footprint |
| 2026-10-05 | 2fbea6e | one tab, connected to a LAN host, idle shell | 86.6 MB RSS (peak 91.7) · 49 MB footprint |

Baseline, set by the second row: **49 MB footprint / 87 MB RSS**. Measured on an M4 Pro,
macOS 26.6.2, with the screen locked, so no frames were being composited. A connected tab
costs about 5 MB over an empty window. phys_footprint_peak reached 153 MB during start-up
and connection; that transient is not explained yet (likely shader and font atlas set-up).

## 2026-10-06 — 0.1.0 release build, wallpaper on

| Build | Scene | Result |
| --- | --- | --- |
| 0.1.0 release (`/Applications/tern.app`) | window open, Great Wave wallpaper (full window + blurred backdrop), no session, idle 12 s | 83 MB phys_footprint · 127 MB RSS · 186 MB footprint peak |

The start-up peak (F7) is the wallpaper: the picture is decoded at full size, then scaled,
blurred and cached (`wallpaper.rs` `prepare`), and the decode buffers are freed after.

## 2026-10-06 — against other terminal and SSH apps

Method: macOS `footprint -p <pid>`, `phys_footprint` summed over every process whose
executable lives in the app's bundle (Electron apps run several). Apps not already running
were launched hidden (`open -g -j`), left 20 s, measured and quit. iTerm2 and Ghostty were
already running with the owner's sessions and were measured as they were, so they are upper
bounds rather than idle figures.

| App | phys_footprint | Processes | State |
| --- | --- | --- | --- |
| tern 0.1.0 | 83 MB | 1 | fresh, idle, wallpaper on (107 MB with the owner's tabs open) |
| Terminal.app | 30 MB | 1 | fresh |
| iTerm2 | 200 MB | 1 | running, with sessions |
| Ghostty | 339 MB | 1 | running, with sessions |
| Termius | 627 MB | 7 | fresh, idle |
| Tabby | 752 MB | 5 | fresh, idle |

### SSH clients, same protocol (supersedes the rows above for tern, Tabby, Termius)

Each app launched fresh, `phys_footprint` summed over its processes every 15 s for 120 s.
Connected = one tab logged in to `password_server --open --demo` on 127.0.0.1:2399 (the
`--open` flag lets any client in with the `none` method, so no password is typed into any app).
Release build of 0.1.0, notarised bundle.

| App, state | 15s | 30s | 45s | 60s | 75s | 90s | 105s | 120s |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| tern idle, no wallpaper | 67 | 67 | 67 | 67 | 67 | 91 | 86 | 86 |
| tern idle, wallpaper | 124 | 129 | 124 | 129 | 124 | 124 | 107 | 108 |
| tern connected, no wallpaper | 180 | 181 | 180 | 82 | 76 | 76 | 76 | 76 |
| tern connected, wallpaper | 131 | 125 | 125 | 125 | 125 | 125 | 125 | 125 |
| Tabby idle (empty profile) | 555 | 477 | 485 | 474 | 469 | 469 | 470 | 471 |
| Tabby connected | 835 | 811 | 806 | 794 | 587 | 577 | 579 | 580 |
| Termius idle | 632 | 599 | 594 | 589 | 587 | 583 | 581 | 583 |

Notes:
- The first minute is noisy for every app (start-up allocations, the wallpaper decode); the
  README quotes the 120 s value.
- Tabby connected ran with the owner's profile, so two of their old tabs were restored
  (not connected). The empty profile did not connect from the command line, so it gives
  only the idle figure.
- Termius has no connected figure: its quick-connect could not be driven by script.
