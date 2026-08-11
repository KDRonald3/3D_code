# Vertical — a call-chain slice through one function

**Status:** implemented and verified in the IDE
**Date:** 10 August 2026
**Surface:** Horizon Map webview, Functions dock
**Contract truth:** [`map.rs`](../../crates/horizon-map/src/map.rs)
**Related:** the multi-file selection and All / Linking / Bridges scopes that
landed on `cursor/windows-ide-build` (commits `23522ee`, `cb17e31`)

---

## Summary

The Functions dock draws every function of the selected file(s) and every edge
between them. On a real file that is unreadable: `resolve.rs` alone renders
**66 nodes and 167 edges**, which at the zoom needed to fit the dock is a green
thicket with no legible labels.

**Vertical** answers a narrower question about one function: *who reaches this,
and what does this reach?* Pick a function, switch the dock to Vertical, and it
shows the whole call chain through it — everything that transitively calls it,
everything it transitively calls, and **nothing else** — laid out so distance
from the centre is distance along the chain.

"Nothing else" is the sharp edge of the definition. A function that calls one of
my callees, but neither reaches me nor is reached by me, is *not* on my chain
and must not appear. That single exclusion is what separates this from the
existing hop control, which is undirected and drags in exactly those siblings.

On the repo's own map this takes a typical function's graph from 66 nodes to
**39**, and from 167 edges to **67**.

## Goals

- Make a single function's place in the call graph legible without zooming.
- Separate *up* from *down*. The existing hop control cannot: `neighborhood`
  ([function_dag.js:404](../../ide/contrib/horizon/browser/media/function_dag.js#L404))
  builds its adjacency undirected —

  ```js
  adj.get(e.from).push(e.to);
  adj.get(e.to).push(e.from);
  ```

  so "2 hops" sweeps in callers, callees, *and* siblings that merely share a
  callee, which is a large part of why depth-limiting has not rescued busy
  files.
- Exclude siblings. Everything shown must be on the chain — reachable from the
  function or able to reach it.
- Support walking a call chain: from a vertical view, re-centre on any node.
- Stay inside what the user is already looking at, so the view is predictable.

## Non-goals

- **Not a repo-wide reachability query.** Vertical never pulls in a function
  that was not already on the canvas. See the decision below.
- **No depth limit.** The chain runs as far as it runs. Vertical is defined by
  reachability, not by a hop count.
- **No change to what the analyser reports.** Conflict and unresolved stubs keep
  their current meaning and presentation.
- **Not a replacement for All.** It is an additional way of scoping the same
  dock graph, beside All / Linking / Bridges.

## The measurements this is designed against

Computed over the repository's own map (`horizon-map.json`, 440 functions across
30 files) and over the `resolve.rs` canvas that motivated the feature.

**On the `resolve.rs` canvas** — 66 nodes, 167 edges, the view in the report:

| | cone |
|---|---|
| median | **39 nodes, 67 edges** |
| p75 | 41 nodes |
| p90 | 46 nodes |
| largest | 56 nodes |
| cone ≥ 80% of the canvas | 1 of 66 functions |
| cone ≤ 25% of the canvas | 9 of 66 functions |

**Repo-wide**, over all 440 functions: median 23, p75 41, p90 53, max 143.

Read that honestly: on a densely self-calling file the cone is **most of the
file**. `resolve_call` keeps 56 of 66 nodes, because nearly everything in
`resolve.rs` is genuinely upstream or downstream of it. Vertical is not a way
to make a hot function small — it is a way to make what is shown *mean*
something, so nothing on screen is there by accident.

What actually improves is **density**, and that is where legibility comes from:

| | edges per node |
|---|---|
| whole canvas | 2.5 |
| median cone | **1.7** |

And the chain is shallow enough to lay out vertically without endless scrolling:

| | rows (levels up + down + 1) |
|---|---|
| median | **7** |
| deepest | 10 |

Even `resolve_call`, the worst case, is 1 row up and 5 down. The cone is also a
DAG in practice — across the `resolve.rs` canvas no function is both an
ancestor and a descendant of the same centre — so layered placement works
without cycle-breaking.

## Behaviour

### What it shows

Given one selected function `f`, the view is exactly:

```
ancestors(f) ∪ { f } ∪ descendants(f)
```

- **ancestors** — every function that calls `f`, every function that calls one
  of those, and so on to the top of the chain.
- **descendants** — every function `f` calls, everything those call, and so on
  to the leaves.

Nothing else. Specifically **not** a function that calls something `f` calls but
that `f` neither reaches nor is reached by — that is a sibling, not part of the
chain, and it is the main thing this view exists to remove.

Edges between the kept nodes are drawn as they are today, with their existing
`resolved` / `conflict` / `unresolved` kinds.

A node reachable both upward and downward (possible through mutual recursion)
belongs to the chain once. It is placed on the side it was first reached from,
and never drawn twice.

### The universe is the canvas as it already stood

Vertical filters the graph the dock had already built for the current file
selection. It never expands beyond it. Two consequences, both accepted:

- A caller that exists in a file you have not selected does **not** appear. To
  see it, widen the selection (shift-click the other file) and the canvas — and
  therefore the vertical slice — widens with it.
- A callee from another file sits on the canvas with **no outgoing edges of its
  own**, because [`buildMany`](../../ide/contrib/horizon/browser/media/function_dag.js#L70)
  only ever walks the *seeds'* call sites. The ring below such a node is
  therefore empty — not because nothing is there, but because the canvas never
  had it. `resolve.rs` dodges this entirely (all 66 of its canvas nodes are
  seeds); other files will not.

### Stubs

Conflict and unresolved stubs appear exactly as they do now, hanging off
whichever function in the cone raised them, scoped to the current file. A stub
is an analyser outcome attached to a call site, not a function, so it has no
callers of its own and never contributes a second ring.

### Layout

Literally vertical, centred on the chosen function:

Left to right, like every other scope in the dock. "Vertical" names the slice
through the call graph, not the direction it is drawn:

```
  roots …  callers of   callers   ┌───────────────┐   callees   callees of   … leaves
           callers                │ this function │             callees
                                  └───────────────┘
```

One column per level, running as far as the chain goes — a median of 7 columns
and at most 10 on the `resolve.rs` canvas. Horizontal distance from the centre
means distance along the call chain. Each column is centred vertically on the
tallest, so the chain reads as a spine, and columns are sized by what they hold
rather than splitting the dock evenly.

### Ordering within a column: call order, not definition order

**Downstream, a column is ordered by where the parent calls each child** — the
source position of the call site, so reading a column top to bottom is reading
the parent's body top to bottom. This is what makes a 39-node cone
comprehensible: the view stops being a graph to decode and becomes the code's
own sequence.

The data is already there. Every call site in the map carries `byte_start`
([map.rs:339](../../crates/horizon-map/src/map.rs#L339)) — 1116 of 1116 in the
repo's own map — and `build` already copies it onto each edge as `byteStart`
([function_dag.js:55](../../ide/contrib/horizon/browser/media/function_dag.js#L55)).
No engine or schema change is needed.

This is a real change, not a formality. Today's layout orders by definition
line ([layout](../../ide/contrib/horizon/browser/media/function_dag.js#L234)),
and across the repo's map:

| | |
|---|---|
| parents with 2+ distinct callees | 184 |
| whose call order differs from definition order | **130 (71%)** |
| pairwise inversions between the two orders | 645/1266 (**51%**) |

At 51% inversions, definition order carries no information about call order.
`main` shows what the difference buys:

```
call order:  build_function_map → write_map_to_file → write_map_compact_to_file
             → write_map → write_map_compact → write_summary_line
def  order:  write_map, write_map_compact, write_map_to_file,
             write_map_compact_to_file, build_function_map, write_summary_line
```

Details that follow from this:

- A callee called several times takes the position of its **earliest** call
  site in that parent.
- A callee reached from several parents in the row above is ordered by the
  parent that placed it; where that is ambiguous, by the earliest call site
  among them.
- **Upstream there is no call order to honour.** Two different callers of `f`
  call it at positions in their own bodies, which are not comparable with each
  other. The ancestor columns are therefore ordered by the barycentre of the
  neighbours they connect to in the next column toward the centre, which is
  what actually reduces crossings, with definition order as a stable tiebreak —
  ordering by call site there would be arbitrary precision, not meaning.

This is a different layout from the current left-to-right layered placement in
[`layout`](../../ide/contrib/horizon/browser/media/function_dag.js#L234), not
the same one re-filtered.

### Reading the slice, and changing it

The slice is a large file cut down to one reviewable piece, and reviewing it
means clicking through its functions. **A click inside the chain therefore
selects and nothing more** — the Inspector follows, the tree does not move. A
click that re-centred would dissolve the slice the moment you started using it,
which is the opposite of what it is for.

Changing the subject is a separate, deliberate act, available three ways:

| Gesture | Effect |
|---|---|
| **⌖** on a node (appears on hover) | centre the chain on that function |
| double-click a node | the same, for anyone not aiming at a 16px target |
| pick a function outside the chain — Functions list, Layers, a jump | a new subject; the chain follows |

The centre is its own state, not `selectedFnId`. It survives every click within
the tree and is dropped when the canvas it belongs to goes away: a centre in
one of the selected files stays valid while that file is selected, and a centre
reached as a callee from another file stays valid only for the exact file
selection it was chosen on. Once it is neither, Vertical has nothing to draw
and falls back to All rather than showing a dead view.

## Affected code

| Where | What changes |
|---|---|
| [`function_dag.js`](../../ide/contrib/horizon/browser/media/function_dag.js) | New directed slice beside `relate` (L552) and `neighborhood` (L404); a vertical variant of `layout` (L234) |
| [`viewer.js` `renderFunctionDag`](../../ide/contrib/horizon/browser/media/viewer.js#L3739) | Dispatch to the new scope; banner text; empty state |
| [`viewer.js` `setFnsScope`](../../ide/contrib/horizon/browser/media/viewer.js#L3731) / [`syncFnsScopeControls`](../../ide/contrib/horizon/browser/media/viewer.js#L3707) | A fourth scope; unlike the others it needs a *function*, not a multi-selection, so its visibility rule differs |
| [`index.html`](../../ide/contrib/horizon/browser/media/index.html#L206) | The `fns-scope-mode` control gains a Vertical button |
| [`viewer.css`](../../ide/contrib/horizon/browser/media/viewer.css) | Ring styling; vertical layout rules |

The reverse index ("who calls me") does not exist anywhere today — the map
records calls only outward, as `call_sites` per function
([map.rs:339](../../crates/horizon-map/src/map.rs#L339)), and `fnIndex` maps a
FunctionId to its definition. Restricted to the canvas, callers can be read
straight off the built graph's edges, so no new index is needed.

## Edge cases

- **Hot functions keep most of the file.** `resolve_call` keeps 56 of 66. That
  is the honest answer to "what is on this chain", not a failure of the filter,
  but it means Vertical does not by itself make every function readable.
  Test functions are the mirror image: `excludes_local_associated_function` has
  0 ancestors and 38 descendants, because a test is a root of the chain.
- **Trivial cones.** `strip_segment_generics` has no callers and no callees on
  the canvas: cone of 1. Nine of 66 functions come out at a quarter of the
  canvas or less.
- **Recursion and cycles.** `classify_non_module_suffix`, `resolve_target_path`
  and `count_folder` call themselves; `resolve_cross_crate_path` participates in
  mutual recursion. Closure must be visited-guarded or it will not terminate.
  A node must never appear twice — first side reached wins — and the centre must
  never reappear in its own chain.
- **The chosen function has no callers and no callees.** Show it alone, with a
  sentence saying so, in the manner the relational scopes now use for an empty
  result.
- **Scope with a multi-selection.** Vertical is about one function. With several
  functions selected the primary is the natural centre; behaviour needs deciding
  during implementation.

## Decisions

| Decision | Why |
|---|---|
| Full transitive closure, no depth limit | The question is "what is on this call chain", and a chain does not stop at an arbitrary hop count. Depth 2 would give a far smaller view (median 10 nodes) but would answer a different, weaker question. |
| Siblings excluded | A function that calls what I call, without reaching me, tells me nothing about my chain. Excluding it is the whole point; the existing undirected hop control cannot. |
| Downward rows ordered by call site, not definition line | Comprehension comes from sequence, not from node count. Measured: 71% of parents call their callees in an order different from the order they are defined, and 51% of pairs are inverted, so definition order is close to random with respect to the code's actual flow. Reading a row left to right should be reading the parent's body top to bottom. |
| Upward rows ordered for crossing minimisation | Call-site positions in two different callers are not comparable, so there is no call order to preserve going up. Pretending otherwise would look principled and mean nothing. |
| The centre is its own state, and a click does not move it | The slice exists to be read, and reading it means clicking its functions. Tying the centre to `selectedFnId` made the tree replace itself on the first click. |
| Universe = the current canvas | Predictability — what you see is always a subset of what you were already looking at, and widening the file selection is the natural way to widen the answer. |
| No repo-wide caller index | Not needed once the universe is the canvas; callers are readable from the built graph's edges. |
| Stubs unchanged | They are call-site outcomes, not functions. Changing their presentation here would make the same data mean two things. |
| A fourth scope, not a new page | The dock already owns dense symbol work, and All / Linking / Bridges is where a user goes to change what the graph is answering. |

## Open questions

- **Does the cone go far enough on hot functions?** Median 39 of 66 nodes, and
  56 of 66 for `resolve_call`. Call-order rows are the main answer to this —
  comprehension is expected to come from sequence rather than from a smaller
  node count — and it should be judged on the built view before anything else
  is added. If a busy function still reads badly, the fix is *presentational*
  (collapsing distant levels, folding a level into a count), never a change to
  what counts as being on the chain.
- ~~Pivot was proposed, not confirmed.~~ **Settled by use.** Click-to-re-centre
  shipped first and was wrong: it replaced the slice as soon as you clicked
  anything in it, defeating the purpose of cutting a large file down to one
  reviewable piece. A click now reviews; moving the centre is deliberate.
- Should the two rings be visually distinct (direct callers heavier or nearer
  than callers-of-callers), or are all four rings just nodes?
- Where does the Vertical button live when no function is selected — hidden,
  or shown disabled with a hint?
- Does Vertical survive a change of file selection, or fall back to All?

## Related work, handled separately

Clicking a function that belongs to another file currently rebuilds the dock
around *that* file, because [`selectFunction`](../../ide/contrib/horizon/browser/media/viewer.js#L1956)
reassigns the primary file (L1974) and `renderFunctionDag` reseeds from
`selectedSet`. That is wrong independently of this feature, and matters more
once Vertical exists, since pivoting must not silently swap the universe. It is
being fixed as its own task.
