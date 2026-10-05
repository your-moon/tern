# Contributing to tern

Thanks for helping. This file is short on purpose; the rules that matter are enforced by CI.

## Before you start

- Look at the [plan issue](https://github.com/your-moon/tern/issues/1) and pick an issue labelled `triage::ready`. Comment that you are taking it.
- For anything not on the plan, open an issue first. Bugs need a runnable reproduction; features use the two-question template.
- A change that copies code from another project must keep a `// Adapted from <repo> <path> (<license>).` header and add the source to [NOTICE.md](NOTICE.md). The license must be compatible with GPL-3.0.

## Making a change

```sh
cargo fmt --all
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
scripts/check-craft.sh
```

All four must pass; CI runs the same commands plus `cargo deny`. [CRAFT.md](CRAFT.md) explains the rules behind them: no panics or `unsafe` in shipped code, no file over 800 lines, bounded memory, comments only where they say why.

- **Tests:** put unit tests beside the code. For risky logic (key encoding, host-key checks, parsing, buffering), break the code on purpose and confirm the test fails before you submit.
- **UI changes:** run the app or `local_demo`, take a screenshot, and attach it to the pull request with what you checked.
- **Memory:** if a change could affect memory, measure before and after with `scripts/measure-rss.sh` and record the result in [docs/perf.md](docs/perf.md).

## Pull requests

- One logical change per pull request, linked to its issue with `Closes #N`.
- Fill in the template: what changed, why, how you verified it (the command and its result), what to review first.
- Add a line under `## [Unreleased]` in [CHANGELOG.md](CHANGELOG.md) for anything a user would notice.
- Commit messages: a short summary line, a blank line, then why the change was needed.

## Code of conduct

This project follows the [Contributor Covenant](CODE_OF_CONDUCT.md).
