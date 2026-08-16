# Contributing to Animus Live

**Do not read the original Animata C++ source.** Animus Live is a clean-room reimplementation. Reading the original's `src/` directory, or pasting it into an AI coding assistant, would compromise that. The original's README, documentation, published papers, screenshots and videos are fine and are the intended reference material.

Note that a checkout of the original may physically sit next to this one on a development machine (it has been at `C:\devnimata`). Directory listings and commit subject lines are not source and have been treated as harmless; the `src/` tree is off-limits regardless of how convenient it is. The reason is concrete rather than ceremonial: the original is GPLv3, Animus Live is `MIT OR Apache-2.0`, and having read the source makes any later resemblance impossible to defend as independent work.

The easiest place to start contributing is `crates/animus-core` and `crates/animus-project`. They are plain Rust with no engine dependency — you can build and test them on any machine with `cargo test -p animus-core -p animus-project`, no GPU and no Bevy knowledge required.

## Development

- Toolchain is pinned in `rust-toolchain.toml`; `rustup` will pick it up automatically.
- Run `cargo fmt --all` and `cargo clippy -p animus-core -p animus-project --all-targets -- -D warnings` before opening a pull request.
- `cargo test -p animus-core -p animus-project` must pass without a GPU.
- License and dependency policy is enforced by `cargo deny` (see `deny.toml`); GPL/LGPL dependencies are not permitted.
