(() => {
  "use strict";

  const els = {
    summaryBar: document.getElementById("summary-bar"),
    repoRoot: document.getElementById("repo-root"),
    stats: document.getElementById("stats"),
    toolbar: document.getElementById("toolbar"),
    viewSwitch: document.getElementById("view-switch"),
    treeControls: document.getElementById("tree-controls"),
    diagControls: document.getElementById("diag-controls"),
    search: document.getElementById("search"),
    filterConflicts: document.getElementById("filter-conflicts"),
    filterUnresolved: document.getElementById("filter-unresolved"),
    matchCount: document.getElementById("match-count"),
    diagCount: document.getElementById("diag-count"),
    empty: document.getElementById("empty-state"),
    tree: document.getElementById("tree"),
    diagnostics: document.getElementById("diagnostics"),
    fileInput: document.getElementById("file-input"),
    diagLayoutGrouped: document.getElementById("diag-layout-grouped"),
    diagLayoutFile: document.getElementById("diag-layout-file"),
    diagKindAll: document.getElementById("diag-kind-all"),
    diagKindConflict: document.getElementById("diag-kind-conflict"),
    diagKindUnresolved: document.getElementById("diag-kind-unresolved"),
  };

  /** @type {Map<string, {fn: object, file: object, filePath: string, el: HTMLElement|null}>} */
  let idIndex = new Map();
  /** @type {WeakMap<HTMLElement, object>} */
  let nodeData = new WeakMap();
  /** Absolute file path → file node element (filled when the file node is built). */
  let fileNodeByPath = new Map();
  let filterState = { text: "", conflicts: false, unresolved: false };
  let sourceLabel = "";
  /** @type {object|null} */
  let currentMap = null;
  /** @type {"tree"|"diagnostics"} */
  let activeView = "tree";
  /** @type {Array<object>} */
  let diagEntries = [];
  /** @type {{walkedConflicts: number, walkedUnresolved: number, summaryConflicts: number, summaryUnresolved: number, match: boolean}|null} */
  let diagReconcile = null;

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

  function countKinds(sites) {
    let conflicts = 0;
    let unresolved = 0;
    for (const site of sites || []) {
      if (site.target?.kind === "conflict") conflicts += 1;
      else if (site.target?.kind === "unresolved") unresolved += 1;
    }
    return { conflicts, unresolved };
  }

  function functionFlags(fn) {
    return countKinds(fn.call_sites);
  }

  function fileFlags(file) {
    const top = countKinds(file.call_sites);
    let conflicts = top.conflicts;
    let unresolved = top.unresolved;
    for (const fn of file.functions || []) {
      const f = functionFlags(fn);
      conflicts += f.conflicts;
      unresolved += f.unresolved;
    }
    return { conflicts, unresolved };
  }

  function folderFlags(folder) {
    let conflicts = 0;
    let unresolved = 0;
    for (const file of folder.files || []) {
      const f = fileFlags(file);
      conflicts += f.conflicts;
      unresolved += f.unresolved;
    }
    for (const child of folder.folders || []) {
      const f = folderFlags(child);
      conflicts += f.conflicts;
      unresolved += f.unresolved;
    }
    return { conflicts, unresolved };
  }

  function crateFlags(crate) {
    let conflicts = 0;
    let unresolved = 0;
    for (const file of crate.files || []) {
      const f = fileFlags(file);
      conflicts += f.conflicts;
      unresolved += f.unresolved;
    }
    for (const folder of crate.folders || []) {
      const f = folderFlags(folder);
      conflicts += f.conflicts;
      unresolved += f.unresolved;
    }
    return { conflicts, unresolved };
  }

  function buildIndex(map) {
    idIndex = new Map();
    fileNodeByPath = new Map();
    const visitFile = (file) => {
      const filePath = String(file.path || "");
      for (const fn of file.functions || []) {
        idIndex.set(fn.id, { fn, file, filePath, el: null });
      }
    };
    const visitFolder = (folder) => {
      for (const file of folder.files || []) visitFile(file);
      for (const child of folder.folders || []) visitFolder(child);
    };
    for (const crate of map.crates || []) {
      for (const file of crate.files || []) visitFile(file);
      for (const folder of crate.folders || []) visitFolder(folder);
    }
  }

  function countFunctions(map) {
    let n = 0;
    const visitFile = (file) => {
      n += (file.functions || []).length;
    };
    const visitFolder = (folder) => {
      for (const file of folder.files || []) visitFile(file);
      for (const child of folder.folders || []) visitFolder(child);
    };
    for (const crate of map.crates || []) {
      for (const file of crate.files || []) visitFile(file);
      for (const folder of crate.folders || []) visitFolder(folder);
    }
    return n;
  }

  function renderStats(summary) {
    const items = [
      { key: "conflicts", label: "conflicts", cls: "warning" },
      { key: "unresolved", label: "unresolved", cls: "danger" },
      { key: "external_dropped", label: "external dropped" },
      { key: "constructor_dropped", label: "constructor dropped" },
      { key: "associated_dropped", label: "associated dropped" },
    ];
    els.stats.innerHTML = items
      .map((item) => {
        const value = summary?.[item.key] ?? 0;
        const cls = item.cls && value > 0 ? item.cls : "";
        return `<span class="stat ${cls}"><strong>${value}</strong> ${item.label}</span>`;
      })
      .join("");
  }

  function flagPills(flags) {
    const parts = [];
    if (flags.conflicts)
      parts.push(`<span class="pill conflict">${flags.conflicts} conflict</span>`);
    if (flags.unresolved)
      parts.push(`<span class="pill unresolved">${flags.unresolved} unresolved</span>`);
    if (!parts.length) return "";
    return `<span class="badge-count">${parts.join("")}</span>`;
  }

  function renderDocs(docs) {
    if (!docs || !docs.length) return null;
    const box = document.createElement("div");
    box.className = "docs";
    box.textContent = docs.map((d) => d.text).join("\n\n");
    return box;
  }

  function renderTargetHtml(target) {
    if (!target || !target.kind) {
      return `<span class="raw-id">(missing target)</span>`;
    }
    if (target.kind === "resolved") {
      const id = target.data;
      if (idIndex.has(id)) {
        return `→ <a href="#" data-jump-id="${escapeHtml(id)}">${escapeHtml(id)}</a>`;
      }
      return `→ <span class="raw-id" title="No matching function node">${escapeHtml(id)}</span>`;
    }
    if (target.kind === "conflict") {
      const data = target.data || {};
      const candidates = data.candidates || [];
      const links = candidates
        .map((id) => {
          if (idIndex.has(id)) {
            return `<li><a href="#" data-jump-id="${escapeHtml(id)}">${escapeHtml(id)}</a></li>`;
          }
          return `<li><span class="raw-id">${escapeHtml(id)}</span></li>`;
        })
        .join("");
      return (
        `→ conflict` +
        (data.reason ? `<span class="reason">${escapeHtml(data.reason)}</span>` : "") +
        `<ul class="candidates">${links}</ul>`
      );
    }
    if (target.kind === "unresolved") {
      const reason = target.data?.reason || "";
      return (
        `→ unresolved` +
        (reason ? `<span class="reason">${escapeHtml(reason)}</span>` : "")
      );
    }
    return `<span class="raw-id">${escapeHtml(JSON.stringify(target))}</span>`;
  }

  function renderCallSites(sites) {
    const list = document.createElement("ul");
    list.className = "call-list";
    for (const site of sites || []) {
      const kind = site.target?.kind || "unknown";
      const li = document.createElement("li");
      li.className = `call-site ${kind}`;
      const macro = site.from_macro
        ? `<span class="badge macro" title="Recovered from macro token tree">macro</span>`
        : "";
      li.innerHTML =
        `<span class="call-line">L${site.line}</span>` +
        `<span class="call-path">${escapeHtml(site.call_path)}` +
        `<span class="badge kind-${kind}">${kind}</span>${macro}</span>` +
        `<div class="target">${renderTargetHtml(site.target)}</div>`;
      list.appendChild(li);
    }
    return list;
  }

  function createNode({ kind, titleHtml, pathText, open, lazyBuild, flags, matchKeys, filePath }) {
    const node = document.createElement("div");
    node.className = `node kind-${kind}`;
    if (open) node.classList.add("open");

    const row = document.createElement("button");
    row.type = "button";
    row.className = "node-row";
    row.innerHTML =
      `<span class="twisty">${open ? "▼" : "▶"}</span>` +
      `<span class="label">${titleHtml}${flagPills(flags || { conflicts: 0, unresolved: 0 })}` +
      (pathText ? `<span class="path">${escapeHtml(pathText)}</span>` : "") +
      `</span>`;

    const children = document.createElement("div");
    children.className = "children";

    let built = false;
    const ensureBuilt = () => {
      if (built || !lazyBuild) return;
      built = true;
      lazyBuild(children);
    };

    if (open && lazyBuild) ensureBuilt();

    row.addEventListener("click", () => {
      const willOpen = !node.classList.contains("open");
      if (willOpen) ensureBuilt();
      node.classList.toggle("open", willOpen);
      row.querySelector(".twisty").textContent = willOpen ? "▼" : "▶";
    });

    node.appendChild(row);
    node.appendChild(children);
    nodeData.set(node, {
      kind,
      flags: flags || { conflicts: 0, unresolved: 0 },
      matchKeys: matchKeys || [],
      ensureBuilt,
      row,
      filePath: filePath || null,
    });
    return node;
  }

  // —— Phase C source panel (minimal proof surface; next UI will replace) ——

  function sourceUnavailableReason(fn, file) {
    const start = fn.byte_start ?? 0;
    const end = fn.byte_end ?? 0;
    if (start === 0 && end === 0) return "no_source";
    if (!(file.content_hash || "")) return "unverifiable";
    return null;
  }

  function renderTokens(tokens) {
    const pre = document.createElement("pre");
    const code = document.createElement("code");
    for (const pair of tokens || []) {
      const text = pair[0] ?? "";
      const cls = pair[1] || "";
      const span = document.createElement("span");
      span.className = cls ? `tok-${cls}` : "tok";
      span.textContent = text;
      code.appendChild(span);
    }
    pre.appendChild(code);
    return pre;
  }

  function setSourcePanelState(panel, state, detail) {
    panel.replaceChildren();
    const meta = document.createElement("div");
    meta.className = "source-panel-meta";
    meta.textContent = detail.meta || "";
    panel.appendChild(meta);

    if (state === "served") {
      panel.appendChild(renderTokens(detail.tokens));
      return;
    }

    const banner = document.createElement("div");
    banner.className = `source-panel-banner ${state}`;
    const messages = {
      loading: "Loading source…",
      stale: "Source changed since the map was built — re-analyse. Slice not shown.",
      missing: "Source file is missing on disk.",
      unverifiable: "Source hash unavailable (map predates content_hash) — cannot verify slice.",
      no_source: "No source range on this function (map predates byte_start/byte_end).",
      error: detail.message || "Failed to load source.",
    };
    banner.textContent = messages[state] || messages.error;
    panel.appendChild(banner);
  }

  async function fetchSourceInto(panel, file, fn) {
    const filePath = String(file.path || "");
    const start = fn.byte_start ?? 0;
    const end = fn.byte_end ?? 0;
    const hash = file.content_hash || "";
    const meta = `${filePath} · bytes [${start}, ${end}) · L${fn.line}`;

    const early = sourceUnavailableReason(fn, file);
    if (early) {
      setSourcePanelState(panel, early, { meta });
      return;
    }

    setSourcePanelState(panel, "loading", { meta });
    const params = new URLSearchParams({
      path: filePath,
      byte_start: String(start),
      byte_end: String(end),
      expected_hash: hash,
    });
    try {
      const res = await fetch(`/api/source?${params}`);
      const body = await res.json().catch(() => ({}));
      if (res.ok && Array.isArray(body.tokens)) {
        setSourcePanelState(panel, "served", { meta, tokens: body.tokens });
        return;
      }
      const err = body.error || "error";
      if (err === "stale" || err === "missing" || err === "unverifiable" || err === "no_source") {
        setSourcePanelState(panel, err, { meta, message: body.message });
      } else {
        setSourcePanelState(panel, "error", {
          meta,
          message: body.message || `Source request failed (${res.status}).`,
        });
      }
    } catch (e) {
      setSourcePanelState(panel, "error", {
        meta,
        message: String(e.message || e),
      });
    }
  }

  function openSourceForFunction(fn, file, hostEl) {
    if (!hostEl) return;
    let panel = hostEl.querySelector(":scope > .source-panel");
    if (!panel) {
      panel = document.createElement("div");
      panel.className = "source-panel";
      hostEl.insertBefore(panel, hostEl.firstChild);
    }
    fetchSourceInto(panel, file, fn);
  }

  function openSourceForDiagnostics(entry) {
    if (entry.enclosing.type !== "function" || !entry.enclosing.functionId) return;
    const indexed = idIndex.get(entry.enclosing.functionId);
    if (!indexed || !indexed.el) return;
    const children = indexed.el.querySelector(":scope > .children");
    openSourceForFunction(indexed.fn, indexed.file, children);
  }

  function buildFunctionNode(fn, file) {
    const flags = functionFlags(fn);
    const node = createNode({
      kind: "function",
      open: false,
      flags,
      matchKeys: [fn.name, fn.id, fn.module_path],
      titleHtml:
        `<span class="kind-tag">fn</span>` +
        `<span class="name">${escapeHtml(fn.name)}</span>` +
        `<span class="meta">L${fn.line} · ${escapeHtml(fn.id)}</span>`,
      lazyBuild: (children) => {
        const panel = document.createElement("div");
        panel.className = "source-panel";
        children.appendChild(panel);
        fetchSourceInto(panel, file, fn);

        const docs = renderDocs(fn.doc_comments);
        if (docs) children.appendChild(docs);
        if ((fn.call_sites || []).length) {
          const label = document.createElement("div");
          label.className = "fn-calls-label";
          label.textContent = `Call sites (${fn.call_sites.length})`;
          children.appendChild(label);
          children.appendChild(renderCallSites(fn.call_sites));
        } else {
          const empty = document.createElement("div");
          empty.className = "docs";
          empty.textContent = "No call sites.";
          children.appendChild(empty);
        }
      },
    });
    const entry = idIndex.get(fn.id);
    if (entry) entry.el = node;
    else
      idIndex.set(fn.id, {
        fn,
        file,
        filePath: String(file.path || ""),
        el: node,
      });
    node.dataset.functionId = fn.id;
    return node;
  }

  function buildFileNode(file) {
    const flags = fileFlags(file);
    const filePath = String(file.path || "");
    const matchKeys = [basename(file.path), file.path, file.module_path];
    for (const fn of file.functions || []) {
      matchKeys.push(fn.name, fn.id);
    }
    const node = createNode({
      kind: "file",
      open: false,
      flags,
      matchKeys,
      filePath,
      titleHtml:
        `<span class="kind-tag">file</span>` +
        `<span class="name">${escapeHtml(basename(file.path))}</span>` +
        `<span class="meta">${escapeHtml(file.module_path)} · ${(file.functions || []).length} fn</span>`,
      pathText: file.path,
      lazyBuild: (children) => {
        const docs = renderDocs(file.doc_comments);
        if (docs) children.appendChild(docs);
        if ((file.call_sites || []).length) {
          const label = document.createElement("div");
          label.className = "file-calls-label";
          label.textContent = `Module-level call sites (${file.call_sites.length})`;
          children.appendChild(label);
          children.appendChild(renderCallSites(file.call_sites));
        }
        for (const fn of file.functions || []) {
          children.appendChild(buildFunctionNode(fn, file));
        }
      },
    });
    fileNodeByPath.set(filePath, node);
    return node;
  }

  function buildFolderNode(folder) {
    const flags = folderFlags(folder);
    return createNode({
      kind: "folder",
      open: true,
      flags,
      matchKeys: [basename(folder.path), folder.path],
      titleHtml:
        `<span class="kind-tag">dir</span>` +
        `<span class="name">${escapeHtml(basename(folder.path))}</span>`,
      pathText: folder.path,
      lazyBuild: (children) => {
        for (const child of folder.folders || []) {
          children.appendChild(buildFolderNode(child));
        }
        for (const file of folder.files || []) {
          children.appendChild(buildFileNode(file));
        }
      },
    });
  }

  function buildCrateNode(crate) {
    const flags = crateFlags(crate);
    const kind = crate.is_library ? "lib" : "bin";
    return createNode({
      kind: "crate",
      open: true,
      flags,
      matchKeys: [crate.name, crate.rustc_name],
      titleHtml:
        `<span class="kind-tag ${kind}">${kind}</span>` +
        `<span class="name">${escapeHtml(crate.name)}</span>` +
        `<span class="meta">edition ${escapeHtml(crate.edition)} · ${escapeHtml(crate.rustc_name)}</span>`,
      lazyBuild: (children) => {
        for (const folder of crate.folders || []) {
          children.appendChild(buildFolderNode(folder));
        }
        for (const file of crate.files || []) {
          children.appendChild(buildFileNode(file));
        }
      },
    });
  }

  function setOpen(node, open) {
    const data = nodeData.get(node);
    if (!data) return;
    if (open) data.ensureBuilt();
    node.classList.toggle("open", open);
    const twisty = data.row.querySelector(".twisty");
    if (twisty) twisty.textContent = open ? "▼" : "▶";
  }

  function expandAncestors(el) {
    let cur = el.parentElement;
    while (cur) {
      if (cur.classList && cur.classList.contains("node")) {
        setOpen(cur, true);
      }
      cur = cur.parentElement;
    }
  }

  /**
   * Ensure crates/folders that may contain `filePath` are built, then open
   * only that file — not every file in the tree (old-viewer rough edge #1).
   */
  function materializeFile(filePath) {
    const containers = els.tree.querySelectorAll(
      ".node.kind-crate, .node.kind-folder"
    );
    for (const n of containers) setOpen(n, true);

    let fileNode = fileNodeByPath.get(filePath);
    if (!fileNode) {
      for (const node of els.tree.querySelectorAll(".node.kind-file")) {
        const data = nodeData.get(node);
        if (data && data.filePath === filePath) {
          fileNode = node;
          fileNodeByPath.set(filePath, node);
          break;
        }
      }
    }
    if (fileNode) setOpen(fileNode, true);
    return fileNode || null;
  }

  function flashRow(row) {
    if (!row) return;
    row.classList.add("highlight");
    row.scrollIntoView({ behavior: "smooth", block: "center" });
    setTimeout(() => row.classList.remove("highlight"), 1600);
  }

  function jumpToFunction(id) {
    const entry = idIndex.get(id);
    if (!entry) return false;

    if (!entry.el || !document.body.contains(entry.el)) {
      materializeFile(entry.filePath);
    }

    const node =
      entry.el ||
      els.tree.querySelector(`[data-function-id="${CSS.escape(id)}"]`);
    if (!node) return false;
    entry.el = node;

    expandAncestors(node);
    setOpen(node, true);
    node.classList.remove("hidden-by-filter");

    const row = node.querySelector(":scope > .node-row");
    flashRow(row);
    return true;
  }

  /** Jump to a file node (module-level call sites have no enclosing FunctionId). */
  function jumpToFile(filePath) {
    const fileNode = materializeFile(filePath);
    if (!fileNode) return false;
    expandAncestors(fileNode);
    setOpen(fileNode, true);
    fileNode.classList.remove("hidden-by-filter");
    const row = fileNode.querySelector(":scope > .node-row");
    flashRow(row);
    return true;
  }

  function functionMatches(node, state) {
    const data = nodeData.get(node);
    if (!data) return true;
    const textOk =
      !state.text ||
      data.matchKeys.some((k) => String(k).toLowerCase().includes(state.text));
    const flagOk =
      (!state.conflicts && !state.unresolved) ||
      (state.conflicts && data.flags.conflicts > 0) ||
      (state.unresolved && data.flags.unresolved > 0);
    return textOk && flagOk;
  }

  function applyFilters() {
    filterState = {
      text: els.search.value.trim().toLowerCase(),
      conflicts: els.filterConflicts.checked,
      unresolved: els.filterUnresolved.checked,
    };
    const filtering =
      !!filterState.text || filterState.conflicts || filterState.unresolved;

    if (filtering) {
      els.tree
        .querySelectorAll(".node.kind-crate, .node.kind-folder, .node.kind-file")
        .forEach((n) => {
          const data = nodeData.get(n);
          if (data) data.ensureBuilt();
        });
    }

    let visibleFns = 0;
    for (const node of els.tree.querySelectorAll(".node.kind-function")) {
      const show = !filtering || functionMatches(node, filterState);
      node.classList.toggle("hidden-by-filter", !show);
      if (show) visibleFns += 1;
    }

    const containers = Array.from(
      els.tree.querySelectorAll(".node.kind-file, .node.kind-folder, .node.kind-crate")
    ).reverse();

    for (const node of containers) {
      if (!filtering) {
        node.classList.remove("hidden-by-filter");
        continue;
      }
      const data = nodeData.get(node);
      const childNodes = node.querySelectorAll(":scope > .children > .node");
      const anyVisibleChild = Array.from(childNodes).some(
        (c) => !c.classList.contains("hidden-by-filter")
      );
      const selfText =
        !!filterState.text &&
        data.matchKeys.some((k) => String(k).toLowerCase().includes(filterState.text));
      const flagOk =
        (!filterState.conflicts && !filterState.unresolved) ||
        (filterState.conflicts && data.flags.conflicts > 0) ||
        (filterState.unresolved && data.flags.unresolved > 0);
      const show = flagOk && (anyVisibleChild || selfText);
      node.classList.toggle("hidden-by-filter", !show);
      if (show && anyVisibleChild) setOpen(node, true);
    }

    const total = idIndex.size;
    const base = filtering
      ? `${visibleFns} / ${total} functions`
      : `${total} functions`;
    els.matchCount.textContent = sourceLabel ? `${base} · ${sourceLabel}` : base;
  }

  // —— Diagnostics worklist (Phase F) ——

  /**
   * Walk every Conflict and Unresolved CallSite in the loaded map.
   * Deliberate drops (external/constructor/associated) are never in the tree.
   */
  function collectDiagnostics(map) {
    const entries = [];
    let walkedConflicts = 0;
    let walkedUnresolved = 0;

    const pushSite = (site, filePath, enclosing) => {
      const kind = site.target?.kind;
      if (kind !== "conflict" && kind !== "unresolved") return;
      if (kind === "conflict") walkedConflicts += 1;
      else walkedUnresolved += 1;
      const data = site.target.data || {};
      entries.push({
        kind,
        callPath: site.call_path || "",
        line: site.line ?? 0,
        fromMacro: !!site.from_macro,
        reason: data.reason || "",
        candidates: kind === "conflict" ? data.candidates || [] : [],
        filePath: String(filePath || ""),
        enclosing,
      });
    };

    const visitFile = (file) => {
      const filePath = String(file.path || "");
      for (const site of file.call_sites || []) {
        pushSite(site, filePath, {
          type: "file",
          label: `file ${basename(filePath)}`,
          filePath,
        });
      }
      for (const fn of file.functions || []) {
        for (const site of fn.call_sites || []) {
          pushSite(site, filePath, {
            type: "function",
            label: fn.id,
            functionId: fn.id,
            filePath,
          });
        }
      }
    };

    const visitFolder = (folder) => {
      for (const file of folder.files || []) visitFile(file);
      for (const child of folder.folders || []) visitFolder(child);
    };

    for (const crate of map.crates || []) {
      for (const file of crate.files || []) visitFile(file);
      for (const folder of crate.folders || []) visitFolder(folder);
    }

    const summary = map.summary || {};
    const summaryConflicts = summary.conflicts ?? 0;
    const summaryUnresolved = summary.unresolved ?? 0;
    return {
      entries,
      reconcile: {
        walkedConflicts,
        walkedUnresolved,
        summaryConflicts,
        summaryUnresolved,
        match:
          walkedConflicts === summaryConflicts &&
          walkedUnresolved === summaryUnresolved,
      },
    };
  }

  function diagKindFilter() {
    if (els.diagKindConflict.checked) return "conflict";
    if (els.diagKindUnresolved.checked) return "unresolved";
    return "all";
  }

  function diagLayoutMode() {
    return els.diagLayoutFile.checked ? "file" : "grouped";
  }

  function filteredDiagEntries() {
    const kind = diagKindFilter();
    if (kind === "all") return diagEntries;
    return diagEntries.filter((e) => e.kind === kind);
  }

  function groupByReason(entries) {
    /** @type {Map<string, {reason: string, kinds: Set<string>, entries: object[]}>} */
    const groups = new Map();
    for (const e of entries) {
      const key = e.reason || "(no reason)";
      let g = groups.get(key);
      if (!g) {
        g = { reason: key, kinds: new Set(), entries: [] };
        groups.set(key, g);
      }
      g.kinds.add(e.kind);
      g.entries.push(e);
    }
    return Array.from(groups.values()).sort((a, b) => {
      if (b.entries.length !== a.entries.length) {
        return b.entries.length - a.entries.length;
      }
      return a.reason.localeCompare(b.reason);
    });
  }

  function groupByFile(entries) {
    /** @type {Map<string, {filePath: string, kinds: Set<string>, entries: object[]}>} */
    const groups = new Map();
    for (const e of entries) {
      const key = e.filePath || "(no path)";
      let g = groups.get(key);
      if (!g) {
        g = { filePath: key, kinds: new Set(), entries: [] };
        groups.set(key, g);
      }
      g.kinds.add(e.kind);
      g.entries.push(e);
    }
    for (const g of groups.values()) {
      g.entries.sort((a, b) => {
        if (a.line !== b.line) return a.line - b.line;
        return a.callPath.localeCompare(b.callPath);
      });
    }
    return Array.from(groups.values()).sort((a, b) =>
      a.filePath.localeCompare(b.filePath)
    );
  }

  function countClassForKinds(kinds) {
    if (kinds.size === 1) {
      return kinds.has("conflict") ? "conflict" : "unresolved";
    }
    return "mixed";
  }

  function renderDiagEntry(entry) {
    const btn = document.createElement("button");
    btn.type = "button";
    btn.className = `diag-entry ${entry.kind}`;
    btn.title = "Jump to this site in the tree";

    const macro = entry.fromMacro
      ? `<span class="badge macro" title="Recovered from macro token tree">macro</span>`
      : "";

    let candidatesHtml = "";
    if (entry.kind === "conflict" && entry.candidates.length) {
      candidatesHtml =
        `<ul class="candidates">` +
        entry.candidates
          .map((id) => `<li>${escapeHtml(id)}</li>`)
          .join("") +
        `</ul>`;
    }

    const enclosingLabel =
      entry.enclosing.type === "function"
        ? `in ${entry.enclosing.label}`
        : `module-level · ${entry.enclosing.label}`;

    btn.innerHTML =
      `<span class="call-line">L${entry.line}</span>` +
      `<span class="call-path">${escapeHtml(entry.callPath)}` +
      `<span class="badge kind-${entry.kind}">${entry.kind}</span>${macro}</span>` +
      `<div class="diag-entry-body">` +
      (entry.reason
        ? `<span class="reason">${escapeHtml(entry.reason)}</span>`
        : "") +
      `<span class="enclosing">${escapeHtml(enclosingLabel)}</span>` +
      `<span class="path">${escapeHtml(entry.filePath)}</span>` +
      candidatesHtml +
      `</div>`;

    btn.addEventListener("click", () => navigateFromDiagnostics(entry));
    return btn;
  }

  /**
   * Navigate to the enclosing function (or file) in the tree, then open source.
   */
  function navigateFromDiagnostics(entry) {
    setActiveView("tree");
    let ok = false;
    if (entry.enclosing.type === "function" && entry.enclosing.functionId) {
      ok = jumpToFunction(entry.enclosing.functionId);
    } else {
      ok = jumpToFile(entry.filePath);
    }
    if (ok) openSourceForDiagnostics(entry);
  }

  function renderDiagGroup({ title, meta, count, countClass, entries, open }) {
    const group = document.createElement("div");
    group.className = "diag-group" + (open ? " open" : "");

    const header = document.createElement("button");
    header.type = "button";
    header.className = "diag-group-header";
    header.innerHTML =
      `<span class="twisty">${open ? "▼" : "▶"}</span>` +
      `<span class="diag-group-title">` +
      `<span class="diag-group-reason">${escapeHtml(title)}</span>` +
      (meta ? `<span class="diag-group-meta">${escapeHtml(meta)}</span>` : "") +
      `</span>` +
      `<span class="diag-group-count ${countClass}">${count}</span>`;

    const body = document.createElement("div");
    body.className = "diag-group-body";

    let built = false;
    const ensureBuilt = () => {
      if (built) return;
      built = true;
      for (const e of entries) body.appendChild(renderDiagEntry(e));
    };

    if (open) ensureBuilt();

    header.addEventListener("click", () => {
      const willOpen = !group.classList.contains("open");
      if (willOpen) ensureBuilt();
      group.classList.toggle("open", willOpen);
      header.querySelector(".twisty").textContent = willOpen ? "▼" : "▶";
    });

    group.appendChild(header);
    group.appendChild(body);
    return group;
  }

  function renderDiagnostics() {
    if (!currentMap) return;
    const root = els.diagnostics;
    root.replaceChildren();

    const summary = currentMap.summary || {};
    const r = diagReconcile;

    const banner = document.createElement("div");
    if (r && r.match) {
      banner.className = "diag-banner ok";
      banner.textContent =
        `Walked ${r.walkedConflicts} conflict` +
        (r.walkedConflicts === 1 ? "" : "s") +
        ` and ${r.walkedUnresolved} unresolved — matches MapSummary.`;
    } else if (r) {
      banner.className = "diag-banner mismatch";
      banner.innerHTML =
        `<strong>Count mismatch</strong> — walked ` +
        `<strong>${r.walkedConflicts}</strong> conflict / ` +
        `<strong>${r.walkedUnresolved}</strong> unresolved, but MapSummary says ` +
        `<strong>${r.summaryConflicts}</strong> / ` +
        `<strong>${r.summaryUnresolved}</strong>. ` +
        `Either the walk or the summary is wrong; do not trust either silently.`;
    } else {
      banner.className = "diag-banner";
      banner.textContent = "No reconciliation data.";
    }
    root.appendChild(banner);

    const dropped = document.createElement("div");
    dropped.className = "diag-dropped-note";
    dropped.innerHTML =
      `<strong>Not listed (deliberate drops):</strong> ` +
      `<span class="drop-count">${summary.external_dropped ?? 0}</span> external, ` +
      `<span class="drop-count">${summary.constructor_dropped ?? 0}</span> constructor, ` +
      `<span class="drop-count">${summary.associated_dropped ?? 0}</span> associated. ` +
      `These sites are excluded from the tree on purpose — their absence here is not a gap in this worklist.`;
    root.appendChild(dropped);

    const entries = filteredDiagEntries();
    const conflictN = entries.filter((e) => e.kind === "conflict").length;
    const unresolvedN = entries.filter((e) => e.kind === "unresolved").length;
    els.diagCount.textContent =
      `${entries.length} sites` +
      ` · ${conflictN} conflict` +
      (conflictN === 1 ? "" : "s") +
      ` · ${unresolvedN} unresolved` +
      (sourceLabel ? ` · ${sourceLabel}` : "");

    if (!entries.length) {
      const empty = document.createElement("p");
      empty.className = "diag-empty";
      empty.textContent =
        diagEntries.length === 0
          ? "No conflicts or unresolved call sites in this map."
          : "No sites match the current kind filter.";
      root.appendChild(empty);
      return;
    }

    const list = document.createElement("div");
    list.className = "diag-groups";

    if (diagLayoutMode() === "file") {
      const groups = groupByFile(entries);
      groups.forEach((g, i) => {
        list.appendChild(
          renderDiagGroup({
            title: basename(g.filePath) || g.filePath,
            meta: g.filePath,
            count: g.entries.length,
            countClass: countClassForKinds(g.kinds),
            entries: g.entries,
            open: i === 0,
          })
        );
      });
    } else {
      const groups = groupByReason(entries);
      groups.forEach((g, i) => {
        const kindMeta =
          g.kinds.size === 1
            ? [...g.kinds][0]
            : "conflict + unresolved";
        list.appendChild(
          renderDiagGroup({
            title: g.reason,
            meta: kindMeta,
            count: g.entries.length,
            countClass: countClassForKinds(g.kinds),
            entries: g.entries,
            open: i === 0,
          })
        );
      });
    }

    root.appendChild(list);
  }

  function setActiveView(view) {
    activeView = view;
    const isTree = view === "tree";

    for (const btn of els.viewSwitch.querySelectorAll(".view-btn")) {
      const on = btn.getAttribute("data-view") === view;
      btn.classList.toggle("active", on);
      btn.setAttribute("aria-selected", on ? "true" : "false");
    }

    els.treeControls.hidden = !isTree;
    els.diagControls.hidden = isTree;
    els.tree.hidden = !isTree || !currentMap;
    els.diagnostics.hidden = isTree || !currentMap;

    if (!isTree && currentMap) {
      renderDiagnostics();
    }
  }

  function showMapError(message) {
    currentMap = null;
    diagEntries = [];
    diagReconcile = null;
    els.empty.hidden = false;
    els.empty.textContent = message;
    els.tree.hidden = true;
    els.tree.replaceChildren();
    els.diagnostics.hidden = true;
    els.diagnostics.replaceChildren();
    els.summaryBar.hidden = true;
    els.toolbar.hidden = true;
    els.repoRoot.textContent = "";
    els.stats.innerHTML = "";
    els.matchCount.textContent = "";
    els.diagCount.textContent = "";
    els.search.value = "";
    els.filterConflicts.checked = false;
    els.filterUnresolved.checked = false;
    idIndex = new Map();
    fileNodeByPath = new Map();
    sourceLabel = "";
    document.title = "Horizon Map Viewer";
    activeView = "tree";
  }

  function loadMap(map, label) {
    if (!map || typeof map !== "object" || !Array.isArray(map.crates)) {
      showMapError(
        "Invalid Horizon map: expected a Repository object with crates[]."
      );
      return;
    }

    currentMap = map;
    buildIndex(map);
    const collected = collectDiagnostics(map);
    diagEntries = collected.entries;
    diagReconcile = collected.reconcile;

    const fnCount = countFunctions(map);
    sourceLabel = label || "";

    els.repoRoot.textContent = map.root || "(no root)";
    renderStats(map.summary || {});
    els.summaryBar.hidden = false;
    els.toolbar.hidden = false;
    els.empty.hidden = true;
    els.tree.replaceChildren();
    els.diagnostics.replaceChildren();
    fileNodeByPath = new Map();

    for (const crate of map.crates) {
      els.tree.appendChild(buildCrateNode(crate));
    }

    document.title = `Horizon — ${basename(map.root || "map")}`;
    els.matchCount.textContent =
      `${fnCount} functions` + (sourceLabel ? ` · ${sourceLabel}` : "");
    applyFilters();
    setActiveView(activeView);
  }

  // Event wiring
  els.tree.addEventListener("click", (ev) => {
    const a = ev.target.closest("a[data-jump-id]");
    if (!a) return;
    ev.preventDefault();
    ev.stopPropagation();
    jumpToFunction(a.getAttribute("data-jump-id"));
  });

  els.search.addEventListener("input", applyFilters);
  els.filterConflicts.addEventListener("change", applyFilters);
  els.filterUnresolved.addEventListener("change", applyFilters);

  for (const btn of els.viewSwitch.querySelectorAll(".view-btn")) {
    btn.addEventListener("click", () => {
      setActiveView(btn.getAttribute("data-view"));
    });
  }

  for (const el of [
    els.diagLayoutGrouped,
    els.diagLayoutFile,
    els.diagKindAll,
    els.diagKindConflict,
    els.diagKindUnresolved,
  ]) {
    el.addEventListener("change", () => {
      if (activeView === "diagnostics") renderDiagnostics();
    });
  }

  els.fileInput.addEventListener("change", () => {
    const file = els.fileInput.files && els.fileInput.files[0];
    if (!file) return;
    const reader = new FileReader();
    reader.onload = async () => {
      try {
        const text = String(reader.result);
        const map = JSON.parse(text);
        // Mirror onto the server so /api/source can validate File.path membership.
        try {
          await fetch("/api/map", {
            method: "POST",
            headers: { "Content-Type": "application/json" },
            body: text,
          });
        } catch (_) {
          /* source panel will fail honestly if the POST did not land */
        }
        loadMap(map, file.name);
      } catch (err) {
        showMapError(`Failed to parse ${file.name}: ${err.message}`);
      }
    };
    reader.onerror = () => {
      showMapError(`Failed to read ${file.name}.`);
    };
    reader.readAsText(file);
  });

  function applyQueryPrefs() {
    try {
      const q = new URLSearchParams(location.search);
      const v = q.get("view");
      if (v === "diagnostics" || v === "tree") activeView = v;
      const layout = q.get("layout");
      if (layout === "file") {
        els.diagLayoutFile.checked = true;
      } else if (layout === "grouped") {
        els.diagLayoutGrouped.checked = true;
      }
      const kind = q.get("kind");
      if (kind === "conflict") els.diagKindConflict.checked = true;
      else if (kind === "unresolved") els.diagKindUnresolved.checked = true;
      else if (kind === "all") els.diagKindAll.checked = true;
    } catch (_) {
      /* ignore */
    }
  }

  async function boot() {
    applyQueryPrefs();
    try {
      const res = await fetch("/api/map");
      if (res.ok) {
        const map = await res.json();
        loadMap(map, "startup");
        return;
      }
    } catch (_) {
      // Fall through to empty-state help.
    }
    els.empty.textContent =
      "No map loaded. Open a Horizon JSON file, or start the server with --map <file>.";
  }

  boot();
})();
