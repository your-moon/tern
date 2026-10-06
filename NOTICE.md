# Third-party code

tern is GPL-3.0-or-later. It copies and adapts code from the projects below; each copied file keeps a header naming its origin.

| Source | License | What |
| --- | --- | --- |
| zed-industries/zed (`crates/terminal`, `crates/terminal_view`) | GPL-3.0 | terminal model, grid element, key/mouse mappings |
| zeronsh/zeron | MIT | UI style, terminal emulator wrapper (alacritty_terminal 0.26); wallpaper effects (none, scanlines, ASCII, halftone, dither) (`crates/mobile/src/wallpaper.rs`), dominant-colour extraction and the surface tint (`crates/ui/src/settings/wallpaper_colors.rs` `tint_variant`), text hardening (`crates/ui/src/theme.rs` `harden_model_foreground`), the hero's size (`crates/ui/src/shell.rs` `new_thread_background_height`), recent-wallpaper history and the 180 ms crossfade (`crates/ui/src/settings/wallpaper.rs`, `motion.rs`); file-browser row shape and washes (`crates/ui/src/files/tree.rs`), port-forward and file-browser popover card (`crates/ui/src/popover.rs`) |
| Eugeny/russh (examples) | Apache-2.0 | SSH client session, auth, known_hosts, jump hosts, port forwarding and agent forwarding |
| chi11321/CrabPort | Apache-2.0 | SSH terminal backend shape (resize, auth flow) |
| longbridge/gpui-component (stories/examples) | Apache-2.0 | tabs, sidebar, dialogs |
| alacritty/alacritty (`alacritty_terminal` crate) | Apache-2.0 | VT emulation (dependency) |
| Eugeny/russh-sftp | Apache-2.0 | SFTP client protocol (dependency), usage after its `sftp_client` example |
| veeso/ssh2-config | MIT | `~/.ssh/config` parsing (dependency) |
| mbadolato/iTerm2-Color-Schemes (Ghostty format) | MIT | terminal colour schemes, `crates/tern/assets/themes.txt` |
| Solar Icons by 480 Design (via zeron) | CC BY 4.0 | UI icons, `crates/tern/assets/icons` |
| daangn/seed-design | MIT | snackbar sizes, colours, motion and behaviour |
| vercel/geist-font (via zeron) | SIL OFL 1.1 | Geist and Geist Mono fonts, `crates/tern/assets/fonts` |
