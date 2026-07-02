# Codebase Visualizer

An interactive, Figma-style map of a codebase. Add a folder — or a single
source file — and a Rust analyzer scans the source, classifies each file,
infers dependencies, extracts the functions and data structures inside each
file, and renders it all into a single-page app for 10× faster understanding
of unfamiliar (or AI-written) code.

The app **opens empty** — nothing is shown until you add a project folder or
source file.

## Architecture

- **`src/lib.rs` + `src/lang.rs`** — the analyzer. Walks a set of source files
  and produces the data model the UI consumes: file nodes, dependency edges,
  per-file summaries / source snippets / risks, folder grouping, per-file
  function & data-structure graphs, and cross-file call / composition edges.
  Heuristic and line/brace based, supporting Rust, Python, JavaScript,
  TypeScript, Go, Java and C/C++.
- **`src/bin/server.rs`** — a small local web server (`tiny_http`). It serves
  the UI and exposes `POST /api/scan`, which runs the analyzer over the files
  the browser uploads when you add a folder.
- **`web/index.dc.html` + `web/support.js`** — the front-end (a Design-Component
  React runtime). The design is unchanged; it is simply driven by the live model
  returned by the analyzer instead of static sample data.

## Run the app

```sh
cargo run --bin server
```

Then open <http://localhost:8787> and pick a project directory with
**Open local folder**, or one or more standalone files with
**Open source file(s)** — you can also drag source files straight onto the
page. (Set `PORT` to change the port. The server listens on `127.0.0.1` only;
set `HOST` to expose it, e.g. `HOST=0.0.0.0`.)

## CLI

Inspect the analysis model for a directory — or a single source file —
without the browser:

```sh
cargo run --bin codebase_visualizer -- path/to/project --pretty
cargo run --bin codebase_visualizer -- path/to/file.py --pretty
```

Options:

- `-o, --output FILE` — write the model JSON to a file (default: stdout).
- `--pretty` — pretty-print the JSON.
- `--max-file-bytes BYTES` — skip files larger than this (default: `500000`).

Supported source extensions: Rust, Python, JavaScript, TypeScript, Go, Java,
C and C++.
