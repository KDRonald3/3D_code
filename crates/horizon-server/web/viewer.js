(() => {
  "use strict";

  const els = {
    summaryBar: document.getElementById("summary-bar"),
    repoRoot: document.getElementById("repo-root"),
    stats: document.getElementById("stats"),
    toolbar: document.getElementById("toolbar"),
    search: document.getElementById("search"),
    filterConflicts: document.getElementById("filter-conflicts"),
    filterUnresolved: document.getElementById("filter-unresolved"),
    matchCount: document.getElementById("match-count"),
    empty: document.getElementById("empty-state"),
    tree: document.getElementById("tree"),
    fileInput: document.getElementById("file-input"),
  };

  /** @type {Map<string, {fn: object, file: object, filePath: string, el: HTMLElement|null}>} */
  let idIndex = new Map();
  /** @type {WeakMap<HTMLElement, object>} */
  let nodeData = new WeakMap();
  /** Absolute file path → file node element (filled when the file node is built). */
  let fileNodeByPath = new Map();
  let filterState = { text: "", conflicts: false, unresolved: false };
  let sourceLabel = "";

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
    // Crates and folders start open and lazy-build on first ensureBuilt.
    // Force-build the containment spine so the target file node exists.
    const containers = els.tree.querySelectorAll(
      ".node.kind-crate, .node.kind-folder"
    );
    for (const n of containers) setOpen(n, true);

    let fileNode = fileNodeByPath.get(filePath);
    if (!fileNode) {
      // Fallback: locate by stored path on node data (e.g. after rebuild).
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
    if (row) {
      row.classList.add("highlight");
      row.scrollIntoView({ behavior: "smooth", block: "center" });
      setTimeout(() => row.classList.remove("highlight"), 1600);
    }
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

  function showMapError(message) {
    els.empty.hidden = false;
    els.empty.textContent = message;
    els.tree.hidden = true;
    els.tree.replaceChildren();
    els.summaryBar.hidden = true;
    els.toolbar.hidden = true;
    els.repoRoot.textContent = "";
    els.stats.innerHTML = "";
    els.matchCount.textContent = "";
    els.search.value = "";
    els.filterConflicts.checked = false;
    els.filterUnresolved.checked = false;
    idIndex = new Map();
    fileNodeByPath = new Map();
    sourceLabel = "";
    document.title = "Horizon Map Viewer";
  }

  function loadMap(map, label) {
    if (!map || typeof map !== "object" || !Array.isArray(map.crates)) {
      showMapError(
        "Invalid Horizon map: expected a Repository object with crates[]."
      );
      return;
    }

    buildIndex(map);
    const fnCount = countFunctions(map);
    sourceLabel = label || "";

    els.repoRoot.textContent = map.root || "(no root)";
    renderStats(map.summary || {});
    els.summaryBar.hidden = false;
    els.toolbar.hidden = false;
    els.empty.hidden = true;
    els.tree.hidden = false;
    els.tree.replaceChildren();
    fileNodeByPath = new Map();

    for (const crate of map.crates) {
      els.tree.appendChild(buildCrateNode(crate));
    }

    document.title = `Horizon — ${basename(map.root || "map")}`;
    els.matchCount.textContent =
      `${fnCount} functions` + (sourceLabel ? ` · ${sourceLabel}` : "");
    applyFilters();
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

  els.fileInput.addEventListener("change", () => {
    const file = els.fileInput.files && els.fileInput.files[0];
    if (!file) return;
    const reader = new FileReader();
    reader.onload = () => {
      try {
        const map = JSON.parse(String(reader.result));
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

  async function boot() {
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
