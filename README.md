# tern

A fast, low-memory SSH terminal for macOS, written in Rust on [GPUI](https://github.com/zed-industries/zed/tree/main/crates/gpui), the UI framework behind Zed.

tern does one thing: connect to the hosts in your `~/.ssh/config` and give each one a tab. It is built for people who want Tabby's convenience without an Electron app's memory bill.

> **Status: pre-alpha.** The app runs and connects; there is no packaged release yet. Progress is tracked on the [plan issue](https://github.com/your-moon/tern/issues/1).

## What works today

- **The app** (`crates/tern`): hosts from `~/.ssh/config` in a sidebar; click one, or run `tern <host>`, and its session opens in a tab.
  - Tabs: ⌘1–9, ⌘⇧[ / ⌘⇧], ⌘W. A closed tab reconnects on Enter.
  - ⌘K opens a fuzzy host picker.
  - ⌘B collapses the sidebar; drag its edge to resize it.
  - ⌘= / ⌘− / ⌘0 change the font size.
  - Settings persist in `~/Library/Application Support/tern/settings.json`.
- **Saved logins** (`crates/tern-vault`): after a password or key passphrase works, tern offers to keep it in one encrypted file (age, scrypt), unlocked with a passphrase once per run.
- **SSH sessions** (`crates/tern-ssh`): ssh-agent, key files with passphrases, password and keyboard-interactive login; `~/.ssh/config` hosts and `ProxyCommand`; `known_hosts` checking where a changed host key is a hard stop. Login questions are asked inside the terminal, worded as OpenSSH words them.
- **Small footprint**: 49 MB with one connected tab, idle. A 16.9 MB burst of output peaks at 6 MB in the SSH layer, because tern stops reading the socket instead of buffering ([numbers](docs/perf.md)).
- **Terminal rendering** (`crates/tern-term`): xterm-256color and truecolor, bold/dim/italic/underline/inverse/strike, wide CJK and emoji on the grid, box drawing painted as paths so lines meet, mouse reporting, selection and bracketed paste.

## Not yet

Rebindable shortcuts, macOS Keychain unlock for the vault, following macOS Reduce motion ([#18](https://github.com/your-moon/tern/issues/18)), a light theme, and a signed release.

## Build

Requires Rust 1.99+ and macOS.

```sh
git clone https://github.com/your-moon/tern
cd tern
cargo test --workspace

# Run the app (optionally straight to a host):
cargo run --release -p tern -- my-host

# Try the SSH layer from your terminal:
cargo run -p tern-ssh --example connect -- user@host

# Try the renderer with a local shell:
cargo run --release -p tern-term --example local_demo
```

## Design

- [Architecture](docs/architecture.html): crates, data flow, threads, memory.
- [UI spec](docs/ui.md): every size and timing, with its source.
- [Performance](docs/perf.md): measured memory and paint cost.
- [Craft rules](CRAFT.md): the gates every change passes.

## Built on

tern copies and adapts code from well-maintained projects rather than reinventing it: Zed's terminal (GPL-3.0), zeron's UI (MIT), russh (Apache-2.0), alacritty_terminal (Apache-2.0) and others. Each copied file names its origin; the full list is in [NOTICE.md](NOTICE.md).

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md). Report security problems privately, as described in [SECURITY.md](SECURITY.md).

## License

[GPL-3.0-or-later](LICENSE).
