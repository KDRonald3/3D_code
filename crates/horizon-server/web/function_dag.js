/**
 * Function call DAG for the bottom Functions tab (Slice 4).
 *
 * Scope: functions defined in the selected file, plus one hop of Resolved
 * callees (even across files) so cross-file edges are visible. Conflict and
 * Unresolved sites become stub sinks — never guessed function nodes — so
 * analyser false-positives read as "could not resolve", not as missing callees.
 *
 * Busy files (many seeds, dense internal edges) are navigated with
 * focus-plus-context: [`neighborhood`] keeps the focused function and every
 * node within N hops along the undirected call graph. Depth `all` restores the
 * full subgraph. Hidden nodes/edges are counted so the banner can stay honest
 * — edge kinds on the visible remnant are unchanged.
 *
 * Layout: deterministic layered placement (no RNG). Seed functions ordered by
 * source line occupy the leftmost columns by topo depth within the subgraph;
 * stubs and external callees sit in the column after their caller.
 *
 * Global: window.HorizonFunctionDag
 */
(() => {
  "use strict";

  const NODE_W = 148;
  const NODE_H = 48;
  const GAP_X = 56;
  const GAP_Y = 18;
  const PAD = 20;

  function basename(path) {
    if (!path) return "";
    const parts = String(path).replace(/\\/g, "/").split("/");
    return parts[parts.length - 1] || path;
  }

  function siteKey(callerId, site) {
    return `${callerId}\0${site.byte_start ?? 0}\0${site.line ?? 0}\0${
      site.call_path || ""
    }`;
  }

  /**
   * @param {object} file File JSON for the selected file
   * @param {string} fileId canvas file-node id
   * @param {Map<string, {fn: object, file: object, fileId: string}>} fnIndex
   * @returns {{nodes: object[], edges: object[], scope: object}}
   */
  function build(file, fileId, fnIndex) {
    /** @type {Map<string, object>} */
    const nodes = new Map();
    /** @type {object[]} */
    const edges = [];

    const seeds = [...(file?.functions || [])].sort(
      (a, b) => (a.line || 0) - (b.line || 0)
    );

    for (const fn of seeds) {
      const id = String(fn.id);
      nodes.set(id, {
        id,
        kind: "function",
        role: "seed",
        name: fn.name || id,
        modulePath: fn.module_path || "",
        line: fn.line || 0,
        fileId,
        filePath: String(file.path || ""),
        fnId: id,
      });
    }

    for (const fn of seeds) {
      const callerId = String(fn.id);
      for (const site of fn.call_sites || []) {
        const kind = site.target?.kind || "unknown";
        const data = site.target?.data;
        const base = {
          callerFnId: callerId,
          callPath: site.call_path || "",
          line: site.line ?? 0,
          byteStart: site.byte_start ?? 0,
          byteEnd: site.byte_end ?? 0,
          fromMacro: !!site.from_macro,
        };

        if (kind === "resolved") {
          const tid = String(data || "");
          if (!tid) continue;
          if (!nodes.has(tid)) {
            const entry = fnIndex?.get?.(tid);
            nodes.set(tid, {
              id: tid,
              kind: "function",
              role: entry && entry.fileId === fileId ? "seed" : "callee",
              name: entry?.fn?.name || tid.split("::").pop() || tid,
              modulePath: entry?.fn?.module_path || "",
              line: entry?.fn?.line || 0,
              fileId: entry?.fileId || null,
              filePath: entry ? String(entry.file.path || "") : "",
              fnId: tid,
              external: !entry || entry.fileId !== fileId,
            });
          }
          edges.push({
            id: `e:${siteKey(callerId, site)}`,
            from: callerId,
            to: tid,
            kind: "resolved",
            ...base,
          });
          continue;
        }

        if (kind === "conflict") {
          const stubId = `conflict:${siteKey(callerId, site)}`;
          const candidates = (data && data.candidates) || [];
          nodes.set(stubId, {
            id: stubId,
            kind: "conflict",
            role: "stub",
            name: site.call_path || "conflict",
            reason: (data && data.reason) || "ambiguous target",
            candidates: candidates.map(String),
            line: site.line ?? 0,
            fileId,
            callerFnId: callerId,
          });
          edges.push({
            id: `e:${siteKey(callerId, site)}`,
            from: callerId,
            to: stubId,
            kind: "conflict",
            ...base,
            reason: (data && data.reason) || "",
            candidates: candidates.map(String),
          });
          continue;
        }

        if (kind === "unresolved") {
          const stubId = `unresolved:${siteKey(callerId, site)}`;
          nodes.set(stubId, {
            id: stubId,
            kind: "unresolved",
            role: "stub",
            name: site.call_path || "unresolved",
            // Honest label — not "missing function".
            reason:
              (data && data.reason) || "analyser could not resolve this call",
            line: site.line ?? 0,
            fileId,
            callerFnId: callerId,
          });
          edges.push({
            id: `e:${siteKey(callerId, site)}`,
            from: callerId,
            to: stubId,
            kind: "unresolved",
            ...base,
            reason: (data && data.reason) || "",
          });
        }
      }
    }

    const nodeList = Array.from(nodes.values()).sort((a, b) =>
      a.id.localeCompare(b.id)
    );
    const edgeList = edges.sort((a, b) => a.id.localeCompare(b.id));

    return {
      nodes: nodeList,
      edges: edgeList,
      scope: {
        fileId,
        filePath: String(file?.path || ""),
        fileName: basename(file?.path || ""),
        seedCount: seeds.length,
        resolvedEdges: edgeList.filter((e) => e.kind === "resolved").length,
        conflictEdges: edgeList.filter((e) => e.kind === "conflict").length,
        unresolvedEdges: edgeList.filter((e) => e.kind === "unresolved").length,
      },
    };
  }

  /**
   * Deterministic layered layout. Returns Map id → {x,y} plus world size.
   * Mutual/recursive calls are common in real maps — drop DFS back-edges so
   * longest-path layering runs on a DAG (no unbounded queue growth).
   * @param {{nodes: object[], edges: object[]}} graph
   */
  function layout(graph) {
    const nodes = graph.nodes || [];
    const edges = graph.edges || [];
    /** @type {Map<string, object>} */
    const byId = new Map(nodes.map((n) => [n.id, n]));
    /** @type {Map<string, string[]>} */
    const outsAll = new Map();
    for (const n of nodes) outsAll.set(n.id, []);
    for (const e of edges) {
      if (!byId.has(e.from) || !byId.has(e.to)) continue;
      outsAll.get(e.from).push(e.to);
    }

    // Classify back-edges (cycle closers) via DFS; keep tree/forward/cross edges.
    /** @type {Set<string>} */
    const backEdgeKey = new Set();
    const WHITE = 0;
    const GRAY = 1;
    const BLACK = 2;
    /** @type {Map<string, number>} */
    const colour = new Map(nodes.map((n) => [n.id, WHITE]));
    const visit = (u) => {
      colour.set(u, GRAY);
      for (const v of outsAll.get(u) || []) {
        const c = colour.get(v) || WHITE;
        if (c === GRAY) {
          backEdgeKey.add(`${u}\0${v}`);
        } else if (c === WHITE) {
          visit(v);
        }
      }
      colour.set(u, BLACK);
    };
    const seeds = nodes
      .filter((n) => n.kind === "function" && n.role === "seed")
      .sort((a, b) => (a.line || 0) - (b.line || 0) || a.id.localeCompare(b.id));
    for (const s of seeds) {
      if ((colour.get(s.id) || WHITE) === WHITE) visit(s.id);
    }
    for (const n of nodes) {
      if ((colour.get(n.id) || WHITE) === WHITE) visit(n.id);
    }

    /** @type {Map<string, string[]>} */
    const outs = new Map();
    /** @type {Map<string, number>} */
    const indeg = new Map();
    for (const n of nodes) {
      outs.set(n.id, []);
      indeg.set(n.id, 0);
    }
    for (const e of edges) {
      if (!byId.has(e.from) || !byId.has(e.to)) continue;
      if (backEdgeKey.has(`${e.from}\0${e.to}`)) continue;
      outs.get(e.from).push(e.to);
      indeg.set(e.to, (indeg.get(e.to) || 0) + 1);
    }

    // Seeds with no in-graph callers start at layer 0; BFS assigns the rest.
    /** @type {Map<string, number>} */
    const layerOf = new Map();
    const queue = [];

    for (const s of seeds) {
      if ((indeg.get(s.id) || 0) === 0) {
        layerOf.set(s.id, 0);
        queue.push(s.id);
      }
    }
    // Seeds that are also callees of other seeds still need a layer.
    for (const s of seeds) {
      if (!layerOf.has(s.id)) {
        layerOf.set(s.id, 0);
        queue.push(s.id);
      }
    }

    // Longest-path layering on the acyclic remnant — each node is enqueued
    // only when its layer increases, and with no cycles that is O(N+E).
    let qi = 0;
    while (qi < queue.length) {
      const id = queue[qi++];
      const L = layerOf.get(id) || 0;
      for (const to of outs.get(id) || []) {
        const next = L + 1;
        if (!layerOf.has(to) || layerOf.get(to) < next) {
          layerOf.set(to, next);
          queue.push(to);
        }
      }
    }

    // Orphans (should not happen) — park at layer 0.
    for (const n of nodes) {
      if (!layerOf.has(n.id)) layerOf.set(n.id, 0);
    }

    /** @type {Map<number, object[]>} */
    const columns = new Map();
    for (const n of nodes) {
      const L = layerOf.get(n.id) || 0;
      if (!columns.has(L)) columns.set(L, []);
      columns.get(L).push(n);
    }
    for (const col of columns.values()) {
      col.sort((a, b) => {
        const ka = a.kind === "function" ? 0 : a.kind === "conflict" ? 1 : 2;
        const kb = b.kind === "function" ? 0 : b.kind === "conflict" ? 1 : 2;
        if (ka !== kb) return ka - kb;
        if ((a.line || 0) !== (b.line || 0)) return (a.line || 0) - (b.line || 0);
        return a.id.localeCompare(b.id);
      });
    }

    const layers = Array.from(columns.keys()).sort((a, b) => a - b);
    /** @type {Map<string, {x:number,y:number}>} */
    const pos = new Map();
    let maxX = PAD;
    let maxY = PAD;

    for (const L of layers) {
      const col = columns.get(L) || [];
      const x = PAD + L * (NODE_W + GAP_X);
      for (let i = 0; i < col.length; i++) {
        const y = PAD + i * (NODE_H + GAP_Y);
        pos.set(col[i].id, { x, y });
        maxX = Math.max(maxX, x + NODE_W);
        maxY = Math.max(maxY, y + NODE_H);
      }
    }

    return {
      pos,
      worldW: Math.max(320, maxX + PAD),
      worldH: Math.max(160, maxY + PAD),
      nodeW: NODE_W,
      nodeH: NODE_H,
    };
  }

  /**
   * Suggest a starting depth for a built graph. Small files stay fully visible;
   * busy ones open at 1 hop so the dock is readable until the reviewer widens it.
   * @param {{nodes?: object[], edges?: object[]}} graph
   * @returns {1|2|'all'}
   */
  function defaultDepth(graph) {
    const n = (graph?.nodes || []).length;
    const e = (graph?.edges || []).length;
    if (n > 24 || e > 40) return 1;
    return "all";
  }

  /**
   * Pick the neighborhood focus: preferred id when present, else the first seed
   * by source line (then id). Returns null only for an empty graph.
   * @param {{nodes?: object[]}} graph
   * @param {string|null|undefined} preferredId
   */
  function pickFocus(graph, preferredId) {
    const nodes = graph?.nodes || [];
    if (!nodes.length) return null;
    const prefer = preferredId != null ? String(preferredId) : "";
    if (prefer && nodes.some((n) => n.id === prefer)) return prefer;
    const seeds = nodes
      .filter((n) => n.kind === "function" && n.role === "seed")
      .sort(
        (a, b) => (a.line || 0) - (b.line || 0) || a.id.localeCompare(b.id)
      );
    if (seeds.length) return seeds[0].id;
    return nodes.slice().sort((a, b) => a.id.localeCompare(b.id))[0].id;
  }

  /**
   * Restrict `graph` to the undirected neighborhood of `focusId` within
   * `depth` hops. `depth === 'all'` (or non-finite) returns the graph unchanged
   * aside from meta. Edges keep their kind / call-site payload — filtering
   * never invents or rewrites targets.
   *
   * @param {{nodes: object[], edges: object[], scope?: object}} graph
   * @param {string|null} focusId
   * @param {number|'all'} depth
   */
  function neighborhood(graph, focusId, depth) {
    const nodes = graph?.nodes || [];
    const edges = graph?.edges || [];
    const totalNodes = nodes.length;
    const totalEdges = edges.length;
    const unlimited =
      depth === "all" ||
      depth == null ||
      depth === Infinity ||
      (typeof depth === "number" && !Number.isFinite(depth));

    if (!nodes.length) {
      return {
        nodes: [],
        edges: [],
        scope: graph?.scope || {},
        meta: {
          focusId: null,
          depth: unlimited ? "all" : depth,
          totalNodes: 0,
          totalEdges: 0,
          shownNodes: 0,
          shownEdges: 0,
          hiddenNodes: 0,
          hiddenEdges: 0,
          truncated: false,
        },
      };
    }

    const focus = pickFocus(graph, focusId);
    if (unlimited) {
      return {
        nodes,
        edges,
        scope: graph?.scope || {},
        meta: {
          focusId: focus,
          depth: "all",
          totalNodes,
          totalEdges,
          shownNodes: totalNodes,
          shownEdges: totalEdges,
          hiddenNodes: 0,
          hiddenEdges: 0,
          truncated: false,
        },
      };
    }

    const hopLimit = Math.max(0, Math.floor(Number(depth) || 0));
    /** @type {Map<string, string[]>} */
    const adj = new Map();
    for (const n of nodes) adj.set(n.id, []);
    for (const e of edges) {
      if (!adj.has(e.from) || !adj.has(e.to)) continue;
      adj.get(e.from).push(e.to);
      adj.get(e.to).push(e.from);
    }

    /** @type {Map<string, number>} */
    const dist = new Map();
    const queue = [];
    if (focus && adj.has(focus)) {
      dist.set(focus, 0);
      queue.push(focus);
    }
    let qi = 0;
    while (qi < queue.length) {
      const id = queue[qi++];
      const d = dist.get(id) || 0;
      if (d >= hopLimit) continue;
      for (const nb of adj.get(id) || []) {
        if (dist.has(nb)) continue;
        dist.set(nb, d + 1);
        queue.push(nb);
      }
    }

    const keep = dist;
    // Depth 0 with a focus still shows the focus node alone.
    if (focus && !keep.has(focus)) keep.set(focus, 0);

    const keptNodes = nodes.filter((n) => keep.has(n.id));
    const keptIds = new Set(keptNodes.map((n) => n.id));
    const keptEdges = edges.filter(
      (e) => keptIds.has(e.from) && keptIds.has(e.to)
    );

    return {
      nodes: keptNodes,
      edges: keptEdges,
      scope: graph?.scope || {},
      meta: {
        focusId: focus,
        depth: hopLimit,
        totalNodes,
        totalEdges,
        shownNodes: keptNodes.length,
        shownEdges: keptEdges.length,
        hiddenNodes: totalNodes - keptNodes.length,
        hiddenEdges: totalEdges - keptEdges.length,
        truncated: keptNodes.length < totalNodes || keptEdges.length < totalEdges,
      },
    };
  }

  window.HorizonFunctionDag = {
    build,
    layout,
    defaultDepth,
    pickFocus,
    neighborhood,
    NODE_W,
    NODE_H,
  };
})();
