# AGENTS.md

## Cursor Cloud specific instructions

`codebase_visualizer` is a single-crate Rust CLI (edition 2021). It scans a source tree and writes a self-contained interactive HTML graph. There are no runtime services, databases, or network ports.

- Build/lint/test/run commands are standard Cargo and are documented in `README.md`. Use `cargo build`, `cargo clippy`, `cargo test`, and `cargo run -- <path> --output <file>.html`.
- `cargo build` compiles native `tree-sitter` grammars via the `cc` crate, so a C compiler (`cc`/`gcc`) must be present. It already is in this environment.
- The generated HTML is a static file opened directly in a browser (`file://...`); there is no server to start. It embeds `vendor/3d-force-graph.min.js`.
- `cargo build`/`cargo clippy` currently emit harmless warnings (dead-code and clippy style lints); these are pre-existing and not build failures.
