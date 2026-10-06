# Changelog

All notable changes to tern are recorded here. The format follows [Keep a Changelog 1.1.0](https://keepachangelog.com/en/1.1.0/), and versions follow [Semantic Versioning](https://semver.org/spec/v2.0.0.html). Until 1.0, a minor version may break things.

## [Unreleased]

### Added

- `tern-ssh`: SSH sessions over russh with ssh-agent, key-file (with passphrase), password and keyboard-interactive login; `~/.ssh/config` hosts and `ProxyCommand`; `known_hosts` checking where a changed key refuses the connection and an unknown key asks first.
- `tern-ssh`: bounded output path. Output is coalesced into 64 KiB chunks over an 8-deep queue, and the session stops reading the socket when the UI falls behind; a 16.9 MB burst peaks at 6 MB RSS. Input reports `Busy` instead of queueing without limit.
- `tern-term`: a GPUI terminal view fed by any byte stream, with xterm-256color and truecolor, text attributes, wide characters, box drawing painted as paths, mouse reporting, selection and bracketed paste.
- `tern-ssh`: password, keyboard-interactive and encrypted-key logins are tested against an in-process russh server, including retries after a wrong answer and cancelling.
- `tern`: the app window, with zeron's chrome: transparent 38 pt titlebar, traffic lights at (14, 14), frosted shell over a blurred background, a 10 pt rounded main panel, bundled Geist fonts and structured JSON logs (`TERN_LOG`). Idle: 66 MB.
- `tern`: a host sidebar listing every concrete `Host` in `~/.ssh/config`; clicking one opens its SSH session in the main panel, with a status dot and the host in the titlebar. `tern <host>` connects at start.
- `tern`: login questions (unknown host key, password, key passphrase, keyboard-interactive) are asked inside the terminal with OpenSSH's wording; Backspace edits, Ctrl-C cancels.
- `tern`: session tabs in the titlebar with status dots; clicking a host with a live tab switches to it. ⌘W closes, ⌘1–9 and ⌘⇧[ / ⌘⇧] or ⌃Tab switch, middle-click closes.
- `tern`: a closed tab says "Press Enter to reconnect"; Enter dials the same host again in the same terminal, keeping its scrollback. Clicking a host whose tab is closed reconnects that tab instead of opening another.
- `tern`: ⌘K host picker: fuzzy search (nucleo, as zeron and helix) over host names and addresses; ↑/↓ or ⌃N/⌃P move, Enter connects, Esc or a click outside closes. Card, header, footer and list height are zeron's command palette values.
- `tern`: shortcuts work before the first tab is open and after the last one closes; the window itself now holds focus when no terminal does.
- `tern`: settings in `~/Library/Application Support/tern/settings.json` (or `$TERN_CONFIG_DIR`), written atomically; a corrupt file falls back to defaults.
- `tern`: ⌘B collapses and expands the sidebar over 200 ms; drag its edge to resize between 224 and 400 pt, double-click the edge to reset to 256. Width and collapsed state persist.
- `tern`: ⌘= / ⌘− / ⌘0 change the terminal font size for every tab (8–32 pt, default 13, as zeron); the grid and the remote PTY follow, and the size persists.
- `tern`: a connecting host's status dot breathes (zeron's 2.4 s pulse), synced between its tab and sidebar row; `"reduceMotion": true` in settings holds it still.
- Docs: idle memory baseline in `docs/perf.md`: 49 MB footprint (87 MB RSS) with one connected tab, about 5 MB over an empty window.
- `tern-vault`: one passphrase-encrypted file for SSH passwords (per `user@host:port`) and key passphrases (per key path), in the age format with an scrypt recipient. A wrong passphrase and a tampered file are both refused, saves are atomic, and plaintext is zeroed after use. Not wired into the app yet.
- `tern`: saved logins. After a password or key passphrase is typed and the login works, tern offers to keep it in the vault, creating one with a repeated passphrase if there is none. Later logins answer from the vault (unlocked once per run, Enter skips), try each saved secret once, and fall back to asking. Unlock and save run off the UI thread.
- `tern-ssh`: `password_server` example, a 127.0.0.1-only server that accepts one password, for checking login flows.
- Docs: README lists what the app does today and what is not done yet.
- `tern`: add, edit and remove connections in the app (⌘N or + in the sidebar). They are kept in `hosts.json`, never in `~/.ssh/config`, and a password typed in the form goes to the vault (created on first use). `~/.ssh/config` hosts stay listed, read-only, with Duplicate to edit; a connection with the same name takes their place.
- `tern`: text fields (adapted from gpui's input example) with a masked mode that never copies its secret.
- `tern`: a crash is written to `~/Library/Logs/tern/panic.log` with a backtrace, even when tern was started from Finder or the Dock.
- `tern`: Settings (⌘, or tern → Settings…) in zeron's layout: Appearance (font size, reduce motion), Terminal (Option as Meta), Connections (list, edit, remove, new), Vault (status, lock now) and About. Escape or Back returns.
- `tern`: a macOS menu bar (Hide, Hide Others, Show All, Quit, Minimize, Zoom) with ⌘Q, ⌘H, ⌥⌘H and ⌘M.
- `tern-term`: `local_demo <command>` for visual checks; scripted runs never take keyboard focus.
- Project: craft gates (`clippy -D warnings`, file-size and attribution checks, `cargo deny`), CI, architecture diagram, UI spec and measured performance notes.

### Changed

- `tern-ssh`: keyboard-interactive rounds arrive as `Prompt::Challenge` with the server's name, instructions and each prompt's text and echo flag, so 2FA prompts such as "Verification code:" are shown as written (#9).
- `tern-ssh`: the known_hosts location is a `ConnectSpec::known_hosts` field instead of the process-wide `TERN_KNOWN_HOSTS` variable; the `connect` example still reads the variable.

### Fixed

- `tern-ssh`: a `~/.ssh/config` alias with both `HostName` and its own `ProxyCommand` now uses that proxy instead of dialling `HostName` directly; a proxy that dies during the handshake is reported with its exit status (#7).

### Security

- A changed host key is a hard stop with no override.
- Secrets are held as zeroizing `SecretString`s and never logged; session output is redacted from `Debug`.
- RUSTSEC-2023-0071 in `rsa` (via russh) is accepted for now; see [SECURITY.md](SECURITY.md) and #16.

[Unreleased]: https://github.com/your-moon/tern/commits/main
