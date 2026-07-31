(() => {
  "use strict";

  // —— Constants (Desktop index.dc.html) ——
  const CARD_W = 150;
  const CARD_H = 84;
  const GAP_X = 40;
  const GAP_Y = 40;
  const CRATE_GAP = 72;
  const PAD = 80;
  const STICKY_RESERVE = 260; // leave room for arch sticky on the left of first row
  const STICKY_W = 228;
  const STICKY_H = 120;
  const ZOOM_STEP = 0.12;
  const ZOOM_MIN = 0.25;
  const ZOOM_MAX = 1.5;
  /** Cap auto-fit so tiny maps do not fill the viewport absurdly. */
  const ZOOM_FIT_MAX = 1.0;
  const FIT_MARGIN = 56;
  const TARGET_ASPECT = 16 / 9;
  const DRAG_MOVE = 5;
  const LEFT_DEFAULT = 236;
  const LEFT_MIN = 180;
  const RIGHT_DEFAULT = 316;
  const RIGHT_MIN = 240;

  const els = {
    app: document.getElementById("app"),
    jsonInput: document.getElementById("json-input"),
    toggleLeft: document.getElementById("toggle-left"),
    toggleRight: document.getElementById("toggle-right"),
    toggleTheme: document.getElementById("toggle-theme"),
    projectName: document.getElementById("project-name"),
    brandSep: document.getElementById("brand-sep"),
    brandSub: document.getElementById("brand-sub"),
    frameStats: document.getElementById("frame-stats"),
    switchProject: document.getElementById("switch-project"),
    importScreen: document.getElementById("import-screen"),
    openJson: document.getElementById("open-json"),
    importError: document.getElementById("import-error"),
    importErrorText: document.getElementById("import-error-text"),
    clearError: document.getElementById("clear-error"),
    mainView: document.getElementById("main-view"),
    leftAside: document.getElementById("left-aside"),
    leftRail: document.getElementById("left-rail"),
    rightAside: document.getElementById("right-aside"),
    rightRail: document.getElementById("right-rail"),
    inspector: document.getElementById("inspector"),
    layerSearch: document.getElementById("layer-search"),
    layerRows: document.getElementById("layer-rows"),
    filterChips: document.getElementById("filter-chips"),
    canvas: document.getElementById("canvas"),
    world: document.getElementById("world"),
    cards: document.getElementById("cards"),
    edgePaths: document.getElementById("edge-paths"),
    edgesSvg: document.getElementById("edges-svg"),
    archSticky: document.getElementById("arch-sticky"),
    archText: document.getElementById("arch-text"),
    canvasEmpty: document.getElementById("canvas-empty"),
    zoomOut: document.getElementById("zoom-out"),
    zoomIn: document.getElementById("zoom-in"),
    zoomReset: document.getElementById("zoom-reset"),
  };

  /** @type {object|null} */
  let currentMap = null;
  /** @type {FileNode[]} */
  let fileNodes = [];
  /** @type {FileEdge[]} */
  let fileEdges = [];
  /** @type {Map<string, {x:number,y:number}>} */
  let layout = new Map();
  /** @type {Map<string, {x:number,y:number}>} */
  let nodePos = new Map();
  /** @type {Map<string, HTMLElement>} */
  let cardEls = new Map();
  /** @type {Set<string>} */
  let collapsed = new Set();
  /** @type {string|null} */
  let selectedId = null;
  /** @type {string|null} selected FunctionId, or null when inspecting a file */
  let selectedFnId = null;
  /** @type {Map<string, {fn: object, file: object, fileId: string}>} */
  let fnIndex = new Map();
  /** Selection history for Back after following call targets. */
  /** @type {{fileId: string|null, fnId: string|null}[]} */
  let selHistory = [];
  /** @type {string|null} */
  let hoverId = null;
  let zoom = 1;
  let panX = 0;
  let panY = 0;
  let leftOpen = true;
  let leftW = LEFT_DEFAULT;
  let rightOpen = true;
  let rightW = RIGHT_DEFAULT;
  let dark = false;
  let userSetTheme = false;
  let filters = { entry: true, file: true };
  let query = "";
  let worldW = 1360;
  let worldH = 600;
  /** Bumps on each source fetch so stale responses are ignored. */
  let sourceFetchGen = 0;

  // Pan / drag state
  let panDrag = null;
  let cardDrag = null;

  /**
   * @typedef {{
   *   id: string,
   *   path: string,
   *   name: string,
   *   modulePath: string,
   *   crateName: string,
   *   crateKey: string,
   *   folderKey: string,
   *   folderLabel: string,
   *   kind: "entry"|"file",
   *   fnCount: number,
   *   conflicts: number,
   *   unresolved: number,
   *   file: object,
   * }} FileNode
   */

  /**
   * @typedef {{ from: string, to: string, count: number }} FileEdge
   */

  function basename(path) {
    if (!path) return "";
    const parts = String(path).replace(/\\/g, "/").split("/");
    return parts[parts.length - 1] || path;
  }

  function escapeHtml(s) {
    return String(s)
      .replace(/&/g, "&amp;")
      .replace(/</g, "&lt;")
      .replace(/>/g, "&gt;")
      .replace(/"/g, "&quot;");
  }

  function applyTheme() {
    els.app.dataset.theme = dark ? "dark" : "light";
    els.toggleTheme.textContent = dark ? "☀" : "☾";
  }

  function setLeftWidth(w) {
    leftW = w;
    els.app.style.setProperty("--left-w", `${leftW}px`);
    els.leftRail.style.left = `${leftW - 5}px`;
  }

  function setRightWidth(w) {
    rightW = w;
    els.app.style.setProperty("--right-w", `${rightW}px`);
    els.rightRail.style.right = `${rightW - 5}px`;
  }

  function setRightOpen(open) {
    rightOpen = open;
    els.rightAside.hidden = false;
    els.rightAside.classList.toggle("collapsed", !rightOpen);
    els.rightRail.hidden = !rightOpen;
  }

  function relativePath(absPath) {
    const root = currentMap?.root ? String(currentMap.root).replace(/\\/g, "/") : "";
    const p = String(absPath || "").replace(/\\/g, "/");
    if (root && p.toLowerCase().startsWith(root.toLowerCase())) {
      const rest = p.slice(root.length).replace(/^\//, "");
      return rest || p;
    }
    return p;
  }

  function joinDocs(docs) {
    if (!docs || !docs.length) return "";
    return docs.map((d) => String(d.text || "")).join("\n\n");
  }

  function lineSpanFromTokens(startLine, tokens) {
    let n = 0;
    for (const pair of tokens || []) {
      const text = Array.isArray(pair) ? String(pair[0] ?? "") : "";
      for (let i = 0; i < text.length; i++) if (text.charCodeAt(i) === 10) n += 1;
    }
    const end = startLine + n;
    return end > startLine ? `L${startLine}–${end}` : `L${startLine}`;
  }

  // —— Map indexing ——

  function countKinds(sites) {
    let conflicts = 0;
    let unresolved = 0;
    for (const site of sites || []) {
      if (site.target?.kind === "conflict") conflicts += 1;
      else if (site.target?.kind === "unresolved") unresolved += 1;
    }
    return { conflicts, unresolved };
  }

  function fileFlags(file) {
    const top = countKinds(file.call_sites);
    let conflicts = top.conflicts;
    let unresolved = top.unresolved;
    for (const fn of file.functions || []) {
      const f = countKinds(fn.call_sites);
      conflicts += f.conflicts;
      unresolved += f.unresolved;
    }
    return { conflicts, unresolved };
  }

  function isEntryFile(crate, file) {
    const name = basename(file.path);
    if (!crate.is_library && (name === "main.rs" || file.module_path === "crate")) {
      return true;
    }
    return false;
  }

  function crateKeyOf(crate) {
    return crate.is_library
      ? String(crate.rustc_name || crate.name)
      : `${crate.rustc_name || crate.name}[bin]`;
  }

  /** Display label: package name, with `[bin]` for binary targets (FunctionId convention). */
  function crateLabelOf(crate) {
    const name = String(crate.name || crate.rustc_name || "");
    return crate.is_library ? name : `${name}[bin]`;
  }

  /**
   * Flatten Repository → FileNode[] and build FunctionId indexes.
   */
  function flattenMap(map) {
    /** @type {FileNode[]} */
    const nodes = [];
    /** @type {Map<string, string>} functionId → fileNodeId */
    const fnOwner = new Map();
    /** @type {Map<string, {fn: object, file: object, fileId: string}>} */
    const index = new Map();

    const crates = [...(map.crates || [])].sort((a, b) => {
      const an = String(a.name || "");
      const bn = String(b.name || "");
      if (an !== bn) return an.localeCompare(bn);
      // libs before bins when same package name
      if (!!a.is_library !== !!b.is_library) return a.is_library ? -1 : 1;
      return String(a.rustc_name || "").localeCompare(String(b.rustc_name || ""));
    });

    for (const crate of crates) {
      const ck = crateKeyOf(crate);
      const crateName = crateLabelOf(crate);

      const visitFile = (file, folderKey, folderLabel) => {
        const path = String(file.path || "");
        const id = path || `${ck}::${file.module_path}`;
        const flags = fileFlags(file);
        const kind = isEntryFile(crate, file) ? "entry" : "file";
        const node = {
          id,
          path,
          name: basename(path) || file.module_path || "file",
          modulePath: String(file.module_path || ""),
          crateName,
          crateKey: ck,
          folderKey,
          folderLabel,
          kind,
          fnCount: (file.functions || []).length,
          conflicts: flags.conflicts,
          unresolved: flags.unresolved,
          file,
        };
        nodes.push(node);
        for (const fn of file.functions || []) {
          if (fn.id) {
            const fid = String(fn.id);
            fnOwner.set(fid, id);
            index.set(fid, { fn, file, fileId: id });
          }
        }
      };

      const rootFiles = [...(crate.files || [])].sort((a, b) =>
        String(a.path || "").localeCompare(String(b.path || ""))
      );
      for (const file of rootFiles) {
        visitFile(file, `${ck}::/`, "(crate root)");
      }

      const walkFolder = (folder) => {
        const fpath = String(folder.path || "");
        const label = basename(fpath) || fpath || "folder";
        const key = `${ck}::${fpath}`;
        const files = [...(folder.files || [])].sort((a, b) =>
          String(a.path || "").localeCompare(String(b.path || ""))
        );
        for (const file of files) visitFile(file, key, label);
        const kids = [...(folder.folders || [])].sort((a, b) =>
          String(a.path || "").localeCompare(String(b.path || ""))
        );
        for (const child of kids) walkFolder(child);
      };

      const folders = [...(crate.folders || [])].sort((a, b) =>
        String(a.path || "").localeCompare(String(b.path || ""))
      );
      for (const folder of folders) walkFolder(folder);
    }

    return { nodes, fnOwner, index };
  }

  /**
   * Derive directed file→file edges from Resolved call sites only.
   *
   * Conflict targets: we draw NOTHING. A Conflict may name candidates in
   * several files; picking one would invent a guessed edge and violate the
   * map's non-guessing rule. Ambiguous fan-out belongs in the diagnostics
   * view (later slice), not as a silent map edge. Unresolved has no target
   * file — also omitted.
   */
  function deriveEdges(nodes, fnOwner) {
    /** @type {Map<string, FileEdge>} */
    const edges = new Map();
    const bump = (from, to) => {
      if (!from || !to || from === to) return;
      const key = `${from}\0${to}`;
      const prev = edges.get(key);
      if (prev) prev.count += 1;
      else edges.set(key, { from, to, count: 1 });
    };

    for (const node of nodes) {
      const consider = (site) => {
        if (site.target?.kind !== "resolved") return;
        const tid = String(site.target.data || "");
        const to = fnOwner.get(tid);
        if (to) bump(node.id, to);
      };
      for (const site of node.file.call_sites || []) consider(site);
      for (const fn of node.file.functions || []) {
        for (const site of fn.call_sites || []) consider(site);
      }
    }

    return Array.from(edges.values()).sort((a, b) => {
      if (a.from !== b.from) return a.from.localeCompare(b.from);
      return a.to.localeCompare(b.to);
    });
  }

  /*
   * Deterministic file-card layout
   * =============================
   *
   * The Repository JSON carries no x/y. We place one card per File so that
   * (a) files in the same crate sit in a contiguous cluster, (b) files that
   * share a folder sit adjacent within that cluster, and (c) the same map
   * always yields the same positions (no RNG, no force-directed iteration).
   *
   * Algorithm:
   * 1. Crates are already sorted by name / lib-before-bin (see flattenMap).
   * 2. Within each crate, collect ordered "groups": crate-root files, then
   *    each folder in path order. Files inside a group are path-sorted.
   *    Flatten groups in that order so folder mates stay consecutive.
   * 3. Pack each crate's files into a grid with cols = max(1, round(√n)) so
   *    the cluster is roughly square. Place row-major (left→right, top→down).
   * 4. Pack crate clusters on a meta-grid whose column count is
   *    max(1, round(√(nCrates × 16/9))), so crates flow across and down toward
   *    a landscape composition. First meta-row leaves STICKY_RESERVE on the
   *    left for the architecture note. Uneven cluster sizes mean meta-rows
   *    are height-aligned to the tallest cluster in the row.
   *
   * Limits: does not minimise edge crossings; a single huge crate still
   * dominates its meta-cell; folder adjacency is linear along the row-major
   * stream, not a nested sub-grid per folder. Fine for audit overview, not a
   * research graph layout. Manual card drag updates nodePos overlays only.
   */
  function computeLayout(nodes) {
    /** @type {Map<string, {x:number,y:number}>} */
    const pos = new Map();
    if (!nodes.length) {
      worldW = 1360;
      worldH = 600;
      return pos;
    }

    // Preserve crate order from flattenMap; group files within.
    /** @type {Map<string, {crateKey:string, groups: Map<string, FileNode[]>}>} */
    const crateBlocks = new Map();
    for (const n of nodes) {
      let block = crateBlocks.get(n.crateKey);
      if (!block) {
        block = { crateKey: n.crateKey, groups: new Map() };
        crateBlocks.set(n.crateKey, block);
      }
      let g = block.groups.get(n.folderKey);
      if (!g) {
        g = [];
        block.groups.set(n.folderKey, g);
      }
      g.push(n);
    }

    /** @type {{ files: FileNode[], cols: number, w: number, h: number }[]} */
    const clusters = [];
    for (const block of crateBlocks.values()) {
      /** @type {FileNode[]} */
      const files = [];
      for (const group of block.groups.values()) {
        for (const n of group) files.push(n);
      }
      const n = files.length;
      const cols = Math.max(1, Math.round(Math.sqrt(n)));
      const rows = Math.max(1, Math.ceil(n / cols));
      const w = cols * CARD_W + (cols - 1) * GAP_X;
      const h = rows * CARD_H + (rows - 1) * GAP_Y;
      clusters.push({ files, cols, w, h });
    }

    const crateCols = Math.max(
      1,
      Math.round(Math.sqrt(clusters.length * TARGET_ASPECT))
    );
    /** @type {{ files: FileNode[], cols: number, w: number, h: number }[][]} */
    const metaRows = [];
    for (let i = 0; i < clusters.length; i++) {
      const r = Math.floor(i / crateCols);
      if (!metaRows[r]) metaRows[r] = [];
      metaRows[r].push(clusters[i]);
    }

    let cursorY = PAD;
    let maxX = 0;
    let maxY = 0;

    for (let ri = 0; ri < metaRows.length; ri++) {
      const row = metaRows[ri];
      let cursorX = PAD + (ri === 0 ? STICKY_RESERVE : 0);
      let rowH = 0;

      for (const cluster of row) {
        const cols = cluster.cols;
        for (let i = 0; i < cluster.files.length; i++) {
          const col = i % cols;
          const rowIdx = Math.floor(i / cols);
          const x = cursorX + col * (CARD_W + GAP_X);
          const y = cursorY + rowIdx * (CARD_H + GAP_Y);
          pos.set(cluster.files[i].id, { x, y });
          maxX = Math.max(maxX, x + CARD_W);
          maxY = Math.max(maxY, y + CARD_H);
        }
        cursorX += cluster.w + CRATE_GAP;
        rowH = Math.max(rowH, cluster.h);
      }

      cursorY += rowH + CRATE_GAP;
    }

    worldW = Math.max(1360, maxX + PAD);
    worldH = Math.max(600, maxY + PAD);
    return pos;
  }

  function cardPosition(id) {
    const ov = nodePos.get(id);
    if (ov) return ov;
    return layout.get(id) || { x: 0, y: 0 };
  }

  /** Axis-aligned bounds of laid-out cards (+ architecture sticky when shown). */
  function contentBounds() {
    let minX = Infinity;
    let minY = Infinity;
    let maxX = -Infinity;
    let maxY = -Infinity;
    let any = false;
    for (const n of fileNodes) {
      const p = cardPosition(n.id);
      minX = Math.min(minX, p.x);
      minY = Math.min(minY, p.y);
      maxX = Math.max(maxX, p.x + CARD_W);
      maxY = Math.max(maxY, p.y + CARD_H);
      any = true;
    }
    if (!any) {
      return { minX: 0, minY: 0, maxX: worldW, maxY: worldH };
    }
    if (!els.archSticky.hidden) {
      minX = Math.min(minX, 22);
      minY = Math.min(minY, 14);
      maxX = Math.max(maxX, 22 + STICKY_W);
      maxY = Math.max(maxY, 14 + STICKY_H);
    }
    return { minX, minY, maxX, maxY };
  }

  /**
   * Choose zoom + pan so every card (and sticky) fits in the canvas with margin.
   * Clamped to [ZOOM_MIN, ZOOM_FIT_MAX]. Used on load and as "reset view".
   */
  function fitView() {
    const rect = els.canvas.getBoundingClientRect();
    const b = contentBounds();
    const contentW = Math.max(1, b.maxX - b.minX);
    const contentH = Math.max(1, b.maxY - b.minY);
    const availW = Math.max(1, rect.width - FIT_MARGIN * 2);
    const availH = Math.max(1, rect.height - FIT_MARGIN * 2);
    const next = Math.min(availW / contentW, availH / contentH);
    zoom = Math.min(ZOOM_FIT_MAX, Math.max(ZOOM_MIN, +next.toFixed(3)));
    const cx = (b.minX + b.maxX) / 2;
    const cy = (b.minY + b.maxY) / 2;
    panX = rect.width / 2 - cx * zoom;
    panY = rect.height / 2 - cy * zoom;
    updateWorldTransform();
  }

  // —— Rendering ——

  function updateWorldTransform() {
    els.world.style.width = `${worldW}px`;
    els.world.style.height = `${worldH}px`;
    els.world.style.transform = `translate(${panX}px, ${panY}px) scale(${zoom})`;
    els.edgesSvg.setAttribute("width", String(worldW));
    els.edgesSvg.setAttribute("height", String(worldH));
    els.zoomReset.textContent = `${Math.round(zoom * 100)}%`;
  }

  function connectedIds() {
    const set = new Set();
    if (!selectedId && !hoverId) return set;
    const focus = hoverId || selectedId;
    for (const e of fileEdges) {
      if (e.from === focus) set.add(e.to);
      if (e.to === focus) set.add(e.from);
    }
    return set;
  }

  function cardVisible(node) {
    if (node.kind === "entry" && !filters.entry) return false;
    if (node.kind === "file" && !filters.file) return false;
    if (!query) return true;
    const q = query.toLowerCase();
    return (
      node.name.toLowerCase().includes(q) ||
      node.modulePath.toLowerCase().includes(q) ||
      node.path.toLowerCase().includes(q) ||
      node.crateName.toLowerCase().includes(q)
    );
  }

  function renderEdges() {
    const focus = hoverId || selectedId;
    const focusSet = focus ? new Set([focus]) : new Set();
    const ctr = (id) => {
      const p = cardPosition(id);
      return {
        x: p.x + CARD_W / 2,
        y: p.y + CARD_H / 2,
        l: p.x,
        r: p.x + CARD_W,
        t: p.y,
        b: p.y + CARD_H,
      };
    };

    const frag = document.createDocumentFragment();
    for (const e of fileEdges) {
      const a = ctr(e.from);
      const b = ctr(e.to);
      const dx = b.x - a.x;
      const dy = b.y - a.y;
      let d;
      if (Math.abs(dx) >= Math.abs(dy)) {
        const sx = dx > 0 ? a.r : a.l;
        const sy = a.y;
        const tx = dx > 0 ? b.l : b.r;
        const ty = b.y;
        const mx = (sx + tx) / 2;
        d = `M${sx},${sy} C${mx},${sy} ${mx},${ty} ${tx},${ty}`;
      } else {
        const sx = a.x;
        const sy = dy > 0 ? a.b : a.t;
        const tx = b.x;
        const ty = dy > 0 ? b.t : b.b;
        const my = (sy + ty) / 2;
        d = `M${sx},${sy} C${sx},${my} ${tx},${my} ${tx},${ty}`;
      }
      const hot = focusSet.has(e.from) || focusSet.has(e.to);
      const path = document.createElementNS("http://www.w3.org/2000/svg", "path");
      path.setAttribute("d", d);
      path.setAttribute("marker-end", hot ? "url(#cvarh)" : "url(#cvar)");
      path.setAttribute("class", hot ? "edge-path hot" : "edge-path");
      if (e.count > 1) path.setAttribute("title", `${e.count} calls`);
      frag.appendChild(path);
    }
    els.edgePaths.replaceChildren(frag);
  }

  function renderCards() {
    const conn = connectedIds();
    const focus = hoverId || selectedId;

    els.cards.replaceChildren();
    cardEls = new Map();

    for (const node of fileNodes) {
      const p = cardPosition(node.id);
      const visible = cardVisible(node);
      const isSel = selectedId === node.id;
      // Dim only for filter/search mismatches. Selection is indigo chrome —
      // never wash out the rest of the map (scanning means reading unselected cards).
      const dim = !visible;
      const linked = !!focus && !isSel && conn.has(node.id);

      const wrap = document.createElement("div");
      wrap.className =
        "file-card" +
        (isSel ? " selected" : "") +
        (dim ? " dim" : "") +
        (linked ? " linked" : "");
      wrap.dataset.id = node.id;
      wrap.style.left = `${p.x}px`;
      wrap.style.top = `${p.y}px`;

      const badges = [];
      if (node.conflicts)
        badges.push(
          `<span class="pill-badge conflict" title="Conflicts">${node.conflicts}</span>`
        );
      if (node.unresolved)
        badges.push(
          `<span class="pill-badge unresolved" title="Unresolved">${node.unresolved}</span>`
        );

      wrap.innerHTML =
        `<div class="card-frame">` +
        `<div class="card-name" title="${escapeHtml(node.path)}">${escapeHtml(node.name)}</div>` +
        `<div class="card-module" title="${escapeHtml(node.modulePath)}">${escapeHtml(node.modulePath)}</div>` +
        `<div class="card-head">` +
        `<span class="kind-chip"><span class="kind-dot ${node.kind}"></span>${node.kind}</span>` +
        (badges.length
          ? `<span class="card-badges">${badges.join("")}</span>`
          : "") +
        `</div>` +
        `<div class="card-skel w78"></div>` +
        `<div class="card-skel w58"></div>` +
        `<span class="card-fn-count">${node.fnCount} fn</span>` +
        `<div class="sel-handles">` +
        `<span class="sel-handle tl"></span>` +
        `<span class="sel-handle tr"></span>` +
        `<span class="sel-handle bl"></span>` +
        `<span class="sel-handle br"></span>` +
        (isSel
          ? `<span class="sel-pill">${node.fnCount} fn</span>`
          : "") +
        `</div>` +
        `</div>`;

      const frame = wrap.querySelector(".card-frame");
      frame.addEventListener("pointerdown", (ev) => onCardPointerDown(ev, node.id));
      frame.addEventListener("click", (ev) => {
        ev.stopPropagation();
        if (cardDrag && cardDrag.moved) return;
        selectFile(node.id, { reveal: false });
      });
      wrap.addEventListener("mouseenter", () => {
        hoverId = node.id;
        refreshFocus();
      });
      wrap.addEventListener("mouseleave", () => {
        if (hoverId === node.id) hoverId = null;
        refreshFocus();
      });

      els.cards.appendChild(wrap);
      cardEls.set(node.id, wrap);
    }
  }

  function refreshFocus() {
    const conn = connectedIds();
    const focus = hoverId || selectedId;
    for (const node of fileNodes) {
      const el = cardEls.get(node.id);
      if (!el) continue;
      const visible = cardVisible(node);
      const isSel = selectedId === node.id;
      const dim = !visible;
      const linked = !!focus && !isSel && conn.has(node.id);
      el.classList.toggle("selected", isSel);
      el.classList.toggle("dim", dim);
      el.classList.toggle("linked", linked);
      const pill = el.querySelector(".sel-pill");
      if (isSel && !pill) {
        const handles = el.querySelector(".sel-handles");
        if (handles) {
          const span = document.createElement("span");
          span.className = "sel-pill";
          span.textContent = `${node.fnCount} fn`;
          handles.appendChild(span);
        }
      } else if (!isSel && pill) {
        pill.remove();
      }
    }
    renderEdges();
    renderLayersSelection();
  }

  function renderLayers() {
    const root = els.layerRows;
    root.replaceChildren();

    /** @type {Map<string, {crate: string, crateKey: string, folders: Map<string, FileNode[]>}>} */
    const tree = new Map();
    for (const n of fileNodes) {
      let c = tree.get(n.crateKey);
      if (!c) {
        c = { crate: n.crateName, crateKey: n.crateKey, folders: new Map() };
        tree.set(n.crateKey, c);
      }
      let f = c.folders.get(n.folderKey);
      if (!f) {
        f = [];
        c.folders.set(n.folderKey, f);
      }
      f.push(n);
    }

    for (const block of tree.values()) {
      const crateCollapsed = collapsed.has(`crate:${block.crateKey}`);
      root.appendChild(
        makeLayerRow({
          kind: "crate",
          label: block.crate,
          indent: 0,
          icon: crateCollapsed ? "▸" : "▾",
          iconClass: "crate",
          selected: false,
          onClick: () => {
            const key = `crate:${block.crateKey}`;
            if (collapsed.has(key)) collapsed.delete(key);
            else collapsed.add(key);
            renderLayers();
          },
        })
      );
      if (crateCollapsed) continue;

      for (const [folderKey, files] of block.folders) {
        const folderCollapsed = collapsed.has(`folder:${folderKey}`);
        const folderLabel = files[0]?.folderLabel || basename(folderKey);
        root.appendChild(
          makeLayerRow({
            kind: "folder",
            label: folderLabel,
            indent: 10,
            icon: folderCollapsed ? "▸" : "▾",
            iconClass: "",
            selected: false,
            onClick: () => {
              const key = `folder:${folderKey}`;
              if (collapsed.has(key)) collapsed.delete(key);
              else collapsed.add(key);
              renderLayers();
            },
          })
        );
        if (folderCollapsed) continue;

        for (const n of files) {
          const visible = cardVisible(n);
          const hot = n.conflicts + n.unresolved > 0;
          root.appendChild(
            makeLayerRow({
              kind: "file",
              id: n.id,
              label: n.name,
              indent: 24,
              icon: n.kind === "entry" ? "★" : "#",
              iconClass: n.kind,
              selected: selectedId === n.id,
              dim: !visible,
              badge: hot ? "!" : n.kind === "entry" ? "★" : "",
              badgeClass: hot
                ? n.unresolved
                  ? "hot"
                  : "conflict"
                : n.kind === "entry"
                  ? "star"
                  : "",
              onClick: () => selectFile(n.id, { reveal: true }),
            })
          );
        }
      }
    }
  }

  function makeLayerRow({
    kind,
    id,
    label,
    indent,
    icon,
    iconClass,
    selected,
    dim,
    badge,
    badgeClass,
    onClick,
  }) {
    const btn = document.createElement("button");
    btn.type = "button";
    btn.className =
      "layer-row" +
      (kind === "file" ? " file" : "") +
      (selected ? " selected" : "") +
      (dim ? " dim" : "");
    if (id) btn.dataset.id = id;
    btn.innerHTML =
      `<span class="layer-indent" style="width:${indent}px"></span>` +
      `<span class="layer-icon ${iconClass || ""}">${icon}</span>` +
      `<span class="layer-label">${escapeHtml(label)}</span>` +
      (badge
        ? `<span class="layer-badge ${badgeClass || ""}">${badge}</span>`
        : "");
    btn.addEventListener("click", onClick);
    return btn;
  }

  function renderLayersSelection() {
    for (const btn of els.layerRows.querySelectorAll(".layer-row.file")) {
      btn.classList.toggle("selected", btn.dataset.id === selectedId);
    }
  }

  function updateChrome() {
    const nFiles = fileNodes.length;
    const nEdges = fileEdges.length;
    const summary = currentMap?.summary || {};
    els.frameStats.hidden = false;
    const cN = summary.conflicts ?? 0;
    const uN = summary.unresolved ?? 0;
    els.frameStats.textContent =
      `${nFiles} frame${nFiles === 1 ? "" : "s"} · ${nEdges} link${nEdges === 1 ? "" : "s"}` +
      (cN ? ` · ${cN} conflict${cN === 1 ? "" : "s"}` : "") +
      (uN ? ` · ${uN} unresolved` : "");

    const rootName = basename(currentMap?.root || "") || "map";
    els.projectName.textContent = rootName;
    els.brandSep.hidden = false;
    els.brandSub.hidden = false;
    els.switchProject.hidden = false;
    els.toggleLeft.hidden = false;
    els.toggleRight.hidden = false;

    const crateN = (currentMap?.crates || []).length;
    const conflictN = summary.conflicts ?? 0;
    const unresolvedN = summary.unresolved ?? 0;
    els.archText.textContent =
      `${nFiles} file${nFiles === 1 ? "" : "s"} across ${crateN} crate${crateN === 1 ? "" : "s"}` +
      (conflictN || unresolvedN
        ? ` · ${conflictN} conflict${conflictN === 1 ? "" : "s"}, ${unresolvedN} unresolved`
        : ".") +
      ` Dropped: ${summary.external_dropped ?? 0} external, ` +
      `${summary.constructor_dropped ?? 0} constructor, ` +
      `${summary.associated_dropped ?? 0} associated.`;
    els.archSticky.hidden = nFiles === 0;

    els.canvasEmpty.hidden = nFiles > 0;
    document.title = `Horizon — ${rootName}`;
  }

  function revealCard(id) {
    const p = cardPosition(id);
    const rect = els.canvas.getBoundingClientRect();
    const cx = p.x + CARD_W / 2;
    const cy = p.y + CARD_H / 2;
    panX = rect.width / 2 - cx * zoom;
    panY = rect.height / 2 - cy * zoom;
    updateWorldTransform();
  }

  function snapshotSelection() {
    return { fileId: selectedId, fnId: selectedFnId };
  }

  function pushHistory() {
    const snap = snapshotSelection();
    if (!snap.fileId && !snap.fnId) return;
    const top = selHistory[selHistory.length - 1];
    if (top && top.fileId === snap.fileId && top.fnId === snap.fnId) return;
    selHistory.push(snap);
    if (selHistory.length > 64) selHistory.shift();
  }

  /**
   * Select a file card. Clears function selection unless keepFn and the fn
   * still belongs to this file.
   */
  function selectFile(id, { reveal = false, push = false, keepFn = false } = {}) {
    if (push) pushHistory();
    selectedId = id;
    if (!keepFn) selectedFnId = null;
    if (reveal && id) revealCard(id);
    setRightOpen(true);
    refreshFocus();
    renderInspector();
  }

  /**
   * Select a function by FunctionId — core audit jump.
   * Selects its owning file card, reveals it, opens Inspector on the fn.
   */
  function selectFunction(fnId, { reveal = true, push = true } = {}) {
    const entry = fnIndex.get(String(fnId));
    if (!entry) return false;
    if (push) pushHistory();
    selectedId = entry.fileId;
    selectedFnId = String(fnId);
    if (reveal) revealCard(entry.fileId);
    setRightOpen(true);
    refreshFocus();
    renderInspector();
    return true;
  }

  function goBack() {
    const prev = selHistory.pop();
    if (!prev) return;
    selectedId = prev.fileId;
    selectedFnId = prev.fnId;
    if (selectedId) revealCard(selectedId);
    setRightOpen(true);
    refreshFocus();
    renderInspector();
  }

  // —— Inspector ——

  const SOURCE_MESSAGES = {
    loading: "Loading source…",
    stale:
      "This file changed on disk since analysis — source not shown. Re-analyse to refresh the map.",
    missing: "Source file is missing on disk.",
    unverifiable:
      "Source cannot be verified — this map predates content hashing.",
    no_source:
      "No source range on this function (map predates byte_start/byte_end).",
    not_in_map: "Path is not in the loaded map — source refused.",
    no_map: "No map loaded on the server — cannot fetch source.",
    range: "Source byte range is invalid or empty.",
    bad_request: "Source request was malformed.",
  };

  function sourceUnavailableReason(fn, file) {
    const start = fn.byte_start ?? 0;
    const end = fn.byte_end ?? 0;
    if (start === 0 && end === 0) return "no_source";
    if (!(file.content_hash || "")) return "unverifiable";
    return null;
  }

  function renderTokens(tokens) {
    const pre = document.createElement("pre");
    pre.className = "source-well";
    const code = document.createElement("code");
    for (const pair of tokens || []) {
      const text = Array.isArray(pair) ? String(pair[0] ?? "") : "";
      const cls = Array.isArray(pair) ? String(pair[1] ?? "") : "";
      const span = document.createElement("span");
      span.className = cls ? `tok-${cls}` : "tok";
      span.textContent = text;
      code.appendChild(span);
    }
    pre.appendChild(code);
    return pre;
  }

  function setSourceHost(host, state, detail) {
    host.replaceChildren();
    const metaEl = host._metaEl;
    if (metaEl && detail.meta != null) metaEl.textContent = detail.meta;
    if (state === "served") {
      host.appendChild(renderTokens(detail.tokens));
      return;
    }
    const banner = document.createElement("div");
    banner.className = `source-banner ${state}`;
    banner.textContent =
      SOURCE_MESSAGES[state] ||
      detail.message ||
      SOURCE_MESSAGES.bad_request ||
      "Failed to load source.";
    if (state === "error" && detail.message) banner.textContent = detail.message;
    host.appendChild(banner);
  }

  async function fetchSourceInto(host, file, fn) {
    const filePath = String(file.path || "");
    const start = fn.byte_start ?? 0;
    const end = fn.byte_end ?? 0;
    const hash = file.content_hash || "";
    const name = basename(filePath) || filePath;
    let meta = `${name} · L${fn.line}`;

    const early = sourceUnavailableReason(fn, file);
    if (early) {
      setSourceHost(host, early, { meta });
      return;
    }

    const gen = ++sourceFetchGen;
    setSourceHost(host, "loading", { meta });
    const params = new URLSearchParams({
      path: filePath,
      byte_start: String(start),
      byte_end: String(end),
      expected_hash: hash,
    });
    try {
      const res = await fetch(`/api/source?${params}`);
      if (gen !== sourceFetchGen) return;
      const body = await res.json().catch(() => ({}));
      if (res.ok && Array.isArray(body.tokens)) {
        meta = `${name} · ${lineSpanFromTokens(fn.line, body.tokens)}`;
        setSourceHost(host, "served", { meta, tokens: body.tokens });
        return;
      }
      const err = body.error || "error";
      if (SOURCE_MESSAGES[err]) {
        setSourceHost(host, err, { meta, message: body.message });
      } else {
        setSourceHost(host, "error", {
          meta,
          message: body.message || `Source request failed (${res.status}).`,
        });
      }
    } catch (e) {
      if (gen !== sourceFetchGen) return;
      setSourceHost(host, "error", {
        meta,
        message: String(e.message || e),
      });
    }
  }

  function jumpLink(id) {
    const a = document.createElement("a");
    a.href = "#";
    a.dataset.jumpId = id;
    a.textContent = id;
    a.addEventListener("click", (ev) => {
      ev.preventDefault();
      selectFunction(id, { reveal: true, push: true });
    });
    return a;
  }

  function renderTargetEl(target) {
    const wrap = document.createElement("div");
    wrap.className = "call-target";
    if (!target || !target.kind) {
      wrap.innerHTML = `<span class="raw-id">(missing target)</span>`;
      return wrap;
    }
    if (target.kind === "resolved") {
      const id = String(target.data || "");
      wrap.appendChild(document.createTextNode("→ "));
      if (fnIndex.has(id)) wrap.appendChild(jumpLink(id));
      else {
        const span = document.createElement("span");
        span.className = "raw-id";
        span.title = "No matching function in this map";
        span.textContent = id;
        wrap.appendChild(span);
      }
      return wrap;
    }
    if (target.kind === "conflict") {
      const data = target.data || {};
      const candidates = data.candidates || [];
      wrap.appendChild(document.createTextNode("→ conflict"));
      if (data.reason) {
        const reason = document.createElement("span");
        reason.className = "reason";
        reason.textContent = data.reason;
        wrap.appendChild(reason);
      }
      const ul = document.createElement("ul");
      ul.className = "candidates";
      for (const raw of candidates) {
        const id = String(raw);
        const li = document.createElement("li");
        if (fnIndex.has(id)) li.appendChild(jumpLink(id));
        else {
          const span = document.createElement("span");
          span.className = "raw-id";
          span.textContent = id;
          li.appendChild(span);
        }
        ul.appendChild(li);
      }
      wrap.appendChild(ul);
      return wrap;
    }
    if (target.kind === "unresolved") {
      wrap.appendChild(document.createTextNode("→ unresolved"));
      const reason = target.data?.reason || "";
      if (reason) {
        const el = document.createElement("span");
        el.className = "reason";
        el.textContent = reason;
        wrap.appendChild(el);
      }
      return wrap;
    }
    wrap.innerHTML = `<span class="raw-id">${escapeHtml(JSON.stringify(target))}</span>`;
    return wrap;
  }

  function renderCallSites(sites) {
    const list = document.createElement("ul");
    list.className = "call-list";
    for (const site of sites || []) {
      const kind = site.target?.kind || "unknown";
      const li = document.createElement("li");
      li.className = `call-site ${kind}`;

      const line = document.createElement("span");
      line.className = "call-line";
      line.textContent = `L${site.line}`;

      const path = document.createElement("span");
      path.className = "call-path";
      path.textContent = site.call_path || "";

      const badges = document.createElement("span");
      badges.className = "call-badges";
      const kindBadge = document.createElement("span");
      kindBadge.className = `badge kind-${kind}`;
      kindBadge.textContent = kind;
      badges.appendChild(kindBadge);
      if (site.from_macro) {
        const m = document.createElement("span");
        m.className = "badge macro";
        m.title = "Recovered from macro token tree";
        m.textContent = "macro";
        badges.appendChild(m);
      }
      path.appendChild(badges);

      li.appendChild(line);
      li.appendChild(path);
      li.appendChild(renderTargetEl(site.target));
      list.appendChild(li);
    }
    return list;
  }

  function makeSection(label, bodyEl, metaText) {
    const sec = document.createElement("div");
    sec.className = "insp-section";
    const head = document.createElement("div");
    head.className = "insp-sec-head";
    const lab = document.createElement("div");
    lab.className = "insp-sec-label";
    lab.textContent = label;
    head.appendChild(lab);
    if (metaText) {
      const meta = document.createElement("div");
      meta.className = "insp-sec-meta";
      meta.textContent = metaText;
      head.appendChild(meta);
    }
    sec.appendChild(head);
    sec.appendChild(bodyEl);
    return sec;
  }

  function renderInspector() {
    const root = els.inspector;
    if (!root) return;
    root.replaceChildren();
    sourceFetchGen += 1; // cancel in-flight fetch when selection changes

    if (!selectedId) {
      const empty = document.createElement("div");
      empty.className = "insp-empty muted";
      empty.textContent = "Select a file on the map or in Layers.";
      root.appendChild(empty);
      return;
    }

    const node = fileNodes.find((n) => n.id === selectedId);
    if (!node) {
      const empty = document.createElement("div");
      empty.className = "insp-empty muted";
      empty.textContent = "Selected file is not in this map.";
      root.appendChild(empty);
      return;
    }

    const fnEntry = selectedFnId ? fnIndex.get(selectedFnId) : null;
    const showingFn = !!(fnEntry && fnEntry.fileId === selectedId);

    // Identity
    const identity = document.createElement("div");
    identity.className = "insp-identity";

    const backRow = document.createElement("div");
    backRow.className = "insp-back-row";
    const backBtn = document.createElement("button");
    backBtn.type = "button";
    backBtn.className = "insp-back";
    backBtn.textContent = "← Back";
    backBtn.disabled = selHistory.length === 0;
    backBtn.title =
      selHistory.length === 0
        ? "No previous selection"
        : "Return to previous selection";
    backBtn.addEventListener("click", () => goBack());
    backRow.appendChild(backBtn);
    identity.appendChild(backRow);

    const titleRow = document.createElement("div");
    titleRow.className = "insp-title-row";
    const dot = document.createElement("span");
    const kind = showingFn ? "fn" : node.kind;
    dot.className = `insp-dot ${kind}`;
    const label = document.createElement("span");
    label.className = "insp-label";
    label.textContent = showingFn ? fnEntry.fn.name : node.name;
    label.title = showingFn ? String(fnEntry.fn.id) : node.path;
    const kindChip = document.createElement("span");
    kindChip.className = `insp-kind ${kind}`;
    kindChip.textContent = kind;
    titleRow.appendChild(dot);
    titleRow.appendChild(label);
    titleRow.appendChild(kindChip);
    identity.appendChild(titleRow);

    const pathEl = document.createElement("div");
    pathEl.className = "insp-path";
    if (showingFn) {
      pathEl.textContent = `${fnEntry.fn.module_path} · L${fnEntry.fn.line}`;
    } else {
      pathEl.textContent = relativePath(node.path) || node.path;
    }
    identity.appendChild(pathEl);

    const metaRow = document.createElement("div");
    metaRow.className = "insp-meta-row";
    if (showingFn) {
      metaRow.innerHTML =
        `<span class="insp-pill">${escapeHtml(basename(node.path))}</span>` +
        `<span class="insp-pill">${(fnEntry.fn.call_sites || []).length} call site${
          (fnEntry.fn.call_sites || []).length === 1 ? "" : "s"
        }</span>`;
    } else {
      const pills = [
        `<span class="insp-pill">${escapeHtml(node.modulePath || "—")}</span>`,
        `<span class="insp-pill">${node.fnCount} function${node.fnCount === 1 ? "" : "s"}</span>`,
      ];
      if (node.conflicts)
        pills.push(
          `<span class="insp-pill conflict">${node.conflicts} conflict${
            node.conflicts === 1 ? "" : "s"
          }</span>`
        );
      if (node.unresolved)
        pills.push(
          `<span class="insp-pill unresolved">${node.unresolved} unresolved</span>`
        );
      metaRow.innerHTML = pills.join("");
    }
    identity.appendChild(metaRow);
    root.appendChild(identity);

    if (showingFn) {
      // Documentation
      const docsText = joinDocs(fnEntry.fn.doc_comments);
      const docsBody = document.createElement("p");
      docsBody.className = docsText ? "insp-docs" : "insp-docs empty";
      docsBody.textContent = docsText || "No documentation comments.";
      root.appendChild(makeSection("Documentation", docsBody));

      // Source — meta sits in the section head (Desktop: SOURCE · loc)
      const sourceHost = document.createElement("div");
      sourceHost.className = "insp-source-host";
      const name = basename(fnEntry.file.path) || "file";
      const srcSec = makeSection(
        "Source",
        sourceHost,
        `${name} · L${fnEntry.fn.line}`
      );
      sourceHost._metaEl = srcSec.querySelector(".insp-sec-meta");
      root.appendChild(srcSec);
      fetchSourceInto(sourceHost, fnEntry.file, fnEntry.fn);

      // Call sites
      const sites = fnEntry.fn.call_sites || [];
      if (sites.length) {
        root.appendChild(
          makeSection(
            "Call sites · references",
            renderCallSites(sites),
            `${sites.length}`
          )
        );
      } else {
        const empty = document.createElement("p");
        empty.className = "insp-muted";
        empty.textContent = "No call sites in this function.";
        root.appendChild(makeSection("Call sites · references", empty));
      }
      return;
    }

    // File view
    const docsText = joinDocs(node.file.doc_comments);
    const docsBody = document.createElement("p");
    docsBody.className = docsText ? "insp-docs" : "insp-docs empty";
    docsBody.textContent = docsText || "No module documentation (//!).";
    root.appendChild(makeSection("Documentation", docsBody));

    const fns = [...(node.file.functions || [])].sort(
      (a, b) => (a.line || 0) - (b.line || 0)
    );
    if (fns.length) {
      const ul = document.createElement("ul");
      ul.className = "fn-list";
      for (const fn of fns) {
        const li = document.createElement("li");
        const btn = document.createElement("button");
        btn.type = "button";
        btn.className = "fn-list-item";
        btn.innerHTML =
          `<span class="fn-list-name">${escapeHtml(fn.name)}</span>` +
          `<span class="fn-list-meta">L${fn.line}</span>`;
        btn.addEventListener("click", () =>
          selectFunction(fn.id, { reveal: false, push: true })
        );
        li.appendChild(btn);
        ul.appendChild(li);
      }
      root.appendChild(makeSection("Functions", ul, `${fns.length}`));
    } else {
      const empty = document.createElement("p");
      empty.className = "insp-muted";
      empty.textContent = "No free functions in this file.";
      root.appendChild(makeSection("Functions", empty));
    }

    const modSites = node.file.call_sites || [];
    if (modSites.length) {
      root.appendChild(
        makeSection(
          "Module-level call sites",
          renderCallSites(modSites),
          `${modSites.length}`
        )
      );
    }
  }

  function showLoaded() {
    els.importScreen.hidden = true;
    els.mainView.hidden = false;
    els.toggleRight.hidden = false;
    setRightOpen(true);
  }

  function showImport(errorMsg) {
    currentMap = null;
    fileNodes = [];
    fileEdges = [];
    layout = new Map();
    nodePos = new Map();
    cardEls = new Map();
    fnIndex = new Map();
    selectedId = null;
    selectedFnId = null;
    selHistory = [];
    hoverId = null;
    els.mainView.hidden = true;
    els.importScreen.hidden = false;
    els.brandSep.hidden = true;
    els.brandSub.hidden = true;
    els.frameStats.hidden = true;
    els.switchProject.hidden = true;
    els.toggleLeft.hidden = true;
    els.toggleRight.hidden = true;
    els.rightAside.hidden = true;
    els.rightRail.hidden = true;
    els.projectName.textContent = "Horizon";
    document.title = "Horizon";
    if (errorMsg) {
      els.importError.hidden = false;
      els.importErrorText.textContent = errorMsg;
    } else {
      els.importError.hidden = true;
      els.importErrorText.textContent = "";
    }
  }

  function loadMap(map, label) {
    if (!map || typeof map !== "object" || !Array.isArray(map.crates)) {
      showImport(
        "Invalid Horizon map: expected a Repository object with crates[]."
      );
      return;
    }

    currentMap = map;
    const { nodes, fnOwner, index } = flattenMap(map);
    fileNodes = nodes;
    fileEdges = deriveEdges(nodes, fnOwner);
    fnIndex = index;
    layout = computeLayout(nodes);
    nodePos = new Map();
    collapsed = new Set();
    selHistory = [];
    selectedFnId = null;

    // Prefer an entry file, else first file.
    const entry = nodes.find((n) => n.kind === "entry");
    selectedId = entry ? entry.id : nodes[0]?.id || null;

    // Optional deep-link: ?fn=FunctionId or ?file=basename
    try {
      const q = new URLSearchParams(location.search);
      const wantFn = q.get("fn");
      const wantFile = q.get("file");
      if (wantFn && fnIndex.has(wantFn)) {
        selectedFnId = wantFn;
        selectedId = fnIndex.get(wantFn).fileId;
      } else if (wantFile) {
        const hit = nodes.find(
          (n) =>
            n.name === wantFile ||
            n.path.replace(/\\/g, "/").endsWith("/" + wantFile)
        );
        if (hit) selectedId = hit.id;
      }
    } catch (_) {
      /* ignore */
    }

    showLoaded();
    updateChrome();
    renderCards();
    renderEdges();
    renderLayers();
    fitView();
    if (selectedId) revealCard(selectedId);
    renderInspector();

    // Keep diagnostics module warm / assert it loads (Slice later will render).
    if (window.HorizonDiagnostics && map) {
      try {
        window.HorizonDiagnostics.collectDiagnostics(map);
      } catch (_) {
        /* preserved module must not break the map view */
      }
    }

    void label;
  }

  // —— Interaction ——

  function onCardPointerDown(ev, id) {
    if (ev.button !== 0) return;
    ev.stopPropagation();
    ev.preventDefault();
    const p = cardPosition(id);
    cardDrag = {
      id,
      sx: ev.clientX,
      sy: ev.clientY,
      ox: p.x,
      oy: p.y,
      moved: false,
    };
    const el = cardEls.get(id);
    if (el) el.classList.add("dragging");
    window.addEventListener("pointermove", onCardPointerMove);
    window.addEventListener("pointerup", onCardPointerUp);
  }

  function onCardPointerMove(ev) {
    if (!cardDrag) return;
    const dx = ev.clientX - cardDrag.sx;
    const dy = ev.clientY - cardDrag.sy;
    if (!cardDrag.moved && Math.hypot(dx, dy) < DRAG_MOVE) return;
    cardDrag.moved = true;
    const nx = cardDrag.ox + dx / zoom;
    const ny = cardDrag.oy + dy / zoom;
    nodePos.set(cardDrag.id, { x: nx, y: ny });
    const el = cardEls.get(cardDrag.id);
    if (el) {
      el.style.left = `${nx}px`;
      el.style.top = `${ny}px`;
    }
    renderEdges();
  }

  function onCardPointerUp() {
    if (cardDrag) {
      const el = cardEls.get(cardDrag.id);
      if (el) el.classList.remove("dragging");
    }
    cardDrag = null;
    window.removeEventListener("pointermove", onCardPointerMove);
    window.removeEventListener("pointerup", onCardPointerUp);
  }

  function onCanvasDown(ev) {
    if (ev.button !== 0) return;
    if (ev.target.closest(".file-card") || ev.target.closest(".zoom-hud")) return;
    panDrag = {
      sx: ev.clientX,
      sy: ev.clientY,
      ox: panX,
      oy: panY,
    };
    els.canvas.classList.add("panning");
    window.addEventListener("pointermove", onCanvasMove);
    window.addEventListener("pointerup", onCanvasUp);
  }

  function onCanvasMove(ev) {
    if (!panDrag) return;
    panX = panDrag.ox + (ev.clientX - panDrag.sx);
    panY = panDrag.oy + (ev.clientY - panDrag.sy);
    updateWorldTransform();
  }

  function onCanvasUp() {
    panDrag = null;
    els.canvas.classList.remove("panning");
    window.removeEventListener("pointermove", onCanvasMove);
    window.removeEventListener("pointerup", onCanvasUp);
  }

  // —— Wiring ——

  els.canvas.addEventListener("pointerdown", onCanvasDown);

  els.zoomIn.addEventListener("click", () => {
    zoom = Math.min(ZOOM_MAX, +(zoom + ZOOM_STEP).toFixed(2));
    updateWorldTransform();
  });
  els.zoomOut.addEventListener("click", () => {
    zoom = Math.max(ZOOM_MIN, +(zoom - ZOOM_STEP).toFixed(2));
    updateWorldTransform();
  });
  els.zoomReset.addEventListener("click", () => {
    fitView();
  });

  els.toggleTheme.addEventListener("click", () => {
    dark = !dark;
    userSetTheme = true;
    applyTheme();
  });

  els.toggleLeft.addEventListener("click", () => {
    leftOpen = !leftOpen;
    els.leftAside.classList.toggle("collapsed", !leftOpen);
    els.leftRail.hidden = !leftOpen;
  });

  els.toggleRight.addEventListener("click", () => {
    setRightOpen(!rightOpen);
  });

  let leftResize = null;
  els.leftRail.addEventListener("pointerdown", (ev) => {
    if (!leftOpen) return;
    ev.preventDefault();
    leftResize = { sx: ev.clientX, ow: leftW };
    window.addEventListener("pointermove", onLeftResizeMove);
    window.addEventListener("pointerup", onLeftResizeUp);
  });
  function onLeftResizeMove(ev) {
    if (!leftResize) return;
    setLeftWidth(Math.max(LEFT_MIN, leftResize.ow + (ev.clientX - leftResize.sx)));
  }
  function onLeftResizeUp() {
    leftResize = null;
    window.removeEventListener("pointermove", onLeftResizeMove);
    window.removeEventListener("pointerup", onLeftResizeUp);
  }

  let rightResize = null;
  els.rightRail.addEventListener("pointerdown", (ev) => {
    if (!rightOpen) return;
    ev.preventDefault();
    rightResize = { sx: ev.clientX, ow: rightW };
    window.addEventListener("pointermove", onRightResizeMove);
    window.addEventListener("pointerup", onRightResizeUp);
  });
  function onRightResizeMove(ev) {
    if (!rightResize) return;
    setRightWidth(
      Math.max(RIGHT_MIN, rightResize.ow - (ev.clientX - rightResize.sx))
    );
  }
  function onRightResizeUp() {
    rightResize = null;
    window.removeEventListener("pointermove", onRightResizeMove);
    window.removeEventListener("pointerup", onRightResizeUp);
  }

  els.layerSearch.addEventListener("input", () => {
    query = els.layerSearch.value.trim();
    renderLayers();
    refreshFocus();
  });

  els.filterChips.addEventListener("click", (ev) => {
    const btn = ev.target.closest(".filter-chip");
    if (!btn || btn.disabled) return;
    const kind = btn.dataset.kind;
    if (kind !== "entry" && kind !== "file") return;
    filters[kind] = !filters[kind];
    btn.classList.toggle("on", filters[kind]);
    renderLayers();
    refreshFocus();
  });

  els.openJson.addEventListener("click", () => els.jsonInput.click());
  els.switchProject.addEventListener("click", () => showImport(null));
  els.clearError.addEventListener("click", () => {
    els.importError.hidden = true;
    els.importErrorText.textContent = "";
  });

  els.jsonInput.addEventListener("change", () => {
    const file = els.jsonInput.files && els.jsonInput.files[0];
    if (!file) return;
    const reader = new FileReader();
    reader.onload = async () => {
      try {
        const text = String(reader.result);
        const map = JSON.parse(text);
        try {
          await fetch("/api/map", {
            method: "POST",
            headers: { "Content-Type": "application/json" },
            body: text,
          });
        } catch (_) {
          /* POST optional for canvas-only; source panel later needs it */
        }
        loadMap(map, file.name);
      } catch (err) {
        showImport(`Failed to parse ${file.name}: ${err.message}`);
      }
    };
    reader.onerror = () => showImport(`Failed to read ${file.name}.`);
    reader.readAsText(file);
    els.jsonInput.value = "";
  });

  // Drop on import screen
  els.importScreen.addEventListener("dragover", (ev) => {
    ev.preventDefault();
  });
  els.importScreen.addEventListener("drop", (ev) => {
    ev.preventDefault();
    const file = ev.dataTransfer?.files?.[0];
    if (!file) return;
    const reader = new FileReader();
    reader.onload = async () => {
      try {
        const text = String(reader.result);
        const map = JSON.parse(text);
        try {
          await fetch("/api/map", {
            method: "POST",
            headers: { "Content-Type": "application/json" },
            body: text,
          });
        } catch (_) {}
        loadMap(map, file.name);
      } catch (err) {
        showImport(`Failed to parse ${file.name}: ${err.message}`);
      }
    };
    reader.readAsText(file);
  });

  async function boot() {
    try {
      const q = new URLSearchParams(location.search);
      if (q.get("theme") === "dark") {
        dark = true;
        userSetTheme = true;
      } else if (q.get("theme") === "light") {
        dark = false;
        userSetTheme = true;
      } else {
        dark = window.matchMedia("(prefers-color-scheme: dark)").matches;
      }
    } catch (_) {
      dark = false;
    }
    applyTheme();
    setLeftWidth(LEFT_DEFAULT);
    setRightWidth(RIGHT_DEFAULT);

    try {
      window
        .matchMedia("(prefers-color-scheme: dark)")
        .addEventListener("change", (e) => {
          if (userSetTheme) return;
          dark = e.matches;
          applyTheme();
        });
    } catch (_) {}

    // Expose selection for verification / later slices (Diagnostics, DAG).
    window.HorizonViewer = {
      selectFile: (id, opts) => selectFile(id, opts || {}),
      selectFunction: (id, opts) => selectFunction(id, opts || {}),
      goBack,
      getSelection: () => snapshotSelection(),
    };

    try {
      const res = await fetch("/api/map");
      if (res.ok) {
        const map = await res.json();
        loadMap(map, "startup");
        return;
      }
    } catch (_) {
      /* fall through */
    }
    showImport(null);
  }

  boot();
})();
