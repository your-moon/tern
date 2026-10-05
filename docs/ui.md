# UI direction

The UI is modeled on zeron (https://github.com/zeronsh/zeron, MIT). We copy its look, not its code paths.
Reference screenshot: `apps/landing/public/assets/app-screenshot.jpg` in that repo.

What we take from it:

- Transparent titlebar: macOS traffic lights inline with the chrome, then sidebar toggle, then `+` (new connection).
- Title line: bold session name, then muted `user@host` (zeron shows `comet @ personal-metal` the same way).
- Dark, frosted surface with a faint tint; no hard window borders; content floats on it.
- Left sidebar (toggleable): saved hosts from `~/.ssh/config` + tern's own list, grouped, searchable.
- Main area: the terminal grid fills the pane edge to edge; tabs live in the titlebar strip.
- Optional right pane later (SFTP browser), same split style as zeron's diff pane.
- Rounded, low-contrast controls; muted secondary text; one accent color.
- Motion: zeron's `motion.rs` timings (200ms ease-out pane width, 150ms tab slide) — values only.
