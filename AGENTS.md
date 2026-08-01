# AGENTS.md

## Cursor Cloud specific instructions

Horizon is a **pure Rust Cargo workspace** (a static-analysis tool that builds a
free-function call map of a Rust repo). There is no database, Docker, Node/JS
build step, `.env`, or background infrastructure. Standard build/run/test
commands live in `README.md`; only the non-obvious caveats are captured here.

### Toolchain gotcha (most important)

The workspace depends (transitively, e.g. `cpufeatures`) on crates that require
Cargo's `edition2024` feature, so it needs **Rust/Cargo ≥ 1.85**. The VM image
may ship an older default toolchain (seen: `1.83.0`), which fails to even parse
those dependency manifests. The default is set to `stable` (via
`rustup default stable`) and the startup update script re-asserts it. If a build
fails with `feature 'edition2024' is required`, run `rustup default stable`.

### Workspace layout

Five workspace members under `crates/`: `horizon-map`, `horizon-engine`,
`horizon` (the `horizon` CLI, the default member), `horizon-server` (the web
viewer), and `horizon-correctness` (correctness harness). The `horizon-types`
crate is **deliberately parked/excluded** from the workspace (see the comment in
root `Cargo.toml`) — do not add it to the build or expect it to compile/test.

### Running the services

- CLI: `cargo run -p horizon -- <path/to/rust/repo>` (JSON map to stdout; `-o
  file.json` to write, `--compact` for single line). The engine shells out to
  `cargo metadata --no-deps --offline` against the *analysed* repo, so a working
  `cargo` must be on `PATH`.
- Web viewer: `cargo run -p horizon-server -- --no-open [--map map.json]`. It
  binds `127.0.0.1` on an **OS-assigned ephemeral port that is printed to
  stdout** (not fixed) — read the printed `http://127.0.0.1:<port>/` URL rather
  than assuming a port. Always pass `--no-open` in a headless VM (otherwise it
  tries `xdg-open`). A host-guard middleware rejects non-loopback `Host`
  headers. Test fixtures under `tests/fixtures/*` are handy inputs for both the
  CLI and the server's "Analyse a local folder" flow (`POST /api/analyse`).

### Lint / test

- Test: `cargo test --workspace`.
- Clippy: `cargo clippy --workspace --all-targets` (passes; a couple of style
  warnings only).
- `cargo fmt --all -- --check` currently reports diffs in a few test files
  because the repo was last formatted with an older `rustfmt`; this is
  pre-existing formatting drift, not a real failure — do not auto-reformat
  unrelated files while working on something else.
