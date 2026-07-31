# Overnight work log — 31 July 2026

Unattended pass over W11 → W6 → W7 → W9 → W10 → W8 on `feat/ast-system`
(branch `cursor/overnight-queue-5755`). Spec:
[`remaining-work.md`](remaining-work.md).

---

## W11 — Qualified calls through an imported path-dependency module — DONE

**Why.** The self-map's last three unresolved sites were all the same false
positive: `use horizon_engine::discover;` then `discover::normalize_path(...)`.
The import target is a real `pub mod` in a declared path dependency, but
`resolve_target_path` forced every cross-crate target through the "final
segment is a function" path, so the binding never became a module the
qualified-call navigator could enter.

**Change.** When a cross-crate import target is an all-`pub` module chain
(including the bare crate root), resolve it as `ForeignModule` instead of
"names a module, not a function". Qualified calls whose leading segment is
that binding (`mod::fn`, renamed `alias::fn`, renamed crate-root `eng::fn`)
continue inside the dependency under the existing visibility / dependency-gate
rules. Unit tests cover the three shapes; the `path-dependency` fixture gains
`via_imported_module` and the Phase 4 integration test asserts the edges.

**Measured.**

| Check | Result |
|---|---|
| `cargo test --workspace` | pass |
| Unit: `format::upper` / `fmt::upper` / `eng::version` | Resolved to the foreign ids |
| Integration: `path-dependency` `via_imported_module` | all three sites Resolved; fixture summary **0 unresolved** |
| Self-map regenerate | **0 conflicts · 0 unresolved**; dropped 143 external, 232 constructor, 219 associated, 26 local |

**W5 re-accounting.** Before: 3 unresolved, all W11 false positives. After: **0
unresolved**. Every previously unresolved self-map site is now either dropped
(local binding) or resolved. The map reports no genuinely missing free function
on this repository.

**Not verified yet.** Browser / UI matrix — deferred until a server is up for
W6+; analyser-only change, no web assets touched.

---

## Queue status

| Item | Status |
|---|---|
| W11 | done (this entry) |
| W6 | next |
| W7 | pending |
| W9 | pending |
| W10 | pending |
| W8 | pending / may stop cleanly |
