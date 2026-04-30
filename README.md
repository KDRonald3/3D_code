# Codebase Visualizer

Generate a self-contained HTML map of a source tree so you can review unfamiliar AI-written code more safely.

The Rust CLI scans common source files, extracts files, functions, and data structures, pulls nearby documentation comments into summaries, infers simple symbol mentions, and writes a searchable visual graph.

## Usage

```sh
cargo run -- path/to/codebase --output codebase-map.html
```

Then open `codebase-map.html` in a browser.

Supported source extensions: Rust, Python, JavaScript, TypeScript, Go, Java, C, and C++.

Useful options:

- `--output FILE` or `-o FILE`: choose the HTML output path.
- `--max-file-bytes BYTES`: skip very large files. Defaults to `500000`.
