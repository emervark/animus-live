# Contributing to Animus Live

**Do not read the original Animata C++ source.** Animus Live is a clean-room reimplementation. Reading the original's `src/` directory, or pasting it into an AI coding assistant, would compromise that. The original's README, documentation, published papers, screenshots and videos are fine and are the intended reference material.

The easiest place to start contributing is `crates/animus-core` and `crates/animus-project`. They are plain Rust with no engine dependency — you can build and test them on any machine with `cargo test -p animus-core -p animus-project`, no GPU and no Bevy knowledge required.

## Development

- Toolchain is pinned in `rust-toolchain.toml`; `rustup` will pick it up automatically.
- Run `cargo fmt --all` and `cargo clippy -p animus-core -p animus-project --all-targets -- -D warnings` before opening a pull request.
- `cargo test -p animus-core -p animus-project` must pass without a GPU.
- License and dependency policy is enforced by `cargo deny` (see `deny.toml`); GPL/LGPL dependencies are not permitted.
