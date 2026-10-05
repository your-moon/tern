# tern

A fast, low-memory SSH terminal for macOS, written in Rust on [GPUI](https://github.com/zed-industries/zed/tree/main/crates/gpui), the UI framework behind Zed.

tern does one thing: connect to the hosts in your `~/.ssh/config` and give each one a tab. It is built for people who want Tabby's convenience without an Electron app's memory bill.

> **Status: pre-alpha.** The SSH layer and the terminal renderer work and are tested; the app window that ties them together is being built ([plan](https://github.com/your-moon/tern/issues/1)). There is no release yet.

## What works today

- **SSH sessions** (`crates/tern-ssh`): ssh-agent, key files with passphrases, password and keyboard-interactive login; `~/.ssh/config` hosts and `ProxyCommand`; `known_hosts` checking where a changed host key is a hard stop.
- **Bounded memory under load**: a 16.9 MB burst of output peaks at 6 MB RSS, including when the reader stalls, because tern stops reading the socket instead of buffering ([numbers](docs/perf.md)).
- **Terminal rendering** (`crates/tern-term`): xterm-256color and truecolor, bold/dim/italic/underline/inverse/strike, wide CJK and emoji on the grid, box drawing painted as paths so lines meet, mouse reporting, selection and bracketed paste.

## Planned for the first release

Host sidebar, tabs, a zeron-style interface ([UI spec](docs/ui.md)), a ⌘K host picker, and a Tabby-style encrypted vault for saved credentials. Progress is tracked on the [plan issue](https://github.com/your-moon/tern/issues/1).

## Build

Requires Rust 1.99+ and macOS.

```sh
git clone https://github.com/your-moon/tern
cd tern
cargo test --workspace

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
