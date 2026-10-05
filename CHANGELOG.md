# Changelog

All notable changes to tern are recorded here. The format follows [Keep a Changelog 1.1.0](https://keepachangelog.com/en/1.1.0/), and versions follow [Semantic Versioning](https://semver.org/spec/v2.0.0.html). Until 1.0, a minor version may break things.

## [Unreleased]

### Added

- `tern-ssh`: SSH sessions over russh with ssh-agent, key-file (with passphrase), password and keyboard-interactive login; `~/.ssh/config` hosts and `ProxyCommand`; `known_hosts` checking where a changed key refuses the connection and an unknown key asks first.
- `tern-ssh`: bounded output path. Output is coalesced into 64 KiB chunks over an 8-deep queue, and the session stops reading the socket when the UI falls behind; a 16.9 MB burst peaks at 6 MB RSS. Input reports `Busy` instead of queueing without limit.
- `tern-term`: a GPUI terminal view fed by any byte stream, with xterm-256color and truecolor, text attributes, wide characters, box drawing painted as paths, mouse reporting, selection and bracketed paste.
- `tern-ssh`: password, keyboard-interactive and encrypted-key logins are tested against an in-process russh server, including retries after a wrong answer and cancelling.
- `tern`: the app window, with zeron's chrome: transparent 38 pt titlebar, traffic lights at (14, 14), frosted shell over a blurred background, a 10 pt rounded main panel, bundled Geist fonts and structured JSON logs (`TERN_LOG`). Idle: 66 MB.
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
