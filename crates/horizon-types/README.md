# horizon-types

Type definitions and inherent-method analysis for Horizon — **outside** the
free-function map algorithm.

> **Parked.** This crate is listed under `exclude` in the root manifest, so it
> takes no part in building, testing, or running the project. `cargo
> build`/`cargo test --workspace` never compile it, and no shipping crate
> depends on it. It exists so the analysis is not lost before the type/`impl`
> side gets its own design pass.
>
> It reaches its conclusions by carrying its own copy of the free-function
> resolve calculus (`resolve.rs` here differs from the engine's by roughly 330
> lines out of 2,875; `extract.rs` by roughly 940 out of 1,710). Keeping that
> copy inside the build would mean every future change to import resolution,
> glob handling, re-export hops, or visibility had to be made twice or the two
> maps would silently disagree. Sharing the calculus is the problem to solve
> before this is wired back in.
>
> Build or test it on demand:
>
> ```
> cargo test --manifest-path crates/horizon-types/Cargo.toml
> ```

The free-function pipeline (`horizon-engine` / `horizon-map`) deliberately
stops at free functions. Calls of the form `Type::assoc(...)` and
`receiver.method(...)` are dropped as `associated_dropped`. This crate owns
the work that goes past that boundary: emitting type nodes, indexing inherent
`impl Type` methods, and resolving method calls when a **certain** one-hop
receiver type is available.

## Public API

| Item | Role |
|------|------|
| `build_type_map(repo_root)` | Entry point: discover → walk → extract → resolve → `Repository` |
| `Repository` / `File` / `Function` | Enriched map: `File.types`, `Function.receiver_type` |
| `TypeItem`, `TypeId`, `TypeKind`, `TypeRef`, `TypeTarget`, … | Type-contract nodes |
| `extract`, `resolve`, `pipeline` | Internals reused by tests |

Discovery, module walking, and parsing come from `horizon-engine`
(`discover`, `modules`, `parse`). This crate does **not** reimplement the
`mod` walk.

## What it extracts

From each walked file:

- **Type definitions** — `struct`, `enum`, `trait`, `type` alias (not
  associated types inside `impl` / `trait` bodies). Includes doc comments,
  byte ranges, enum variant names, and named struct fields.
- **Field / alias type paths** — as pending `type_refs`, later resolved to
  `TypeTarget` (resolved / conflict / unresolved). Primitives and obvious
  prelude / external roots are omitted rather than inventing edges.
- **Inherent methods** — `impl Type { fn … }` (no trait). Emitted as
  `Function` nodes with `receiver_type` set. Trait impls and trait items stay
  out.
- **Method-shaped call sites** — `Type::method(...)` and
  `receiver.method(...)`, with a one-hop receiver hint when one is certain.

## Field / alias resolution

Type paths written on fields and alias right-hand sides are absolutized
relative to the defining module (same `crate` / `self` / `super` rules as
imports), then looked up in the crate’s type index (and imports). Ambiguous
candidates become `TypeConflict`; unknown local-looking names become
`UnresolvedType`. External / primitive paths are dropped from `type_refs`
entirely — same honesty rule as external call drops.

## Inherent methods

An `impl Type` (no `for Trait`) contributes each `fn` as a `Function` whose
`module_path` looks like `crate::Type::method` and whose `receiver_type` is
the `TypeId` of `Type`. Free functions keep `receiver_type: None`.

## One-hop receiver hints (certain only)

Method resolution never guesses. A receiver is typed only when one of these
shapes is present:

| Hint | Why it is certain |
|------|-------------------|
| Parameter annotation `x: Type` | The binding’s type is written in source |
| `let x: Type = …` | Same — explicit type ascription |
| `let x = Type::…` / `Type(...)` constructor on the RHS | The constructed type is the path’s head |
| Bare `self` / `Self::` inside `impl Type` | The impl’s self type is the receiver |
| `self.field` when `field` has a declared type on that inherent type | The field’s type is written on the struct |
| `let Some(x) = local_fn()` (or `Ok`) when `local_fn`’s return type peels to `Option<T>` / `Result<T, _>` | The binding’s type is the peeled `T` from a declared signature |

Anything less certain — including multi-hop chains like `self.a.b.method()`,
untyped locals, and trait-impl bodies — yields no receiver type.

## Unknown receiver

When the receiver type is unknown, the call is counted as
`associated_dropped`. Same-named inherent methods on *other* types are **not**
turned into a `Conflict`; that would invent a clash the author never wrote.

When the receiver type *is* known but no inherent method matches (e.g. a
trait method like `.clone()` on `String`), the call is also
`associated_dropped`.

## Why this is not in the free-function algorithm

The free-function map’s headline invariant is: never guess, and never pretend
methods are free functions. Mixing type nodes and one-hop method resolution
into that pipeline couples two designs and inflates `associated_dropped` /
function counts in ways that obscure the free-function story. Keeping this
crate separate leaves the original algorithm intact for self-maps, glob
conflicts, and the review UI’s free-function contract, while still providing
a complete type/method analysis for callers that opt in via `build_type_map`.

## Fixtures

Integration tests live in this crate and point at workspace fixtures:

- `tests/fixtures/type-definitions`
- `tests/fixtures/inherent-methods`
