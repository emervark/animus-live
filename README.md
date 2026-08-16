# Animus Live

Animus Live is an independent, clean-room reimplementation inspired by [Animata](http://animata.kibu.hu/) (Kitchen Budapest, 2007). It is not affiliated with the original project and contains no code derived from it.

Animus Live is a real-time puppet animation tool for live performance.

## Workspace layout

- `crates/animus-core` — document model, geometry and physics solver. No engine dependency.
- `crates/animus-project` — on-disk project format: JSON document, content-addressed assets, migrations.

See `docs/heritage.md` for the project's relationship to the original Animata, and `CONTRIBUTING.md` before opening a pull request.

## Building

```bash
cargo build
cargo test -p animus-core -p animus-project
```

## License

Licensed under either of

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE))
- MIT license ([LICENSE-MIT](LICENSE-MIT))

at your option.
