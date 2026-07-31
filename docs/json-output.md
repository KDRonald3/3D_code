# Horizon JSON output

**Status:** contract for consumers (CLI and library emit the same shape)
**Date:** 30 July 2026

This document describes the JSON written by `horizon` (and by
`horizon_map::write_map` / `horizon_map::map_to_string`, also re-exported from
`horizon_engine`). It is aimed at someone building a consumer who cannot read
the Rust source. The visual frontend is deferred; this file is the contract it
will be built against.

The Rust types live in [`crates/horizon-map/src/map.rs`](../crates/horizon-map/src/map.rs).
Serialization uses [`serde`](https://serde.rs/) with the attributes shown
there. Unless noted, every field is always present.

Output is **pretty-printed** by default (indented, trailing newline). Pass
`--compact` on the CLI for a single-line document. Field names and value shapes
are identical either way.

---

## Top level: `Repository`

One analysis run produces one object:

| Field | Type | Optional? | Meaning |
|---|---|---|---|
| `root` | string (absolute path) | no | Repository path that was analysed |
| `crates` | array of `Crate` | no | Every mapped compilation unit (library-like targets including `proc-macro`, plus binaries); may be empty. Examples, tests, benches, and `build.rs` are not discovered |
| `summary` | `MapSummary` | no | Aggregate counts for incomplete and deliberately dropped edges |

Paths in the document are absolute filesystem paths (platform-native separators
as produced by the host). On Windows they look like
`C:\\Users\\…\\src\\lib.rs`.

---

## `MapSummary`

Map health without walking the tree. **Resolved** call sites are not counted
here — only incomplete and excluded edges.

| Field | Type | Meaning |
|---|---|---|
| `conflicts` | number (usize) | Call sites whose target is `kind: "conflict"` |
| `unresolved` | number (usize) | Call sites whose target is `kind: "unresolved"` |
| `external_dropped` | number (usize) | Calls into `std` / `core` / registry deps / etc. that were **dropped** |
| `constructor_dropped` | number (usize) | Enum-variant / tuple-struct constructor forms that were **dropped** |
| `associated_dropped` | number (usize) | Associated functions on types (`Vec::new`, …) that were **dropped** |

### How to interpret the counters

- `conflicts` and `unresolved` count sites that **appear in the tree** under
  some `File` or `Function` `call_sites` array.
- `*_dropped` counts sites that were recognised and **omitted entirely**. They
  do **not** appear anywhere in `call_sites`. A consumer that tallies
  `call_sites.length` across the tree will never see them. That is deliberate:
  external crates, constructors, and `impl` items are outside the indexed
  universe, not “failed resolutions.”
- Do not treat a high `external_dropped` as a resolution bug. Do not treat a
  dropped constructor as `unresolved`.

---

## `Crate`

One compilation unit (one library-like target **or** one binary). Library-like
includes ordinary `lib` / `rlib` / `dylib` / `cdylib` / `staticlib` and
`proc-macro` crates (`is_library: true` for all of those). A package with both
a lib and a bin yields **two** crate objects. Folder trees are built
independently per root so lib and `src/bin/` files are never interleaved.

| Field | Type | Optional? | Meaning |
|---|---|---|---|
| `name` | string | no | Package name from the manifest (dashes preserved) |
| `rustc_name` | string | no | Real cargo/rustc target name (`-` → `_`). A lib and bin in the same package may share this string |
| `is_library` | boolean | no | `true` for the package library; `false` for a binary |
| `edition` | string | no | Edition from cargo metadata (`"2021"`, `"2024"`, …) |
| `roots` | array of string (paths) | no | Absolute `src_path` of this compilation root (one entry in practice) |
| `dependencies` | array of `Dependency` | no | Manifest dependencies |
| `folders` | array of `Folder` | no | Immediate child directories under the crate source layout |
| `files` | array of `File` | no | Files at the crate source root (e.g. `lib.rs`, `main.rs`) |

---

## `Dependency`

| Field | Type | Optional? | Meaning |
|---|---|---|---|
| `name` | string | no | Package name from cargo metadata (dashes) |
| `rename` | string | **yes** | Manifest alias when declared as `alias = { package = "real-name", … }`. This is the name used in `use alias::…` paths. Omitted when the package name is the import name |
| `kind` | `"path"` \| `"external"` | no | Path deps may be walked; external calls are dropped |
| `path` | string (absolute path) | **yes** | Present only when `kind` is `"path"`; omitted from JSON when absent |

Cross-crate resolution and the dependency gate use the **import** rustc name
(`rename` when present, else `name`, with `-` → `_`).

---

## `Folder`

| Field | Type | Optional? | Meaning |
|---|---|---|---|
| `path` | string (absolute path) | no | Directory on disk |
| `folders` | array of `Folder` | no | Nested directories |
| `files` | array of `File` | no | Source files in this directory that the `mod` walk found |

Only declaration-driven modules appear. Undeclared files (e.g. `orphan.rs`) are
absent.

---

## `File`

| Field | Type | Optional? | Meaning |
|---|---|---|---|
| `path` | string (absolute path) | no | Source file on disk |
| `module_path` | string | no | Module path of this file (`"crate"`, `"crate::shapes"`, …) |
| `content_hash` | string | no | Lowercase hex-encoded SHA-256 (64 hex digits, no algorithm prefix) of the **raw file bytes** as read from disk at extract time — no newline normalisation. On Windows a CRLF edit changes the digest. A consumer that slices source by `Function.byte_*` must re-hash the path and refuse to slice on mismatch. Algorithm is SHA-256 by this contract; switching later would be a wire-format bump |
| `functions` | array of `Function` | no | Free functions defined in this file |
| `call_sites` | array of `CallSite` | no | Calls **outside** any free function (e.g. `const` / `static` init). Empty when every path-form call sits inside a function. Never holds calls from `impl` / `trait` items |
| `doc_comments` | array of `DocComment` | no | Inner module docs (`//!`, `/*! … */`, `#![doc = "…"]`) |

---

## `Function`

| Field | Type | Optional? | Meaning |
|---|---|---|---|
| `id` | string (`FunctionId`) | no | Canonical identity — see below |
| `name` | string | no | Function name as written |
| `module_path` | string | no | Full path including name, with a leading `crate` segment (e.g. `"crate::shapes::get"`). Distinct from `id`, which substitutes the crate key for `crate` |
| `line` | number (u32) | no | 1-based line of the `fn` keyword |
| `byte_start` | number (u32) | no | UTF-8 byte offset of the start of this free-function item in the file |
| `byte_end` | number (u32) | no | UTF-8 byte offset one past the end of this free-function item |
| `call_sites` | array of `CallSite` | no | Outgoing call edges in **source order** |
| `doc_comments` | array of `DocComment` | no | Outer docs (`///`, `/** … */`, `#[doc = "…"]`) |

The `byte_start` / `byte_end` range is the full `ast::Fn` syntax node: outer
attributes (`#[cfg]`, `#[inline]`, …), outer doc comments, signature, and body.
It deliberately does **not** start at the `fn` keyword alone (that would hide
the attributes an auditor needs when comparing `#[cfg]`-duplicated definitions).
Doc comment text therefore appears both inside this range and separately in
`doc_comments`; that duplication is accepted.

Methods and `impl` items never appear as `Function` nodes.

---

## `CallSite`

| Field | Type | Optional? | Meaning |
|---|---|---|---|
| `call_path` | string | no | Path text exactly as written at the call site |
| `line` | number (u32) | no | 1-based line of the start of the call expression |
| `byte_start` | number (u32) | no | UTF-8 byte offset of the start of the call in the file |
| `byte_end` | number (u32) | no | UTF-8 byte offset one past the end of the call |
| `target` | `CallTarget` | no | How the edge completes |
| `from_macro` | boolean | **yes** | Present and `true` only when the site was recovered from a macro argument token tree. Omitted when false (ordinary `CallExpr` syntax). Treat `true` as lower-certainty provenance: recovery is allowlist-based and never guesses among candidates, but the call was not a real CST `CallExpr`. |

Columns are omitted: editors derive display columns from the open file and the
byte range.

---

## `CallTarget` (adjacent tagging)

Serialized with serde **adjacent tagging**: a `kind` field plus a `data`
payload. Discriminator values are snake_case.

| `kind` | `data` shape | Meaning |
|---|---|---|
| `"resolved"` | string — a bare `FunctionId` | Exactly one definition. `data` is the id of the canonical function node |
| `"conflict"` | object `{ "candidates": string[], "reason": string }` | Several candidate definitions; the tool refused to pick a winner |
| `"unresolved"` | object `{ "reason": string }` | No indexed free-function definition matches |

### Examples

```json
{ "kind": "resolved", "data": "glob_ambiguity::shapes::get" }
```

```json
{
  "kind": "conflict",
  "data": {
    "candidates": [
      "glob_ambiguity::shapes::get",
      "glob_ambiguity::text::get"
    ],
    "reason": "`get` is ambiguous because of multiple glob imports in `crate::app` (rustc E0659)"
  }
}
```

```json
{
  "kind": "unresolved",
  "data": {
    "reason": "no indexed definition matches `mystery`"
  }
}
```

### What each variant means (do not conflate them)

This distinction is the heart of the tool’s design.

- **`resolved`** — name lookup found exactly one free-function definition. The
  edge is a **reference** (by id) to that one canonical node, not a copy of it.
- **`conflict`** — name lookup found **two or more** plausible definitions and
  the language does not allow picking one (classic case: two glob imports
  supplying the same name → rustc `E0659`). This is a first-class map outcome,
  not an error channel. A frontend should show every candidate. Guessing a
  “likely” winner is forbidden.
- **`unresolved`** — this is a free-function-shaped call, but **no** indexed
  definition matches. There are not several alternatives; there is no callee.
  Typical causes: a typo, a name that only exists behind a macro, or a path
  into a workspace sibling that is not a declared dependency.

**Not** represented as `unresolved` (and absent from `call_sites`):

- Calls into external crates → counted in `external_dropped`
- Enum / tuple constructors (`Ok(x)`, `Some(v)`, …) → `constructor_dropped`
- Associated functions (`Vec::new`, `Type::assoc`) → `associated_dropped`
- Method calls and anything inside `impl` / `trait` bodies → never extracted

---

## `Conflict` / `UnresolvedCall` payloads

When `kind` is `"conflict"`, `data` is:

| Field | Type | Meaning |
|---|---|---|
| `candidates` | array of `FunctionId` strings | Every candidate definition; always non-empty |
| `reason` | string | Human-readable explanation |

When `kind` is `"unresolved"`, `data` is:

| Field | Type | Meaning |
|---|---|---|
| `reason` | string | Human-readable explanation |

Candidates are function ids, never file paths: `#[cfg]`-duplicated definitions
in the same file must remain distinct.

---

## `DocComment`

| Field | Type | Optional? | Meaning |
|---|---|---|---|
| `kind` | `"outer"` \| `"inner"` | no | Outer docs follow an item; inner docs document the enclosing module/file |
| `text` | string | no | Body with markers stripped; consecutive pieces on one owner joined with `\n` |

Ordinary `//` / `/* */` comments never appear.

---

## `FunctionId` format

A string that uniquely names one free-function definition in the repository map:

```text
{crate_key}::{module_path}
{crate_key}::{module_path}#L{line}   // only when disambiguating
```

| Piece | Rule |
|---|---|
| `crate_key` | For a library: `rustc_name`. For a binary: `{rustc_name}[bin]` |
| `module_path` | Module segments plus the function name, as seen from outside the crate. The internal leading `crate` segment is **replaced** by `crate_key` (not prepended), so the id does not name the crate twice |
| `#L{line}` | Appended **only** when two or more definitions share the same crate-key-plus-module path (typically `#[cfg]` duplicates). Every colliding definition gets the suffix; unique paths stay free of line numbers |

Examples:

| Situation | Id |
|---|---|
| `fn get` in `shapes` of package `glob-ambiguity` | `glob_ambiguity::shapes::get` |
| Binary `main` in package `horizon` | `horizon[bin]::main` |
| Cfg-colliding `open` at lines 10 and 14 | `fs_utils::open#L10` / `fs_utils::open#L14` |

`[` / `]` are illegal in Rust identifiers and Cargo package names, so
`name[bin]` cannot be mistaken for a real crate.

### Using an id as a lookup key

1. Walk `repository.crates` → each crate’s `files` and nested `folders[].files`.
2. For each file, scan `functions` until `function.id === targetId`.
3. That node is the canonical definition. Every `resolved` (and every conflict
   candidate) that names the same string refers to **this same node**.

Ids are references, not embedded copies. Recursion is a pointer back at the
caller: if `countdown` calls `countdown`, the call site’s
`target.data` equals the enclosing function’s `id`. Expanding resolved targets
by value would loop forever; follow ids instead.

Because every id is prefixed by a compilation-unit key, definitions in
different crates (including a package’s lib and bin) cannot collide inside one
repository map.

---

## Containment tree (summary)

```text
Repository
└── crates[]
    ├── folders[] / files[]
    │     └── folders[] / files[]   (nested)
    └── File
        ├── content_hash            (SHA-256 hex of raw bytes)
        ├── call_sites[]            (module-level)
        ├── doc_comments[]          (inner)
        └── functions[]
            ├── byte_start / byte_end  (full ast::Fn node)
            ├── call_sites[] → CallTarget
            └── doc_comments[]      (outer)
```

There are no `struct` / `enum` / `trait` / module nodes in the emitted map.
Inline modules contribute path segments on `Function.module_path` / `File.module_path`
but do not appear as separate containers.

---

## Worked example: `tests/fixtures/glob-ambiguity/`

This fixture is a minimal crate that triggers rustc `E0659`: two sibling
modules each define `get`, both are glob-imported in `app`, and `get(3)` is
called with no explicit import. Horizon must emit a `conflict` naming both
candidates — never pick a winner.

Generated with (from the Horizon repository root):

```powershell
cargo run --quiet -p horizon -- tests/fixtures/glob-ambiguity
```

Absolute paths below reflect the machine that produced this capture. Field
shapes and the conflict payload are what matter for the contract.

```json
{
  "root": "C:\\Users\\kouat\\code\\Horizon\\tests\\fixtures\\glob-ambiguity",
  "crates": [
    {
      "name": "glob-ambiguity",
      "rustc_name": "glob_ambiguity",
      "is_library": true,
      "edition": "2021",
      "roots": [
        "C:\\Users\\kouat\\code\\Horizon\\tests\\fixtures\\glob-ambiguity\\src\\lib.rs"
      ],
      "dependencies": [],
      "folders": [],
      "files": [
        {
          "path": "C:\\Users\\kouat\\code\\Horizon\\tests\\fixtures\\glob-ambiguity\\src\\app.rs",
          "module_path": "crate::app",
          "content_hash": "ff887c08cd8e3b9e6774779930c60c2dd9c84a14e129a39676c0fb4c9f7ac9fd",
          "functions": [
            {
              "id": "glob_ambiguity::app::run",
              "name": "run",
              "module_path": "crate::app::run",
              "line": 6,
              "byte_start": 127,
              "byte_end": 163,
              "call_sites": [
                {
                  "call_path": "get",
                  "line": 7,
                  "byte_start": 155,
                  "byte_end": 161,
                  "target": {
                    "kind": "conflict",
                    "data": {
                      "candidates": [
                        "glob_ambiguity::shapes::get",
                        "glob_ambiguity::text::get"
                      ],
                      "reason": "`get` is ambiguous because of multiple glob imports in `crate::app` (rustc E0659)"
                    }
                  }
                }
              ],
              "doc_comments": []
            }
          ],
          "call_sites": [],
          "doc_comments": [
            {
              "kind": "inner",
              "text": "Both globs, no explicit import — rustc rejects `get` as ambiguous (`E0659`)."
            }
          ]
        },
        {
          "path": "C:\\Users\\kouat\\code\\Horizon\\tests\\fixtures\\glob-ambiguity\\src\\lib.rs",
          "module_path": "crate",
          "content_hash": "9409e87d6569589767475078c65263faf4902488614e1e78edfecea9ed71e006",
          "functions": [],
          "call_sites": [],
          "doc_comments": [
            {
              "kind": "inner",
              "text": "Minimal crate whose sole purpose is to trigger rustc `E0659`:\ntwo sibling modules each define `get`, both are glob-imported, and\n`get` is called unqualified."
            }
          ]
        },
        {
          "path": "C:\\Users\\kouat\\code\\Horizon\\tests\\fixtures\\glob-ambiguity\\src\\shapes.rs",
          "module_path": "crate::shapes",
          "content_hash": "67f6fe77d04c5f66f3302efe4b3481fc151cb5dd813f9c2f2f0199aecb97ba24",
          "functions": [
            {
              "id": "glob_ambiguity::shapes::get",
              "name": "get",
              "module_path": "crate::shapes::get",
              "line": 3,
              "byte_start": 52,
              "byte_end": 95,
              "call_sites": [],
              "doc_comments": []
            }
          ],
          "call_sites": [],
          "doc_comments": [
            {
              "kind": "inner",
              "text": "One of two glob-imported definitions of `get`."
            }
          ]
        },
        {
          "path": "C:\\Users\\kouat\\code\\Horizon\\tests\\fixtures\\glob-ambiguity\\src\\text.rs",
          "module_path": "crate::text",
          "content_hash": "78d1f9374d2a3bc5e8b6939e16cfc572cddff393124a5ef3964847adb80cd7cd",
          "functions": [
            {
              "id": "glob_ambiguity::text::get",
              "name": "get",
              "module_path": "crate::text::get",
              "line": 3,
              "byte_start": 50,
              "byte_end": 93,
              "call_sites": [],
              "doc_comments": []
            }
          ],
          "call_sites": [],
          "doc_comments": [
            {
              "kind": "inner",
              "text": "The other glob-imported definition of `get`."
            }
          ]
        }
      ]
    }
  ],
  "summary": {
    "conflicts": 1,
    "unresolved": 0,
    "external_dropped": 0,
    "constructor_dropped": 0,
    "associated_dropped": 0
  }
}
```

Reading guide for this document:

- One crate, four files at the source root (`folders` is empty).
- `glob_ambiguity::app::run` has a single call site; `target.kind` is
  `"conflict"` with both `shapes::get` and `text::get` as candidates.
- `summary.conflicts` is `1`, matching that site.
- Inner doc comments on each file appear under `File.doc_comments`.
- No dropped counters are non-zero: every extracted call stayed in the tree.

---

## Related documents

- Specification of record: [`docs/requirements/rust-function-map.md`](requirements/rust-function-map.md)
- Fixture catalogue (including this example’s source): [`tests/fixtures/README.md`](../tests/fixtures/README.md)
