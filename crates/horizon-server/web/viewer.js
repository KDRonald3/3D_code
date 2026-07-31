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
  // Sidebar sizes — Desktop defaults/mins (index.dc.html). No artificial max:
  // panels grow with the pointer up to the opposite panel / window edge.
  // Canvas may shrink to zero; that is intentional.
  const LEFT_DEFAULT = 236;
  const LEFT_MIN = 180;
  const RIGHT_DEFAULT = 316;
  const RIGHT_MIN = 240;
  /** Bottom dock — Desktop default ~248px; grab-friendly minimum. */
  const BOTTOM_DEFAULT = 248;
  const BOTTOM_MIN = 120;
  /** Floor for #fns-viewport / .diag-body so rail squeeze cannot collapse them. */
  const FNS_VIEWPORT_MIN = 48;
  /*
   * Dock chrome width → progressive label compaction. Full Functions chrome
   * overflow began near 360px centre-column; compact by 420px (margin). Tight
   * shortens tab labels so tabs + ✕ stay hittable near 180px centre column.
   */
  const BOTTOM_CHROME_COMPACT_PX = 420;
  const BOTTOM_CHROME_TIGHT_PX = 300;

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
    openFolder: document.getElementById("open-folder"),
    analyseForm: document.getElementById("analyse-form"),
    analysePath: document.getElementById("analyse-path"),
    analyseRun: document.getElementById("analyse-run"),
    analyseCancelForm: document.getElementById("analyse-cancel-form"),
    analyseOverlay: document.getElementById("analyse-overlay"),
    analyseOverlayPath: document.getElementById("analyse-overlay-path"),
    analyseOverlayElapsed: document.getElementById("analyse-overlay-elapsed"),
    importError: document.getElementById("import-error"),
    importErrorText: document.getElementById("import-error-text"),
    clearError: document.getElementById("clear-error"),
    recentBlock: document.getElementById("recent-block"),
    recentList: document.getElementById("recent-list"),
    pageMap: document.getElementById("page-map"),
    pageDiff: document.getElementById("page-diff"),
    pageFns: document.getElementById("page-fns"),
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
    centerCol: document.querySelector(".center-col"),
    bottomPanel: document.getElementById("bottom-panel"),
    bottomRail: document.getElementById("bottom-rail"),
    bottomClose: document.getElementById("bottom-close"),
    diagPane: document.getElementById("diag-pane"),
    diagBody: document.getElementById("diag-body"),
    diagBanner: document.getElementById("diag-banner"),
    diagGroupMode: document.getElementById("diag-group-mode"),
    fnsPane: document.getElementById("fns-pane"),
    fnsBanner: document.getElementById("fns-banner"),
    fnsViewport: document.getElementById("fns-viewport"),
    fnsWorld: document.getElementById("fns-world"),
    fnsEdgesSvg: document.getElementById("fns-edges-svg"),
    fnsEdgePaths: document.getElementById("fns-edge-paths"),
    fnsNodes: document.getElementById("fns-nodes"),
    fnsControls: document.getElementById("fns-controls"),
    fnsDepthMode: document.getElementById("fns-depth-mode"),
    fnsFit: document.getElementById("fns-fit"),
    fnsZoomHud: document.getElementById("fns-zoom-hud"),
    fnsZoomOut: document.getElementById("fns-zoom-out"),
    fnsZoomIn: document.getElementById("fns-zoom-in"),
    fnsZoomReset: document.getElementById("fns-zoom-reset"),
    tabFunctions: document.getElementById("tab-functions"),
    pageDock: document.getElementById("page-dock"),
    pageDockBadge: document.getElementById("page-dock-badge"),
    tabDiagnostics: document.getElementById("tab-diagnostics"),
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
  /**
   * Home widths — the size each rail returns to after the opposite's conquest.
   * Updated only when that rail is manually dragged (or set via API as manual).
   * Passive squeeze/restore by the aggressor never writes these.
   */
  let leftHomeW = LEFT_DEFAULT;
  let rightHomeW = RIGHT_DEFAULT;
  /** Prior `--left-occupied` for pan compensation; null until first publish. */
  let lastLeftOccupied = null;
  /**
   * Prior `--bottom-occupied` while the dock is open; null until first publish.
   * Used so growing/shrinking the dock (viewport top moves) does not jump the
   * Functions DAG — same idea as left-occupied → panX for the map.
   */
  let lastBottomOccupied = null;
  let bottomOpen = false;
  let bottomH = BOTTOM_DEFAULT;
  let bottomHomeH = BOTTOM_DEFAULT;
  /** Last measured .bottom-chrome width (px); 0 until observed. */
  let bottomChromeWidth = 0;
  /** True when chrome width ≤ BOTTOM_CHROME_COMPACT_PX (short control labels). */
  let bottomChromeCompact = false;
  /** True when chrome width ≤ BOTTOM_CHROME_TIGHT_PX (short tab labels too). */
  let bottomChromeTight = false;
  /**
   * Bottom dock sits *below* the canvas, so growing it shortens the viewport
   * from the bottom — canvas top (origin Y) does not move. Unlike left-occupied
   * → panX, bottom-occupied needs no panY compensation (same as right rail).
   */
  let diagGroupMode = "reason"; // "reason" | "file"
  /** @type {"diag"|"fns"} */
  let bottomTab = "diag";
  /** @type {object[]} */
  let diagEntries = [];
  let diagReconcile = null;
  /** @type {object|null} */
  let diagFocus = null;
  /** @type {Set<string>} */
  let diagCollapsed = new Set();
  /** @type {object|null} last built function DAG (for verification). */
  let fnsGraph = null;
  /**
   * Neighborhood depth for the Functions DAG: 1 | 2 | 'all'.
   * Reset (via auto default) when the selected file changes so a busy file
   * opens readable and a small one still shows everything.
   * @type {1|2|'all'}
   */
  let fnsDepth = "all";
  /** File id the current `fnsDepth` was chosen for — null until first build. */
  let fnsDepthFileId = null;
  /** @type {object|null} last full (unfiltered) DAG build for depth meta. */
  let fnsFullGraph = null;
  /** Dock Functions canvas transform — fully independent of map zoom/pan. */
  let fnsZoom = 1;
  let fnsPanX = 0;
  let fnsPanY = 0;
  let fnsWorldW = 320;
  let fnsWorldH = 160;
  let fnsPanDrag = null;
  /**
   * Set when a dock pan gesture exceeded DRAG_MOVE. Consumed by fns-edge
   * click handlers so a pan starting on an edge does not activate it.
   * Node presses never pan (see resolveFnsPointerGesture) so they never set this.
   */
  let suppressFnsClick = false;
  /**
   * Hand-placed Functions-node positions in dock world units. Survive re-layout
   * (depth change / neighborhood slice); Fit must not clear these.
   * @type {Map<string, {x:number,y:number}>}
   */
  let fnsNodePos = new Map();
  /** @type {Map<string, HTMLElement>} */
  let fnsNodeEls = new Map();
  let fnsNodeDrag = null;
  /**
   * Set on Functions-node pointerup when the gesture exceeded DRAG_MOVE.
   * Consumed by the subsequent click so a rearrange does not select / open
   * the Inspector. Cleared on the next click handler run.
   */
  let suppressFnsNodeClick = false;
  /** Last Functions-node pointer gesture — for hand-drag verification. */
  let lastFnsNodeGesture = null;
  /** @type {object|null} last Functions-node activation (stub reason / selection). */
  let lastFnsNodeActivation = null;
  /** @type {string|null} */
  let fnsActiveEdgeId = null;
  /** Last boot outcome for diagnostics (null while in flight). */
  let bootStatus = null;
  let dark = false;
  let userSetTheme = false;
  let filters = { entry: true, file: true, fn: true, struct: true };
  let query = "";
  let worldW = 1360;
  let worldH = 600;
  /** Bumps on each source fetch so stale responses are ignored. */
  let sourceFetchGen = 0;

  // Pan / drag state
  let panDrag = null;
  let cardDrag = null;
  /**
   * Set on card pointerup when the gesture exceeded DRAG_MOVE. Consumed by the
   * subsequent click so a rearrange does not count as a selection (and therefore
   * does not open the Inspector). Cleared on the next click handler run.
   */
  let suppressCardClick = false;
  /** Last card pointer gesture — for hand-drag verification in the browser. */
  let lastCardGesture = null;

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
   *   typeCount: number,
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

  const THEME_KEY = "horizon.theme";
  const RECENT_KEY = "horizon.recent";
  const RECENT_MAX = 5;
  /** Skip storing maps larger than this — localStorage is ~5 MiB total. */
  const RECENT_MAX_BYTES = 1_500_000;

  function applyTheme() {
    els.app.dataset.theme = dark ? "dark" : "light";
    if (els.toggleTheme) {
      els.toggleTheme.textContent = dark ? "☀" : "☾";
      els.toggleTheme.title = dark ? "Switch to light theme" : "Switch to dark theme";
    }
  }

  function persistTheme() {
    try {
      localStorage.setItem(THEME_KEY, dark ? "dark" : "light");
    } catch (_) {
      /* private mode / quota — theme still works for the session */
    }
  }

  function readRecent() {
    try {
      const raw = localStorage.getItem(RECENT_KEY);
      if (!raw) return [];
      const list = JSON.parse(raw);
      return Array.isArray(list) ? list : [];
    } catch (_) {
      return [];
    }
  }

  function writeRecent(list) {
    try {
      localStorage.setItem(RECENT_KEY, JSON.stringify(list));
    } catch (err) {
      // Quota — drop oldest until it fits, then give up silently.
      let trimmed = list.slice();
      while (trimmed.length > 1) {
        trimmed = trimmed.slice(0, -1);
        try {
          localStorage.setItem(RECENT_KEY, JSON.stringify(trimmed));
          return;
        } catch (_) {
          /* keep trimming */
        }
      }
    }
  }

  /**
   * Remember a loaded map for the import Recent list. Stores the Repository
   * JSON itself (honest restore — no re-analyse). Oversized maps are skipped
   * rather than corrupting localStorage.
   */
  function rememberRecent(map, label) {
    if (!map || typeof map !== "object") return;
    let json;
    try {
      json = JSON.stringify(map);
    } catch (_) {
      return;
    }
    if (json.length > RECENT_MAX_BYTES) return;
    const root = map.root != null ? String(map.root) : "";
    const id = root || label || `map-${Date.now()}`;
    const entry = {
      id,
      label: label || basename(root) || "Untitled map",
      root,
      savedAt: Date.now(),
      summary: map.summary || null,
      map,
    };
    const prev = readRecent().filter((e) => e && e.id !== id);
    writeRecent([entry, ...prev].slice(0, RECENT_MAX));
    renderRecent();
  }

  function renderRecent() {
    const list = els.recentList;
    const block = els.recentBlock;
    if (!list || !block) return;
    const items = readRecent().filter((e) => e && e.map && typeof e.map === "object");
    list.replaceChildren();
    if (!items.length) {
      block.hidden = true;
      return;
    }
    block.hidden = false;
    for (const item of items) {
      const btn = document.createElement("button");
      btn.type = "button";
      btn.className = "recent-chip";
      btn.title = item.root || item.label;
      const label = document.createElement("span");
      label.className = "recent-chip-label";
      label.textContent = item.label || "Untitled map";
      const meta = document.createElement("span");
      meta.className = "recent-chip-meta";
      const s = item.summary || item.map.summary || {};
      const unresolved = s.unresolved ?? 0;
      const conflicts = s.conflicts ?? 0;
      meta.textContent =
        conflicts || unresolved
          ? `${conflicts} conflict · ${unresolved} unresolved`
          : "clean";
      btn.appendChild(label);
      btn.appendChild(meta);
      btn.addEventListener("click", async () => {
        try {
          const text = JSON.stringify(item.map);
          try {
            await fetch("/api/map", {
              method: "POST",
              headers: { "Content-Type": "application/json" },
              body: text,
            });
          } catch (_) {
            /* canvas can still load without server store */
          }
          loadMap(item.map, item.label || "recent");
        } catch (err) {
          showImport(`Failed to restore recent map: ${err.message || err}`);
        }
      });
      list.appendChild(btn);
    }
  }

  /** Pages rail active state — Map, or the open dock tab. Diff stays inert. */
  function syncPagesActive() {
    const page =
      bottomOpen && bottomTab === "fns"
        ? "fns"
        : bottomOpen && bottomTab === "diag"
          ? "diag"
          : "map";
    document.querySelectorAll(".page-btn[data-page]").forEach((btn) => {
      if (btn.disabled) {
        btn.classList.remove("active");
        return;
      }
      btn.classList.toggle("active", btn.dataset.page === page);
    });
  }

  function workspaceWidth() {
    const ws = els.leftAside?.parentElement;
    if (!ws) return window.innerWidth || 1200;
    return ws.getBoundingClientRect().width || window.innerWidth || 1200;
  }

  function canvasWidth() {
    const W = workspaceWidth();
    const L = leftOpen ? leftW : 0;
    const R = rightOpen ? rightW : 0;
    return Math.max(0, W - L - R);
  }

  /** Hard max for a rail: workspace minus the opposite rail's minimum. */
  function maxLeftWidth() {
    const otherMin = rightOpen ? RIGHT_MIN : 0;
    return Math.max(LEFT_MIN, workspaceWidth() - otherMin);
  }

  function maxRightWidth() {
    const otherMin = leftOpen ? LEFT_MIN : 0;
    return Math.max(RIGHT_MIN, workspaceWidth() - otherMin);
  }

  /** Occupied left inset — what actually shifts the canvas origin. */
  function leftOccupiedPx() {
    return leftOpen ? leftW : 0;
  }

  function rightOccupiedPx() {
    return rightOpen ? rightW : 0;
  }

  /**
   * Screen X of a world point on the main map canvas.
   *
   * Transform is `translate(panXpx, panYpx) scale(zoom)`. CSS applies the
   * list right-to-left to points, so screenX = canvasLeft + panX + worldX*zoom.
   * Thus panX is already in screen pixels — compensation is panX -= Δ with
   * no zoom factor. Mirrored in `rail_layout::screen_x`.
   */
  function screenXOfWorld(canvasLeft, worldX) {
    return canvasLeft + panX + worldX * zoom;
  }

  /**
   * Screen X of a Functions-DAG world point inside the dock viewport.
   * Same transform convention as the map — pan is in screen pixels.
   */
  function screenXOfFnsWorld(viewportLeft, worldX) {
    return viewportLeft + fnsPanX + worldX * fnsZoom;
  }

  /**
   * Screen Y of a Functions-DAG world point inside the dock viewport.
   */
  function screenYOfFnsWorld(viewportTop, worldY) {
    return viewportTop + fnsPanY + worldY * fnsZoom;
  }

  /**
   * Publish the single authoritative width. Panel size, rail position, and
   * centre margins all read these CSS variables — nothing else tracks width.
   *
   * Every change to left-occupied (direct left drag, passive squeeze/restore
   * by the right aggressor, left toggle collapse/expand) funnels through here.
   * When left-occupied changes by Δ, panX -= Δ so map content stays put on
   * screen (cards may clip under the panel — intentional). The dock Functions
   * canvas shares the same centre-col left edge, so fnsPanX gets the same Δ
   * compensation — otherwise the DAG would slide when Layers is resized.
   * Right-occupied changes do not move the canvas/dock origin (they shrink
   * from the right); when the right aggressor squeezes left, that left Δ is
   * still handled here.
   */
  function publishRailVars() {
    const leftOcc = leftOccupiedPx();
    const rightOcc = rightOccupiedPx();
    if (lastLeftOccupied != null && leftOcc !== lastLeftOccupied) {
      // Screen-pixel compensation — do not divide/multiply by zoom.
      const delta = leftOcc - lastLeftOccupied;
      panX -= delta;
      if (!Number.isFinite(panX)) panX = 0;
      if (els.world) updateWorldTransform();
      // Dock shares centre-col's left margin — same Δ, independent transform.
      fnsPanX -= delta;
      if (!Number.isFinite(fnsPanX)) fnsPanX = 0;
      if (els.fnsWorld) updateFnsWorldTransform();
    }
    lastLeftOccupied = leftOcc;
    els.app.style.setProperty("--left-w", `${leftW}px`);
    els.app.style.setProperty("--right-w", `${rightW}px`);
    els.app.style.setProperty("--left-occupied", `${leftOcc}px`);
    els.app.style.setProperty("--right-occupied", `${rightOcc}px`);
    // Clear any stale inline overrides — position comes from CSS + vars only.
    els.leftRail.style.left = "";
    els.rightRail.style.right = "";
  }

  /**
   * Pure right-aggressor layout. Keep in lockstep with
   * `horizon_server::rail_layout::compute_right_aggressor` (unit-tested).
   * Consumes canvas first; past canvas-zero squeezes left toward LEFT_MIN.
   * Shrinking restores left to leftHome before canvas gains pixels.
   * Does not modify homes.
   * @returns {{ left: number, right: number }}
   */
  function computeRightAggressorLayout(W, desiredRight, leftIsOpen, leftHome) {
    const Lmin = leftIsOpen ? LEFT_MIN : 0;
    const desired = Number(desiredRight);
    const R = Math.max(
      RIGHT_MIN,
      Math.min(W - Lmin, Number.isFinite(desired) ? desired : RIGHT_MIN)
    );
    if (!leftIsOpen) return { left: 0, right: R };
    const Lhome = Math.max(LEFT_MIN, leftHome);
    if (Lhome + R <= W) return { left: Lhome, right: R };
    return { left: Math.max(Lmin, W - R), right: R };
  }

  /**
   * Pure left-aggressor layout. Keep in lockstep with
   * `horizon_server::rail_layout::compute_left_aggressor`.
   * @returns {{ left: number, right: number }}
   */
  function computeLeftAggressorLayout(W, desiredLeft, rightIsOpen, rightHome) {
    const Rmin = rightIsOpen ? RIGHT_MIN : 0;
    const desired = Number(desiredLeft);
    const L = Math.max(
      LEFT_MIN,
      Math.min(W - Rmin, Number.isFinite(desired) ? desired : LEFT_MIN)
    );
    if (!rightIsOpen) return { left: L, right: 0 };
    const Rhome = Math.max(RIGHT_MIN, rightHome);
    if (L + Rhome <= W) return { left: L, right: Rhome };
    return { left: L, right: Math.max(Rmin, W - L) };
  }

  /**
   * Prove JS rail arithmetic matches the shared fixture table (same file the
   * Rust oracle tests). Fetches `/static/rail_layout_cases.json`.
   * @returns {Promise<{ok:boolean,total:number,fails:object[]}>}
   */
  async function runRailFixtureTable() {
    const res = await fetch("/static/rail_layout_cases.json");
    if (!res.ok) {
      return {
        ok: false,
        total: 0,
        fails: [{ name: "fetch", error: `HTTP ${res.status}` }],
      };
    }
    const doc = await res.json();
    if (
      Math.abs(Number(doc.left_min) - LEFT_MIN) > 1e-9 ||
      Math.abs(Number(doc.right_min) - RIGHT_MIN) > 1e-9
    ) {
      return {
        ok: false,
        total: 0,
        fails: [
          {
            name: "mins",
            error: `fixture mins ${doc.left_min}/${doc.right_min} ≠ ${LEFT_MIN}/${RIGHT_MIN}`,
          },
        ],
      };
    }
    const fails = [];
    for (const c of doc.cases || []) {
      const got =
        c.side === "right"
          ? computeRightAggressorLayout(
              c.workspace,
              c.desired,
              c.other_open,
              c.other_home
            )
          : computeLeftAggressorLayout(
              c.workspace,
              c.desired,
              c.other_open,
              c.other_home
            );
      if (
        Math.abs(got.left - c.expect_left) > 1e-9 ||
        Math.abs(got.right - c.expect_right) > 1e-9
      ) {
        fails.push({
          name: c.name,
          got,
          expect: { left: c.expect_left, right: c.expect_right },
        });
      }
    }
    return { ok: fails.length === 0, total: (doc.cases || []).length, fails };
  }

  /** Apply pure right-aggressor result; does not modify homes. */
  function layoutRightAggressor(desiredRight) {
    const next = computeRightAggressorLayout(
      workspaceWidth(),
      desiredRight,
      leftOpen,
      leftHomeW
    );
    if (leftOpen) leftW = next.left;
    rightW = next.right;
    publishRailVars();
  }

  /** Apply pure left-aggressor result; does not modify homes. */
  function layoutLeftAggressor(desiredLeft) {
    const next = computeLeftAggressorLayout(
      workspaceWidth(),
      desiredLeft,
      rightOpen,
      rightHomeW
    );
    leftW = next.left;
    if (rightOpen) rightW = next.right;
    publishRailVars();
  }

  /** Manual left resize — updates leftHomeW (invalidates prior squeeze memory). */
  function setLeftWidth(w) {
    layoutLeftAggressor(w);
    leftHomeW = leftW;
  }

  /** Manual right resize — updates rightHomeW. */
  function setRightWidth(w) {
    layoutRightAggressor(w);
    rightHomeW = rightW;
  }

  function setRightOpen(open) {
    rightOpen = open;
    els.rightAside.hidden = false;
    els.rightAside.classList.toggle("collapsed", !rightOpen);
    els.rightRail.hidden = !rightOpen;
    // Re-apply aggressor layout from current homes so occupied space is honest.
    if (rightOpen) layoutRightAggressor(rightHomeW);
    else if (leftOpen) layoutLeftAggressor(leftHomeW);
    else publishRailVars();
  }

  function setLeftOpen(open) {
    leftOpen = open;
    els.leftAside.classList.toggle("collapsed", !leftOpen);
    els.leftRail.hidden = !leftOpen;
    if (leftOpen) layoutLeftAggressor(leftHomeW);
    else if (rightOpen) layoutRightAggressor(rightHomeW);
    else publishRailVars();
  }

  /** Window chrome changed size — fit rails into the new workspace, no map refit. */
  function enforceWorkspaceFit() {
    const W = workspaceWidth();
    const Lmin = leftOpen ? LEFT_MIN : 0;
    const Rmin = rightOpen ? RIGHT_MIN : 0;
    const Lh = leftOpen ? Math.max(LEFT_MIN, leftHomeW) : 0;
    const Rh = rightOpen ? Math.max(RIGHT_MIN, rightHomeW) : 0;
    if (Lh + Rh <= W) {
      if (leftOpen) leftW = Lh;
      if (rightOpen) rightW = Rh;
      publishRailVars();
    } else if (leftOpen && rightOpen) {
      rightW = Math.max(Rmin, Math.min(Rh, W - Lmin));
      leftW = Math.max(Lmin, W - rightW);
      publishRailVars();
    } else if (leftOpen) {
      leftW = Math.min(Math.max(LEFT_MIN, leftW), W);
      publishRailVars();
    } else if (rightOpen) {
      rightW = Math.min(Math.max(RIGHT_MIN, rightW), W);
      publishRailVars();
    }
    if (bottomOpen) layoutBottomHeight(bottomH);
  }

  function centerColHeight() {
    const col = els.centerCol;
    if (!col) return window.innerHeight || 800;
    return col.getBoundingClientRect().height || window.innerHeight || 800;
  }

  function publishBottomVars() {
    const occ = bottomOpen ? bottomH : 0;
    // Map canvas top does not move when the dock grows — no map panY change.
    // The dock viewport top *does* move (panel grows upward). Compensate so
    // the Functions DAG stays under the same screen pixels. Skip open/close
    // transitions (0 ↔ N) — those are show/hide, not a continuous resize.
    if (
      lastBottomOccupied != null &&
      lastBottomOccupied > 0 &&
      occ > 0 &&
      occ !== lastBottomOccupied
    ) {
      fnsPanY += occ - lastBottomOccupied;
      if (!Number.isFinite(fnsPanY)) fnsPanY = 0;
      if (els.fnsWorld) updateFnsWorldTransform();
    }
    lastBottomOccupied = occ;
    els.app.style.setProperty("--bottom-h", `${bottomH}px`);
    els.app.style.setProperty("--bottom-occupied", `${occ}px`);
    if (els.bottomRail) els.bottomRail.style.bottom = "";
    syncBottomChromeCompact();
  }

  /**
   * Apply .is-compact / .is-tight on .bottom-chrome from its measured width.
   * Labels shorten; chrome height stays 40px (CSS-locked) — never wrap.
   */
  function syncBottomChromeCompact() {
    const chrome = els.bottomPanel
      ? els.bottomPanel.querySelector(".bottom-chrome")
      : null;
    if (!chrome) return;
    // Hidden panel has no useful width; keep prior classes until shown.
    if (els.bottomPanel.hidden) return;
    const w = chrome.getBoundingClientRect().width;
    if (!(w > 0)) return;
    bottomChromeWidth = w;
    bottomChromeCompact = w <= BOTTOM_CHROME_COMPACT_PX;
    bottomChromeTight = w <= BOTTOM_CHROME_TIGHT_PX;
    chrome.classList.toggle("is-compact", bottomChromeCompact);
    chrome.classList.toggle("is-tight", bottomChromeTight);
  }

  /**
   * Set bottom dock height. Canvas may shrink to zero; hard stop is the
   * center-column height. Map panY unchanged (canvas top fixed). Dock fnsPanY
   * is compensated inside publishBottomVars when height changes while open.
   */
  function layoutBottomHeight(desired) {
    const colH = Math.max(0, centerColHeight());
    // Canvas may shrink to zero; hard max is the full center column.
    const maxH = Math.max(BOTTOM_MIN, colH);
    const raw = Number(desired);
    const want = Number.isFinite(raw) ? raw : BOTTOM_DEFAULT;
    bottomH = Math.max(BOTTOM_MIN, Math.min(maxH, want));
    publishBottomVars();
  }

  function setBottomHeight(h) {
    layoutBottomHeight(h);
    bottomHomeH = bottomH;
  }

  function setBottomOpen(open) {
    bottomOpen = !!open;
    if (els.bottomPanel) {
      els.bottomPanel.hidden = !bottomOpen;
      els.bottomPanel.setAttribute("aria-hidden", bottomOpen ? "false" : "true");
    }
    if (els.bottomRail) els.bottomRail.hidden = !bottomOpen;
    if (bottomOpen) {
      layoutBottomHeight(bottomHomeH);
      applyBottomTab();
    } else {
      publishBottomVars();
    }
    syncPagesActive();
  }

  function setBottomTab(tab) {
    bottomTab = tab === "fns" ? "fns" : "diag";
    applyBottomTab();
    syncPagesActive();
  }

  function applyBottomTab() {
    const fns = bottomTab === "fns";
    if (els.fnsPane) els.fnsPane.hidden = !fns;
    if (els.diagPane) els.diagPane.hidden = fns;
    if (els.diagGroupMode) els.diagGroupMode.hidden = fns;
    if (els.fnsControls) els.fnsControls.hidden = !fns;
    if (els.tabFunctions) {
      els.tabFunctions.classList.toggle("active", fns);
      els.tabFunctions.setAttribute("aria-selected", fns ? "true" : "false");
    }
    if (els.tabDiagnostics) {
      els.tabDiagnostics.classList.toggle("active", !fns);
      els.tabDiagnostics.setAttribute("aria-selected", !fns ? "true" : "false");
    }
    if (!bottomOpen) return;
    if (fns) renderFunctionDag({ fit: !fnsGraph });
    else renderDiagnostics();
  }

  function beginRailDrag(railEl, ev) {
    ev.preventDefault();
    ev.stopPropagation();
    try {
      railEl.setPointerCapture(ev.pointerId);
    } catch (_) {
      /* capture unsupported — window listeners still work */
    }
    els.app.classList.add("resizing-rail");
  }

  function endRailDrag(railEl, ev) {
    els.app.classList.remove("resizing-rail");
    if (ev && railEl && ev.pointerId != null) {
      try {
        if (railEl.hasPointerCapture?.(ev.pointerId)) {
          railEl.releasePointerCapture(ev.pointerId);
        }
      } catch (_) {
        /* ignore */
      }
    }
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
          typeCount: (file.types || []).length,
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
   * Clamped to [ZOOM_MIN, ZOOM_FIT_MAX]. Survives a near-zero canvas (rails
   * dragged until they meet): never divides by zero or yields NaN.
   */
  function fitView() {
    const rect = els.canvas.getBoundingClientRect();
    const b = contentBounds();
    const contentW = Math.max(1, b.maxX - b.minX);
    const contentH = Math.max(1, b.maxY - b.minY);
    // When the canvas is narrower than the margin budget, still use ≥1px.
    const availW = Math.max(1, rect.width - Math.min(FIT_MARGIN * 2, rect.width * 0.5));
    const availH = Math.max(1, rect.height - Math.min(FIT_MARGIN * 2, rect.height * 0.5));
    let next = Math.min(availW / contentW, availH / contentH);
    if (!Number.isFinite(next) || next <= 0) next = ZOOM_MIN;
    zoom = Math.min(ZOOM_FIT_MAX, Math.max(ZOOM_MIN, +next.toFixed(3)));
    const cx = (b.minX + b.maxX) / 2;
    const cy = (b.minY + b.maxY) / 2;
    const vw = Math.max(0, rect.width);
    const vh = Math.max(0, rect.height);
    panX = vw / 2 - cx * zoom;
    panY = vh / 2 - cy * zoom;
    if (!Number.isFinite(panX)) panX = 0;
    if (!Number.isFinite(panY)) panY = 0;
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
    // Content chips: a file with functions matches Fns; a file with types
    // matches Types. Empty files (neither) stay visible so the map never
    // hides a card solely for lacking both. Turning a chip off dims files
    // whose only matching content is that kind.
    const hasFn = (node.fnCount || 0) > 0;
    const hasType = (node.typeCount || 0) > 0;
    if (hasFn || hasType) {
      const matchFn = hasFn && filters.fn;
      const matchType = hasType && filters.struct;
      if (!matchFn && !matchType) return false;
    }
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
        `<span class="card-fn-count">${node.fnCount} fn` +
        (node.typeCount
          ? ` · ${node.typeCount} type${node.typeCount === 1 ? "" : "s"}`
          : "") +
        `</span>` +
        `<div class="sel-handles">` +
        `<span class="sel-handle tl"></span>` +
        `<span class="sel-handle tr"></span>` +
        `<span class="sel-handle bl"></span>` +
        `<span class="sel-handle br"></span>` +
        (isSel
          ? `<span class="sel-pill">${node.fnCount} fn` +
            (node.typeCount ? ` · ${node.typeCount} ty` : "") +
            `</span>`
          : "") +
        `</div>` +
        `</div>`;

      const frame = wrap.querySelector(".card-frame");
      frame.addEventListener("pointerdown", (ev) => onCardPointerDown(ev, node.id));
      frame.addEventListener("click", (ev) => {
        ev.stopPropagation();
        // cardDrag is already cleared on pointerup — use suppressCardClick,
        // which remembers that this gesture was a rearrange, not a selection.
        if (suppressCardClick) {
          suppressCardClick = false;
          lastCardGesture = {
            ...(lastCardGesture || {}),
            id: node.id,
            clickSuppressed: true,
            selected: false,
            rightOpenAfter: rightOpen,
          };
          return;
        }
        selectFile(node.id, { reveal: false });
        lastCardGesture = {
          id: node.id,
          moved: false,
          clickSuppressed: false,
          selected: true,
          selectedId,
          rightOpenAfter: rightOpen,
        };
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
          span.textContent =
            `${node.fnCount} fn` +
            (node.typeCount ? ` · ${node.typeCount} ty` : "");
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
   * Single policy: may this intent open a collapsed Inspector?
   * Selection (file / function / diagnostics / history) → yes.
   * View manipulation (pan / card-drag / rail-resize / zoom) → never call this.
   */
  function openInspectorForSelection() {
    if (!rightOpen) setRightOpen(true);
  }

  /**
   * Select a file card. Clears function selection unless keepFn and the fn
   * still belongs to this file. Opens the Inspector (selection intent).
   */
  function selectFile(id, { reveal = false, push = false, keepFn = false } = {}) {
    if (push) pushHistory();
    selectedId = id;
    if (!keepFn) selectedFnId = null;
    if (reveal && id) revealCard(id);
    openInspectorForSelection();
    refreshFocus();
    renderInspector();
    if (bottomOpen && bottomTab === "fns") renderFunctionDag({ fit: true });
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
    openInspectorForSelection();
    refreshFocus();
    renderInspector();
    if (bottomOpen && bottomTab === "fns") renderFunctionDag({ fit: false });
    return true;
  }

  function goBack() {
    const prev = selHistory.pop();
    if (!prev) return;
    selectedId = prev.fileId;
    selectedFnId = prev.fnId;
    if (selectedId) revealCard(selectedId);
    openInspectorForSelection();
    refreshFocus();
    renderInspector();
    if (bottomOpen && bottomTab === "fns") renderFunctionDag({ fit: false });
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

  function siteMatchesDiagFocus(site) {
    if (!diagFocus) return false;
    if (diagFocus.byteStart && site.byte_start === diagFocus.byteStart) return true;
    return (
      site.line === diagFocus.line &&
      String(site.call_path || "") === String(diagFocus.callPath || "")
    );
  }

  function renderCallSites(sites) {
    const list = document.createElement("ul");
    list.className = "call-list";
    for (const site of sites || []) {
      const kind = site.target?.kind || "unknown";
      const li = document.createElement("li");
      const focus = siteMatchesDiagFocus(site);
      li.className = `call-site ${kind}` + (focus ? " diag-focus" : "");
      if (focus) li.dataset.diagFocus = "1";

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

  function scrollDiagFocusIntoInspector() {
    const el = els.inspector?.querySelector(".call-site.diag-focus");
    if (el && typeof el.scrollIntoView === "function") {
      el.scrollIntoView({ block: "nearest", behavior: "smooth" });
    }
  }

  // —— Diagnostics worklist (bottom dock) ——

  function rebuildDiagnostics() {
    const HD = window.HorizonDiagnostics;
    if (!HD || !currentMap) {
      diagEntries = [];
      diagReconcile = null;
      return;
    }
    const result = HD.collectDiagnostics(currentMap);
    diagEntries = result.entries || [];
    diagReconcile = result.reconcile || null;
    updateDockBadge();
  }

  function updateDockBadge() {
    const badge = els.pageDockBadge;
    if (!badge) return;
    const n = diagEntries.length;
    if (!n) {
      badge.hidden = true;
      badge.textContent = "";
      return;
    }
    badge.hidden = false;
    badge.textContent = String(n);
    const hasConflict = diagEntries.some((e) => e.kind === "conflict");
    badge.classList.toggle("conflict", hasConflict);
  }

  function openDiagnosticEntry(entry) {
    if (!entry) return;
    diagFocus = entry;
    // A diagnostics row is a selection — openInspectorForSelection runs via
    // selectFile / selectFunction. Do not force the bottom dock open.
    const fnId = entry.enclosing?.functionId;
    if (fnId && fnIndex.has(String(fnId))) {
      selectFunction(fnId, { reveal: true, push: true });
    } else {
      const node = fileNodes.find((n) => n.path === entry.filePath);
      if (node) selectFile(node.id, { reveal: true, push: true });
      else {
        openInspectorForSelection();
        renderInspector();
      }
    }
    requestAnimationFrame(() => scrollDiagFocusIntoInspector());
    if (bottomTab === "diag") renderDiagnostics();
  }

  function focusDagCallSite(edgeOrStub) {
    if (!edgeOrStub) return;
    const callerId = edgeOrStub.callerFnId;
    diagFocus = {
      kind: edgeOrStub.kind,
      callPath: edgeOrStub.callPath || edgeOrStub.name || "",
      line: edgeOrStub.line ?? 0,
      byteStart: edgeOrStub.byteStart ?? 0,
      byteEnd: edgeOrStub.byteEnd ?? 0,
      reason: edgeOrStub.reason || "",
      candidates: edgeOrStub.candidates || [],
      enclosing: callerId
        ? { type: "function", functionId: callerId }
        : null,
    };
    fnsActiveEdgeId = edgeOrStub.id || null;
    if (callerId && fnIndex.has(String(callerId))) {
      selectFunction(callerId, { reveal: true, push: true });
    } else {
      openInspectorForSelection();
      renderInspector();
      if (bottomOpen && bottomTab === "fns") renderFunctionDag({ fit: false });
    }
    requestAnimationFrame(() => scrollDiagFocusIntoInspector());
  }

  function updateFnsWorldTransform() {
    if (!els.fnsWorld) return;
    if (!Number.isFinite(fnsPanX)) fnsPanX = 0;
    if (!Number.isFinite(fnsPanY)) fnsPanY = 0;
    if (!Number.isFinite(fnsZoom) || fnsZoom <= 0) fnsZoom = ZOOM_MIN;
    els.fnsWorld.style.transform = `translate(${fnsPanX}px, ${fnsPanY}px) scale(${fnsZoom})`;
    if (els.fnsZoomReset) {
      els.fnsZoomReset.textContent = `${Math.round(fnsZoom * 100)}%`;
    }
  }

  /**
   * Cursor-anchored zoom for the dock viewport (same factor/clamps as the map).
   * @param {number} clientX
   * @param {number} clientY
   * @param {number} deltaY
   */
  function zoomFnsAt(clientX, clientY, deltaY) {
    const vp = els.fnsViewport;
    if (!vp) return;
    const rect = vp.getBoundingClientRect();
    const mx = clientX - rect.left;
    const my = clientY - rect.top;
    const before = fnsZoom > 0 && Number.isFinite(fnsZoom) ? fnsZoom : ZOOM_MIN;
    const factor = deltaY < 0 ? 1.08 : 1 / 1.08;
    fnsZoom = Math.max(ZOOM_MIN, Math.min(ZOOM_MAX, +(before * factor).toFixed(3)));
    fnsPanX = mx - (mx - fnsPanX) * (fnsZoom / before);
    fnsPanY = my - (my - fnsPanY) * (fnsZoom / before);
    updateFnsWorldTransform();
  }

  /**
   * Cursor-anchored zoom for the main map canvas.
   * @param {number} clientX
   * @param {number} clientY
   * @param {number} deltaY
   */
  function zoomMapAt(clientX, clientY, deltaY) {
    const rect = els.canvas.getBoundingClientRect();
    const mx = clientX - rect.left;
    const my = clientY - rect.top;
    const before = zoom > 0 && Number.isFinite(zoom) ? zoom : ZOOM_MIN;
    const factor = deltaY < 0 ? 1.08 : 1 / 1.08;
    zoom = Math.max(ZOOM_MIN, Math.min(ZOOM_MAX, +(before * factor).toFixed(3)));
    panX = mx - (mx - panX) * (zoom / before);
    panY = my - (my - panY) * (zoom / before);
    if (!Number.isFinite(panX)) panX = 0;
    if (!Number.isFinite(panY)) panY = 0;
    updateWorldTransform();
  }

  function fitFunctionDag() {
    const vp = els.fnsViewport;
    if (!vp) return;
    const rect = vp.getBoundingClientRect();
    const pad = 24;
    const zw = Math.max(1, fnsWorldW);
    const zh = Math.max(1, fnsWorldH);
    const availW = Math.max(1, rect.width - Math.min(pad * 2, rect.width * 0.5));
    const availH = Math.max(1, rect.height - Math.min(pad * 2, rect.height * 0.5));
    let next = Math.min(availW / zw, availH / zh);
    if (!Number.isFinite(next) || next <= 0) next = ZOOM_MIN;
    // Same fit clamp as the map — dense DAGs stay readable via manual zoom-out.
    fnsZoom = Math.min(ZOOM_FIT_MAX, Math.max(ZOOM_MIN, +next.toFixed(3)));
    fnsPanX = (rect.width - zw * fnsZoom) / 2;
    fnsPanY = (rect.height - zh * fnsZoom) / 2;
    if (!Number.isFinite(fnsPanX)) fnsPanX = 0;
    if (!Number.isFinite(fnsPanY)) fnsPanY = 0;
    updateFnsWorldTransform();
  }

  function getBottomTransform() {
    return {
      zoom: fnsZoom,
      panX: fnsPanX,
      panY: fnsPanY,
      world: els.fnsWorld?.style?.transform || "",
    };
  }

  /**
   * Effective dock-world position of a Functions node (hand-placed overlay
   * wins over the last layout position).
   * @param {string} id
   */
  function fnsNodePosition(id) {
    const ov = fnsNodePos.get(id);
    if (ov) return ov;
    const pos = fnsGraph?.positions && fnsGraph.positions[id];
    return pos || { x: 0, y: 0 };
  }

  /**
   * Screen rect of a Functions-DAG node (for rail-drift verification).
   * Uses the same pan+scale math as screenXOfFnsWorld — not getBoundingClientRect
   * alone — so a wrong transform is caught even if the DOM moved with the panel.
   * @param {string} nodeId
   */
  function getFnsNodeScreenRect(nodeId) {
    if (!nodeId || !fnsGraph || !els.fnsViewport) return null;
    if (!(fnsGraph.positions && fnsGraph.positions[nodeId]) && !fnsNodePos.has(nodeId)) {
      return null;
    }
    const pos = fnsNodePosition(nodeId);
    if (!pos || !Number.isFinite(pos.x) || !Number.isFinite(pos.y)) return null;
    const vp = els.fnsViewport.getBoundingClientRect();
    const nw = fnsGraph.layout?.nodeW || 0;
    const nh = fnsGraph.layout?.nodeH || 0;
    const left = screenXOfFnsWorld(vp.left, pos.x);
    const top = screenYOfFnsWorld(vp.top, pos.y);
    const width = nw * fnsZoom;
    const height = nh * fnsZoom;
    return {
      nodeId: String(nodeId),
      left,
      top,
      right: left + width,
      bottom: top + height,
      width,
      height,
      zoom: fnsZoom,
      viewportLeft: vp.left,
      viewportTop: vp.top,
    };
  }

  /**
   * Pure Functions-dock pointer policy — assertable without a browser DOM.
   *
   * Rule: a gesture that begins on a node never pans. Below DRAG_MOVE it is a
   * selection click (mirrors file cards). At/above DRAG_MOVE it rearranges the
   * node without selecting. Pan only originates from empty canvas background
   * (or an edge stroke).
   *
   * @param {{
   *   startedOn: "node"|"edge"|"background"|"hud",
   *   movePx?: number,
   *   releasedInsideNode?: boolean,
   * }} opts
   * @returns {{
   *   action: "select"|"drag"|"pan"|"activateEdge"|"ignore"|"none",
   *   pan: boolean,
   *   select: boolean,
   *   suppressClick: boolean,
   * }}
   */
  function resolveFnsPointerGesture(opts) {
    const startedOn = opts && opts.startedOn;
    const movePx = Number(opts && opts.movePx);
    const moved = Number.isFinite(movePx) && movePx >= DRAG_MOVE;
    if (startedOn === "hud") {
      return { action: "ignore", pan: false, select: false, suppressClick: false };
    }
    if (startedOn === "node") {
      // Never pan from a node. Same click-vs-drag threshold as file cards.
      if (moved) {
        return { action: "drag", pan: false, select: false, suppressClick: true };
      }
      const inside =
        opts.releasedInsideNode === undefined ? true : !!opts.releasedInsideNode;
      return {
        action: inside ? "select" : "none",
        pan: false,
        select: inside,
        suppressClick: false,
      };
    }
    if (startedOn === "edge") {
      if (moved) {
        return { action: "pan", pan: true, select: false, suppressClick: true };
      }
      return {
        action: "activateEdge",
        pan: false,
        select: false,
        suppressClick: false,
      };
    }
    // background
    if (moved) {
      return { action: "pan", pan: true, select: false, suppressClick: true };
    }
    return { action: "none", pan: false, select: false, suppressClick: false };
  }

  /**
   * Banner / viewport / chrome geometry for the Functions dock pane.
   * Invariants while dock is open on the Functions tab:
   * - banner height stays constant across dock widths (no wrap)
   * - chrome height / contentTopInset stay constant across side-rail resizes
   *   (chrome is height-locked at 40px; labels compact instead of wrapping)
   * - viewport height stays > 0
   */
  function getFnsPaneMetrics() {
    const banner = els.fnsBanner;
    const vp = els.fnsViewport;
    const pane = els.fnsPane;
    const panel = els.bottomPanel;
    if (!banner || !vp) return null;
    const br = banner.getBoundingClientRect();
    const vr = vp.getBoundingClientRect();
    const pr = pane ? pane.getBoundingClientRect() : null;
    const panelRect = panel ? panel.getBoundingClientRect() : null;
    const chrome = panel ? panel.querySelector(".bottom-chrome") : null;
    const chromeRect = chrome ? chrome.getBoundingClientRect() : null;
    const fnsVisible = bottomOpen && bottomTab === "fns" && pane && !pane.hidden;
    return {
      bannerHeight: br.height,
      bannerTop: br.top,
      viewportHeight: vr.height,
      viewportTop: vr.top,
      viewportWidth: vr.width,
      paneHeight: pr ? pr.height : 0,
      paneWidth: pr ? pr.width : 0,
      panelTop: panelRect ? panelRect.top : 0,
      panelHeight: panelRect ? panelRect.height : 0,
      chromeHeight: chromeRect ? chromeRect.height : 0,
      chromeWidth: chromeRect ? chromeRect.width : bottomChromeWidth,
      // Panel top → banner top. Stable across left/right rail resizes.
      contentTopInset:
        panelRect && Number.isFinite(br.top)
          ? br.top - panelRect.top
          : 0,
      compact: bottomChromeCompact,
      tight: bottomChromeTight,
      bottomOpen,
      tab: bottomTab,
      fnsVisible,
      viewportMinPx: FNS_VIEWPORT_MIN,
      // Honest while Functions pane is showing; closed/other-tab may be 0.
      viewportAlive: !fnsVisible || vr.height > 0,
    };
  }

  /** Set Functions banner HTML and a plain-text title (full string for tooltip). */
  function setFnsBanner(html) {
    const banner = els.fnsBanner;
    if (!banner) return;
    banner.innerHTML = html;
    banner.title = (banner.textContent || "").replace(/\s+/g, " ").trim();
  }

  /**
   * Set dock zoom, keeping the viewport centre anchored (test / HUD helper).
   * @param {number} scale
   */
  function setBottomZoom(scale) {
    const next = Math.max(ZOOM_MIN, Math.min(ZOOM_MAX, Number(scale)));
    if (!Number.isFinite(next) || next <= 0) return getBottomTransform();
    const vp = els.fnsViewport;
    const before = fnsZoom > 0 && Number.isFinite(fnsZoom) ? fnsZoom : ZOOM_MIN;
    fnsZoom = +next.toFixed(3);
    if (vp && before > 0) {
      const rect = vp.getBoundingClientRect();
      const cx = rect.width / 2;
      const cy = rect.height / 2;
      fnsPanX = cx - (cx - fnsPanX) * (fnsZoom / before);
      fnsPanY = cy - (cy - fnsPanY) * (fnsZoom / before);
    }
    updateFnsWorldTransform();
    return getBottomTransform();
  }

  /**
   * Build + paint the Functions DAG for the selected file.
   * @param {{fit?: boolean}} opts
   */
  function syncFnsDepthButtons() {
    const root = els.fnsDepthMode;
    if (!root) return;
    root.querySelectorAll("button[data-depth]").forEach((btn) => {
      const v = btn.getAttribute("data-depth");
      const on =
        v === "all" ? fnsDepth === "all" : String(fnsDepth) === String(v);
      btn.classList.toggle("on", on);
      btn.setAttribute("aria-pressed", on ? "true" : "false");
    });
  }

  /**
   * @param {1|2|'all'} depth
   * @param {{fit?: boolean}} [opts]
   */
  function setFnsDepth(depth, opts = {}) {
    const next =
      depth === "all" || depth === 2 || depth === "2"
        ? depth === "all"
          ? "all"
          : 2
        : 1;
    fnsDepth = next;
    syncFnsDepthButtons();
    if (bottomOpen && bottomTab === "fns") {
      renderFunctionDag({ fit: !!opts.fit });
    }
  }

  /** Redraw Functions-dock edges from effective (possibly hand-placed) positions. */
  function renderFnsEdges() {
    const pathsEl = els.fnsEdgePaths;
    if (!pathsEl || !fnsGraph) return;
    const nw = fnsGraph.layout?.nodeW || 148;
    const nh = fnsGraph.layout?.nodeH || 48;
    const frag = document.createDocumentFragment();
    for (const edge of fnsGraph.edges || []) {
      const a = fnsNodePosition(edge.from);
      const b = fnsNodePosition(edge.to);
      if (
        !Number.isFinite(a.x) ||
        !Number.isFinite(a.y) ||
        !Number.isFinite(b.x) ||
        !Number.isFinite(b.y)
      ) {
        continue;
      }
      // Skip edges whose endpoints are not in the current graph (or overlays).
      const hasA =
        (fnsGraph.positions && fnsGraph.positions[edge.from]) ||
        fnsNodePos.has(edge.from);
      const hasB =
        (fnsGraph.positions && fnsGraph.positions[edge.to]) ||
        fnsNodePos.has(edge.to);
      if (!hasA || !hasB) continue;
      const x1 = a.x + nw;
      const y1 = a.y + nh / 2;
      const x2 = b.x;
      const y2 = b.y + nh / 2;
      const mx = (x1 + x2) / 2;
      const path = document.createElementNS("http://www.w3.org/2000/svg", "path");
      path.setAttribute(
        "d",
        `M${x1},${y1} C${mx},${y1} ${mx},${y2} ${x2},${y2}`
      );
      path.setAttribute(
        "class",
        `fns-edge ${edge.kind}` +
          (fnsActiveEdgeId === edge.id ? " active" : "")
      );
      path.dataset.edgeId = edge.id;
      path.style.pointerEvents = "stroke";
      path.addEventListener("click", (ev) => {
        ev.stopPropagation();
        if (suppressFnsClick) {
          suppressFnsClick = false;
          return;
        }
        focusDagCallSite(edge);
      });
      frag.appendChild(path);
    }
    pathsEl.replaceChildren(frag);
  }

  function renderFunctionDag(opts = {}) {
    const fit = !!opts.fit;
    const nodesEl = els.fnsNodes;
    const pathsEl = els.fnsEdgePaths;
    const svg = els.fnsEdgesSvg;
    if (!nodesEl || !pathsEl) return;

    const HD = window.HorizonFunctionDag;
    if (!HD) {
      nodesEl.innerHTML =
        `<div class="fns-empty">Function DAG module failed to load.</div>`;
      fnsGraph = null;
      fnsFullGraph = null;
      return;
    }

    const fileNode = selectedId
      ? fileNodes.find((n) => n.id === selectedId)
      : null;
    if (!fileNode) {
      fnsGraph = null;
      fnsFullGraph = null;
      fnsDepthFileId = null;
      setFnsBanner(
        `Select a file on the map or in Layers to see its function call DAG.`
      );
      nodesEl.replaceChildren();
      pathsEl.replaceChildren();
      return;
    }

    let full;
    let graph;
    let laid;
    let meta;
    try {
      full = HD.build(fileNode.file, fileNode.id, fnIndex);
      // Per-file depth default: budget-grown neighborhood (see HD.defaultDepth).
      if (fnsDepthFileId !== fileNode.id) {
        fnsDepthFileId = fileNode.id;
        fnsDepth = HD.defaultDepth(full);
        syncFnsDepthButtons();
      }
      const focusId = selectedFnId;
      const sliced = HD.neighborhood(full, focusId, fnsDepth);
      graph = { nodes: sliced.nodes, edges: sliced.edges, scope: sliced.scope };
      meta = sliced.meta;
      laid = HD.layout(graph);
    } catch (err) {
      console.error("[HorizonViewer.renderFunctionDag]", err);
      fnsGraph = null;
      fnsFullGraph = null;
      setFnsBanner(`Function DAG failed to layout this file.`);
      nodesEl.innerHTML =
        `<div class="fns-empty">Could not build the call DAG for this file. See console for details.</div>`;
      pathsEl.replaceChildren();
      return;
    }
    fnsFullGraph = full;
    fnsGraph = {
      ...graph,
      meta,
      layout: {
        worldW: laid.worldW,
        worldH: laid.worldH,
        nodeW: laid.nodeW,
        nodeH: laid.nodeH,
      },
      positions: Object.fromEntries(laid.pos),
    };
    fnsWorldW = laid.worldW;
    fnsWorldH = laid.worldH;

    const sc = graph.scope;
    const focusName = (() => {
      if (!meta?.focusId) return null;
      const n = (full.nodes || []).find((x) => x.id === meta.focusId);
      return n?.name || String(meta.focusId).split("::").pop() || meta.focusId;
    })();
    let depthNote = "";
    if (meta && meta.depth !== "all") {
      depthNote =
        ` · focus <strong>${escapeHtml(focusName || "?")}</strong>` +
        ` · ${meta.depth} hop${meta.depth === 1 ? "" : "s"}` +
        ` · showing ${meta.shownNodes} of ${meta.totalNodes} nodes` +
        `, ${meta.shownEdges} of ${meta.totalEdges} edges`;
      if (meta.truncated) {
        depthNote +=
          ` <span title="Nodes and edges outside this neighborhood are hidden, not deleted. Widen depth or choose All to see every analyser outcome.">(neighborhood — widen depth to see the rest)</span>`;
      }
    } else if (meta) {
      depthNote = ` · all ${meta.totalNodes} nodes`;
    }
    setFnsBanner(
      `<strong>${escapeHtml(sc.fileName || "file")}</strong>` +
        ` · ${sc.seedCount} function${sc.seedCount === 1 ? "" : "s"}` +
        ` · ${sc.resolvedEdges} resolved` +
        ` · ${sc.conflictEdges} conflict` +
        ` · ${sc.unresolvedEdges} unresolved` +
        depthNote +
        ` <span title="Unresolved stubs mean the analyser could not resolve the call — not that a callee is missing from the repo.">(stubs are analyser outcomes)</span>`
    );

    if (svg) {
      svg.setAttribute("width", String(laid.worldW));
      svg.setAttribute("height", String(laid.worldH));
      svg.style.width = `${laid.worldW}px`;
      svg.style.height = `${laid.worldH}px`;
    }
    if (els.fnsWorld) {
      els.fnsWorld.style.width = `${laid.worldW}px`;
      els.fnsWorld.style.height = `${laid.worldH}px`;
    }

    // Edges + nodes prefer hand-placed overlays (fnsNodePos) over laid.pos.
    renderFnsEdges();

    nodesEl.replaceChildren();
    fnsNodeEls = new Map();
    for (const node of graph.nodes) {
      if (!laid.pos.get(node.id) && !fnsNodePos.has(node.id)) continue;
      const p = fnsNodePosition(node.id);
      // div (not button) so conflict candidate chips can be nested buttons.
      const el = document.createElement("div");
      const stubEdge =
        node.kind !== "function"
          ? graph.edges.find((e) => e.to === node.id)
          : null;
      const stubActive =
        !!stubEdge && !!fnsActiveEdgeId && stubEdge.id === fnsActiveEdgeId;
      const isFocus = !!(meta?.focusId && node.id === meta.focusId);
      el.className =
        `fns-node ${node.kind}` +
        (node.external ? " external" : "") +
        (node.kind === "function" && node.fnId === selectedFnId
          ? " selected"
          : "") +
        (isFocus ? " focus" : "") +
        (stubActive ? " stub-active" : "");
      el.style.left = `${p.x}px`;
      el.style.top = `${p.y}px`;
      el.setAttribute("role", "button");
      el.tabIndex = 0;
      el.title =
        node.kind === "function"
          ? node.modulePath || node.fnId
          : node.reason || node.kind;

      const name = document.createElement("span");
      name.className = "fns-node-name";
      name.textContent = node.name;
      el.appendChild(name);

      // Named metaEl so it does not TDZ-shadow the neighborhood `meta` above.
      const metaEl = document.createElement("span");
      metaEl.className = "fns-node-meta";
      if (node.kind === "function") {
        metaEl.textContent = node.external
          ? `${basename(node.filePath) || "other file"} · L${node.line}`
          : `L${node.line}`;
      } else if (node.kind === "conflict") {
        metaEl.textContent = `${node.candidates?.length || 0} candidates · L${node.line}`;
      } else {
        metaEl.textContent = node.reason || "analyser could not resolve";
      }
      el.appendChild(metaEl);

      if (node.kind !== "function") {
        const kind = document.createElement("span");
        kind.className = "fns-node-kind";
        kind.textContent =
          node.kind === "conflict"
            ? "conflict"
            : "unresolved — not a missing fn";
        el.appendChild(kind);
      }

      const activateNode = () => {
        if (node.kind === "function" && node.fnId) {
          selectFunction(node.fnId, { reveal: true, push: true });
          lastFnsNodeActivation = {
            kind: "function",
            nodeId: node.id,
            fnId: node.fnId,
            external: !!node.external,
            reason: null,
            callerFnId: null,
            selectedFnId,
            surfacedReason: false,
          };
          return;
        }
        // Stub: no definition to open. Surface the analyser's reason via the
        // caller's call-site focus — never pretend the stub is a selectable fn.
        const edge = (fnsGraph?.edges || []).find((e) => e.to === node.id);
        const reason =
          (node.reason && String(node.reason)) ||
          (edge && edge.reason && String(edge.reason)) ||
          "analyser could not resolve this call";
        const payload = edge
          ? {
              ...edge,
              // Prefer the stub's honest reason (edge may have "").
              reason,
              kind: edge.kind || node.kind,
            }
          : {
              kind: node.kind,
              callerFnId: node.callerFnId,
              callPath: node.name,
              line: node.line,
              reason,
              candidates: node.candidates,
            };
        focusDagCallSite(payload);
        lastFnsNodeActivation = {
          kind: node.kind,
          nodeId: node.id,
          fnId: null,
          external: false,
          reason,
          callerFnId: payload.callerFnId || null,
          selectedFnId,
          surfacedReason: !!reason,
        };
      };
      el.addEventListener("pointerdown", (ev) =>
        onFnsNodePointerDown(ev, node.id)
      );
      el.addEventListener("click", (ev) => {
        if (ev.target.closest("button")) return;
        ev.stopPropagation();
        // fnsNodeDrag is already cleared on pointerup — use suppressFnsNodeClick,
        // which remembers that this gesture was a rearrange, not a selection.
        if (suppressFnsNodeClick) {
          suppressFnsNodeClick = false;
          lastFnsNodeGesture = {
            ...(lastFnsNodeGesture || {}),
            id: node.id,
            clickSuppressed: true,
            selected: false,
            rightOpenAfter: rightOpen,
          };
          return;
        }
        // Pan-from-edge leftover; node presses never set this.
        if (suppressFnsClick) {
          suppressFnsClick = false;
          return;
        }
        activateNode();
        lastFnsNodeGesture = {
          id: node.id,
          moved: false,
          clickSuppressed: false,
          selected: true,
          selectedId: selectedFnId,
          rightOpenAfter: rightOpen,
        };
      });
      el.addEventListener("keydown", (ev) => {
        if (ev.key === "Enter" || ev.key === " ") {
          ev.preventDefault();
          activateNode();
        }
      });

      if (node.kind === "conflict" && (node.candidates || []).length) {
        for (const cid of node.candidates) {
          if (!fnIndex.has(String(cid))) continue;
          const chip = document.createElement("button");
          chip.type = "button";
          chip.className = "fns-cand";
          chip.textContent = `→ ${String(cid).split("::").pop()}`;
          chip.title = String(cid);
          chip.addEventListener("click", (ev) => {
            ev.stopPropagation();
            selectFunction(cid, { reveal: true, push: true });
          });
          el.appendChild(chip);
        }
      }

      nodesEl.appendChild(el);
      fnsNodeEls.set(node.id, el);
    }

    if (!graph.nodes.length) {
      nodesEl.innerHTML =
        `<div class="fns-empty">No free functions in this file.</div>`;
    }

    updateFnsWorldTransform();
    if (fit) fitFunctionDag();
  }

  function renderDiagnostics() {
    const body = els.diagBody;
    const banner = els.diagBanner;
    if (!body) return;
    body.replaceChildren();
    const HD = window.HorizonDiagnostics;
    if (!HD) {
      body.innerHTML =
        `<div class="diag-empty">Diagnostics module failed to load.</div>`;
      return;
    }
    if (!currentMap) {
      body.innerHTML = `<div class="diag-empty">No map loaded.</div>`;
      if (banner) banner.hidden = true;
      return;
    }

    const summary = currentMap.summary || {};
    const droppedExt = summary.external_dropped ?? 0;
    const droppedCtor = summary.constructor_dropped ?? 0;
    const droppedAssoc = summary.associated_dropped ?? 0;
    const droppedTotal = droppedExt + droppedCtor + droppedAssoc;

    if (banner) {
      banner.hidden = false;
      banner.className = "diag-banner";
      const rec = diagReconcile;
      let head = `${diagEntries.length} audit site${
        diagEntries.length === 1 ? "" : "s"
      }`;
      if (rec) {
        head += ` · walked ${rec.walkedConflicts} conflict${
          rec.walkedConflicts === 1 ? "" : "s"
        }, ${rec.walkedUnresolved} unresolved`;
        if (!rec.match) {
          banner.classList.add("warn");
          head += ` — mismatches summary (${rec.summaryConflicts}/${rec.summaryUnresolved})`;
        }
      }
      const dropText =
        droppedTotal > 0
          ? `Dropped from the map (not listed): ${droppedExt} external, ${droppedCtor} constructor, ${droppedAssoc} associated — deliberate exclusions, not resolution failures.`
          : `No deliberate drops in this map summary.`;
      banner.innerHTML =
        `<div>${escapeHtml(head)}</div>` +
        `<div class="diag-dropped">${escapeHtml(dropText)}</div>`;
      banner.title = `${head} ${dropText}`;
    }

    if (!diagEntries.length) {
      body.innerHTML =
        `<div class="diag-empty">No conflicts or unresolved call sites. The analyser resolved every free-function call it kept.</div>`;
      return;
    }

    const groups =
      diagGroupMode === "file"
        ? HD.groupByFile(diagEntries)
        : HD.groupByReason(diagEntries);

    for (const g of groups) {
      const key =
        diagGroupMode === "file"
          ? `file:${g.filePath}`
          : `reason:${g.reason}`;
      const collapsed = diagCollapsed.has(key);
      const countClass = HD.countClassForKinds(g.kinds);
      const wrap = document.createElement("div");
      wrap.className = "diag-group";

      const head = document.createElement("button");
      head.type = "button";
      head.className = "diag-group-head";
      const title = document.createElement("span");
      title.className = "diag-group-title";
      title.textContent =
        diagGroupMode === "file"
          ? basename(g.filePath) || g.filePath
          : g.reason;
      title.title =
        diagGroupMode === "file" ? g.filePath : g.reason;
      const count = document.createElement("span");
      count.className = `diag-group-count ${countClass}`;
      count.textContent = String(g.entries.length);
      head.appendChild(title);
      head.appendChild(count);
      head.addEventListener("click", () => {
        if (diagCollapsed.has(key)) diagCollapsed.delete(key);
        else diagCollapsed.add(key);
        renderDiagnostics();
      });
      wrap.appendChild(head);

      if (!collapsed) {
        for (const entry of g.entries) {
          wrap.appendChild(makeDiagEntryButton(entry));
        }
      }
      body.appendChild(wrap);
    }
  }

  function makeDiagEntryButton(entry) {
    const btn = document.createElement("button");
    btn.type = "button";
    btn.className =
      "diag-entry" +
      (diagFocus &&
      diagFocus.filePath === entry.filePath &&
      diagFocus.line === entry.line &&
      diagFocus.callPath === entry.callPath
        ? " active"
        : "");

    const top = document.createElement("div");
    top.className = "diag-entry-top";
    const kind = document.createElement("span");
    kind.className = `diag-kind ${entry.kind}`;
    kind.textContent = entry.kind;
    const call = document.createElement("span");
    call.className = "diag-call";
    call.textContent = entry.callPath || "(empty path)";
    const loc = document.createElement("span");
    loc.className = "diag-loc";
    const enc =
      entry.enclosing?.type === "function"
        ? entry.enclosing.functionId
        : basename(entry.filePath);
    loc.textContent = `${basename(entry.filePath)} · L${entry.line}`;
    loc.title = enc || "";
    top.appendChild(kind);
    top.appendChild(call);
    if (entry.fromMacro) {
      const m = document.createElement("span");
      m.className = "diag-macro";
      m.textContent = "macro";
      m.title = "Recovered from macro token tree";
      top.appendChild(m);
    }
    top.appendChild(loc);
    btn.appendChild(top);

    if (entry.reason) {
      const reason = document.createElement("p");
      reason.className = "diag-reason";
      reason.textContent = entry.reason;
      btn.appendChild(reason);
    }

    if (entry.kind === "conflict" && entry.candidates?.length) {
      const ul = document.createElement("ul");
      ul.className = "diag-cands";
      for (const raw of entry.candidates) {
        const id = String(raw);
        const li = document.createElement("li");
        if (fnIndex.has(id)) {
          const a = document.createElement("button");
          a.type = "button";
          a.className = "diag-cand-jump";
          a.textContent = id;
          a.title = "Open candidate in Inspector";
          a.addEventListener("click", (ev) => {
            ev.stopPropagation();
            diagFocus = entry;
            selectFunction(id, { reveal: true, push: true });
            renderDiagnostics();
          });
          li.appendChild(a);
        } else {
          li.textContent = id;
        }
        ul.appendChild(li);
      }
      btn.appendChild(ul);
    }

    btn.addEventListener("click", () => openDiagnosticEntry(entry));
    return btn;
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
      if (node.typeCount)
        pills.push(
          `<span class="insp-pill">${node.typeCount} type${
            node.typeCount === 1 ? "" : "s"
          }</span>`
        );
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
    els.toggleLeft.hidden = false;
    els.toggleRight.hidden = false;
    // Sync chrome to sticky open flags — never force a rail open on load.
    setLeftOpen(leftOpen);
    setRightOpen(rightOpen);
  }

  function showImport(errorMsg) {
    currentMap = null;
    fileNodes = [];
    fileEdges = [];
    layout = new Map();
    nodePos = new Map();
    cardEls = new Map();
    fnsNodePos = new Map();
    fnsNodeEls = new Map();
    fnIndex = new Map();
    selectedId = null;
    selectedFnId = null;
    selHistory = [];
    hoverId = null;
    lastLeftOccupied = null;
    lastBottomOccupied = null;
    diagEntries = [];
    diagReconcile = null;
    diagFocus = null;
    setBottomOpen(false);
    els.mainView.hidden = true;
    els.importScreen.hidden = false;
    renderRecent();
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
    fnsNodePos = new Map();
    fnsNodeEls = new Map();
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
    rebuildDiagnostics();
    // New map → drop stale DAG; depth will re-default for the next file.
    fnsGraph = null;
    fnsFullGraph = null;
    fnsDepthFileId = null;
    fnsActiveEdgeId = null;
    if (bottomOpen) {
      if (bottomTab === "fns") renderFunctionDag({ fit: true });
      else renderDiagnostics();
    }
    fitView();
    if (selectedId) revealCard(selectedId);
    renderInspector();
    if (diagFocus) requestAnimationFrame(() => scrollDiagFocusIntoInspector());
    rememberRecent(map, label);
    syncPagesActive();
  }

  // —— Interaction ——

  function onCardPointerDown(ev, id) {
    if (ev.button !== 0) return;
    ev.stopPropagation();
    ev.preventDefault();
    suppressCardClick = false;
    const p = cardPosition(id);
    cardDrag = {
      id,
      sx: ev.clientX,
      sy: ev.clientY,
      ox: p.x,
      oy: p.y,
      moved: false,
      rightOpenAtStart: rightOpen,
      selectedIdAtStart: selectedId,
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
    const z = zoom > 0 && Number.isFinite(zoom) ? zoom : ZOOM_MIN;
    const nx = cardDrag.ox + dx / z;
    const ny = cardDrag.oy + dy / z;
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
      // Remember drag across the click that browsers fire after pointerup.
      // Clearing cardDrag here used to make the click handler always select.
      suppressCardClick = !!cardDrag.moved;
      lastCardGesture = {
        id: cardDrag.id,
        moved: !!cardDrag.moved,
        suppressCardClick,
        rightOpenAtStart: cardDrag.rightOpenAtStart,
        selectedIdAtStart: cardDrag.selectedIdAtStart,
        rightOpenAfterUp: rightOpen,
        selectedIdAfterUp: selectedId,
        // Filled in by the click handler if it runs:
        clickSuppressed: null,
        selected: null,
        rightOpenAfter: null,
      };
    }
    cardDrag = null;
    window.removeEventListener("pointermove", onCardPointerMove);
    window.removeEventListener("pointerup", onCardPointerUp);
  }

  function onFnsNodePointerDown(ev, id) {
    if (ev.button !== 0) return;
    // Nested candidate chips own their clicks; do not start a rearrange.
    if (ev.target.closest("button")) return;
    ev.stopPropagation();
    ev.preventDefault();
    suppressFnsNodeClick = false;
    const p = fnsNodePosition(id);
    fnsNodeDrag = {
      id,
      sx: ev.clientX,
      sy: ev.clientY,
      ox: p.x,
      oy: p.y,
      moved: false,
      rightOpenAtStart: rightOpen,
      selectedIdAtStart: selectedFnId,
    };
    const el = fnsNodeEls.get(id);
    if (el) el.classList.add("dragging");
    window.addEventListener("pointermove", onFnsNodePointerMove);
    window.addEventListener("pointerup", onFnsNodePointerUp);
  }

  function onFnsNodePointerMove(ev) {
    if (!fnsNodeDrag) return;
    const dx = ev.clientX - fnsNodeDrag.sx;
    const dy = ev.clientY - fnsNodeDrag.sy;
    if (!fnsNodeDrag.moved && Math.hypot(dx, dy) < DRAG_MOVE) return;
    fnsNodeDrag.moved = true;
    // Dock zoom — not the main canvas zoom.
    const z = fnsZoom > 0 && Number.isFinite(fnsZoom) ? fnsZoom : ZOOM_MIN;
    const nx = fnsNodeDrag.ox + dx / z;
    const ny = fnsNodeDrag.oy + dy / z;
    fnsNodePos.set(fnsNodeDrag.id, { x: nx, y: ny });
    const el = fnsNodeEls.get(fnsNodeDrag.id);
    if (el) {
      el.style.left = `${nx}px`;
      el.style.top = `${ny}px`;
    }
    renderFnsEdges();
  }

  function onFnsNodePointerUp() {
    if (fnsNodeDrag) {
      const el = fnsNodeEls.get(fnsNodeDrag.id);
      if (el) el.classList.remove("dragging");
      // Remember drag across the click that browsers fire after pointerup.
      suppressFnsNodeClick = !!fnsNodeDrag.moved;
      lastFnsNodeGesture = {
        id: fnsNodeDrag.id,
        moved: !!fnsNodeDrag.moved,
        suppressFnsNodeClick,
        rightOpenAtStart: fnsNodeDrag.rightOpenAtStart,
        selectedIdAtStart: fnsNodeDrag.selectedIdAtStart,
        rightOpenAfterUp: rightOpen,
        selectedIdAfterUp: selectedFnId,
        clickSuppressed: null,
        selected: null,
        rightOpenAfter: null,
      };
    }
    fnsNodeDrag = null;
    window.removeEventListener("pointermove", onFnsNodePointerMove);
    window.removeEventListener("pointerup", onFnsNodePointerUp);
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

  els.canvas.addEventListener(
    "wheel",
    (ev) => {
      ev.preventDefault();
      zoomMapAt(ev.clientX, ev.clientY, ev.deltaY);
    },
    { passive: false }
  );

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
    persistTheme();
  });

  // Pages rail — Map focuses the canvas; Functions / Diagnostics open the dock.
  // Diff · PR #142 stays disabled (no diff source in the contract).
  if (els.pageMap) {
    els.pageMap.addEventListener("click", () => {
      setBottomOpen(false);
      syncPagesActive();
    });
  }
  if (els.pageFns) {
    els.pageFns.addEventListener("click", () => {
      if (!bottomOpen) setBottomOpen(true);
      setBottomTab("fns");
    });
  }

  els.toggleLeft.addEventListener("click", () => {
    setLeftOpen(!leftOpen);
    // Sidebar size must never change the map transform (owner rule).
  });

  els.toggleRight.addEventListener("click", () => {
    setRightOpen(!rightOpen);
  });

  // —— Rail resize ——
  // Grow: consume canvas, then squeeze the opposite rail toward its minimum.
  // Shrink: restore the opposite rail to its home width before canvas grows.
  // Never call fitView — zoom/pan stay put. Homes update only on manual drag end.

  let leftResize = null;
  let rightResize = null;

  function detachLeftResizeListeners() {
    els.leftRail.removeEventListener("pointermove", onLeftResizeMove);
    els.leftRail.removeEventListener("pointerup", onLeftResizeUp);
    els.leftRail.removeEventListener("pointercancel", onLeftResizeUp);
    els.leftRail.removeEventListener("lostpointercapture", onLeftResizeUp);
    window.removeEventListener("pointerup", onLeftResizeUp, true);
    window.removeEventListener("pointercancel", onLeftResizeUp, true);
    window.removeEventListener("blur", onLeftResizeUp);
  }

  function detachRightResizeListeners() {
    els.rightRail.removeEventListener("pointermove", onRightResizeMove);
    els.rightRail.removeEventListener("pointerup", onRightResizeUp);
    els.rightRail.removeEventListener("pointercancel", onRightResizeUp);
    els.rightRail.removeEventListener("lostpointercapture", onRightResizeUp);
    window.removeEventListener("pointerup", onRightResizeUp, true);
    window.removeEventListener("pointercancel", onRightResizeUp, true);
    window.removeEventListener("blur", onRightResizeUp);
  }

  function onLeftResizeMove(ev) {
    if (!leftResize) return;
    layoutLeftAggressor(leftResize.ow + (ev.clientX - leftResize.sx));
  }
  function onLeftResizeUp(ev) {
    if (!leftResize) return;
    if (
      ev &&
      ev.type === "lostpointercapture" &&
      typeof ev.buttons === "number" &&
      (ev.buttons & 1) !== 0
    ) {
      return;
    }
    endRailDrag(els.leftRail, ev);
    leftResize = null;
    detachLeftResizeListeners();
    // Manual left drag establishes a new home (invalidates prior squeeze memory).
    leftHomeW = leftW;
  }
  els.leftRail.addEventListener("pointerdown", (ev) => {
    if (!leftOpen || ev.button !== 0) return;
    beginRailDrag(els.leftRail, ev);
    leftResize = { sx: ev.clientX, ow: leftW };
    els.leftRail.addEventListener("pointermove", onLeftResizeMove);
    els.leftRail.addEventListener("pointerup", onLeftResizeUp);
    els.leftRail.addEventListener("pointercancel", onLeftResizeUp);
    els.leftRail.addEventListener("lostpointercapture", onLeftResizeUp);
    window.addEventListener("pointerup", onLeftResizeUp, true);
    window.addEventListener("pointercancel", onLeftResizeUp, true);
    window.addEventListener("blur", onLeftResizeUp);
  });

  function onRightResizeMove(ev) {
    if (!rightResize) return;
    layoutRightAggressor(rightResize.ow - (ev.clientX - rightResize.sx));
  }
  function onRightResizeUp(ev) {
    if (!rightResize) return;
    if (
      ev &&
      ev.type === "lostpointercapture" &&
      typeof ev.buttons === "number" &&
      (ev.buttons & 1) !== 0
    ) {
      return;
    }
    endRailDrag(els.rightRail, ev);
    rightResize = null;
    detachRightResizeListeners();
    rightHomeW = rightW;
  }
  els.rightRail.addEventListener("pointerdown", (ev) => {
    if (!rightOpen || ev.button !== 0) return;
    beginRailDrag(els.rightRail, ev);
    rightResize = { sx: ev.clientX, ow: rightW };
    els.rightRail.addEventListener("pointermove", onRightResizeMove);
    els.rightRail.addEventListener("pointerup", onRightResizeUp);
    els.rightRail.addEventListener("pointercancel", onRightResizeUp);
    els.rightRail.addEventListener("lostpointercapture", onRightResizeUp);
    window.addEventListener("pointerup", onRightResizeUp, true);
    window.addEventListener("pointercancel", onRightResizeUp, true);
    window.addEventListener("blur", onRightResizeUp);
  });

  // Window size change may force rails to fit; never touch the map transform.
  window.addEventListener("resize", () => {
    enforceWorkspaceFit();
  });

  // —— Bottom dock resize (vertical; canvas top does not move → no panY) ——
  let bottomResize = null;

  function detachBottomResizeListeners() {
    els.bottomRail.removeEventListener("pointermove", onBottomResizeMove);
    els.bottomRail.removeEventListener("pointerup", onBottomResizeUp);
    els.bottomRail.removeEventListener("pointercancel", onBottomResizeUp);
    els.bottomRail.removeEventListener("lostpointercapture", onBottomResizeUp);
    window.removeEventListener("pointerup", onBottomResizeUp, true);
    window.removeEventListener("pointercancel", onBottomResizeUp, true);
    window.removeEventListener("blur", onBottomResizeUp);
  }

  function onBottomResizeMove(ev) {
    if (!bottomResize) return;
    // Drag up (smaller clientY) grows the dock.
    layoutBottomHeight(bottomResize.oh - (ev.clientY - bottomResize.sy));
  }
  function onBottomResizeUp(ev) {
    if (!bottomResize) return;
    if (
      ev &&
      ev.type === "lostpointercapture" &&
      typeof ev.buttons === "number" &&
      (ev.buttons & 1) !== 0
    ) {
      return;
    }
    endRailDrag(els.bottomRail, ev);
    els.app.classList.remove("resizing-rail-ns");
    bottomResize = null;
    detachBottomResizeListeners();
    bottomHomeH = bottomH;
  }
  if (els.bottomRail) {
    els.bottomRail.addEventListener("pointerdown", (ev) => {
      if (!bottomOpen || ev.button !== 0) return;
      beginRailDrag(els.bottomRail, ev);
      els.app.classList.add("resizing-rail-ns");
      bottomResize = { sy: ev.clientY, oh: bottomH };
      els.bottomRail.addEventListener("pointermove", onBottomResizeMove);
      els.bottomRail.addEventListener("pointerup", onBottomResizeUp);
      els.bottomRail.addEventListener("pointercancel", onBottomResizeUp);
      els.bottomRail.addEventListener("lostpointercapture", onBottomResizeUp);
      window.addEventListener("pointerup", onBottomResizeUp, true);
      window.addEventListener("pointercancel", onBottomResizeUp, true);
      window.addEventListener("blur", onBottomResizeUp);
    });
  }

  if (els.bottomClose) {
    els.bottomClose.addEventListener("click", () => setBottomOpen(false));
  }
  if (els.pageDock) {
    els.pageDock.addEventListener("click", () => {
      if (!bottomOpen || bottomTab !== "diag") {
        setBottomOpen(true);
        setBottomTab("diag");
      } else {
        setBottomOpen(false);
      }
    });
  }
  if (els.tabFunctions) {
    els.tabFunctions.addEventListener("click", () => {
      if (!bottomOpen) setBottomOpen(true);
      setBottomTab("fns");
    });
  }
  if (els.tabDiagnostics) {
    els.tabDiagnostics.addEventListener("click", () => {
      if (!bottomOpen) setBottomOpen(true);
      setBottomTab("diag");
    });
  }
  if (els.fnsFit) {
    els.fnsFit.addEventListener("click", () => fitFunctionDag());
  }
  if (els.fnsDepthMode) {
    els.fnsDepthMode.addEventListener("click", (ev) => {
      const btn = ev.target.closest("button[data-depth]");
      if (!btn || !els.fnsDepthMode.contains(btn)) return;
      const raw = btn.getAttribute("data-depth");
      const depth = raw === "all" ? "all" : raw === "2" ? 2 : 1;
      setFnsDepth(depth, { fit: true });
    });
    syncFnsDepthButtons();
  }
  if (els.fnsZoomIn) {
    els.fnsZoomIn.addEventListener("click", (ev) => {
      ev.stopPropagation();
      fnsZoom = Math.min(ZOOM_MAX, +(fnsZoom + ZOOM_STEP).toFixed(2));
      updateFnsWorldTransform();
    });
  }
  if (els.fnsZoomOut) {
    els.fnsZoomOut.addEventListener("click", (ev) => {
      ev.stopPropagation();
      fnsZoom = Math.max(ZOOM_MIN, +(fnsZoom - ZOOM_STEP).toFixed(2));
      updateFnsWorldTransform();
    });
  }
  if (els.fnsZoomReset) {
    els.fnsZoomReset.addEventListener("click", (ev) => {
      ev.stopPropagation();
      fitFunctionDag();
    });
  }
  if (els.fnsViewport) {
    els.fnsViewport.addEventListener("pointerdown", (ev) => {
      if (ev.button !== 0) return;
      // HUD buttons handle their own clicks; candidate chips are nested buttons.
      if (ev.target.closest(".zoom-hud")) return;
      if (ev.target.closest("button")) return;
      // Node gesture → select-or-drag, never pan (matches resolveFnsPointerGesture).
      if (ev.target.closest(".fns-node")) return;
      suppressFnsClick = false;
      fnsPanDrag = {
        sx: ev.clientX,
        sy: ev.clientY,
        ox: fnsPanX,
        oy: fnsPanY,
        moved: false,
        // Edge strokes may start a pan; background always may.
        startedOn: ev.target.closest(".fns-edge") ? "edge" : "background",
      };
      try {
        els.fnsViewport.setPointerCapture?.(ev.pointerId);
      } catch (_) {
        /* ignore */
      }
    });
    els.fnsViewport.addEventListener("pointermove", (ev) => {
      if (!fnsPanDrag) return;
      const dx = ev.clientX - fnsPanDrag.sx;
      const dy = ev.clientY - fnsPanDrag.sy;
      const movePx = Math.hypot(dx, dy);
      const decision = resolveFnsPointerGesture({
        startedOn: fnsPanDrag.startedOn || "background",
        movePx,
      });
      if (!decision.pan) return;
      if (!fnsPanDrag.moved) {
        fnsPanDrag.moved = true;
        els.fnsViewport.classList.add("panning");
      }
      fnsPanX = fnsPanDrag.ox + dx;
      fnsPanY = fnsPanDrag.oy + dy;
      updateFnsWorldTransform();
    });
    const endFnsPan = () => {
      if (fnsPanDrag) {
        suppressFnsClick = !!fnsPanDrag.moved;
      }
      fnsPanDrag = null;
      els.fnsViewport?.classList.remove("panning");
    };
    els.fnsViewport.addEventListener("pointerup", endFnsPan);
    els.fnsViewport.addEventListener("pointercancel", endFnsPan);
    els.fnsViewport.addEventListener(
      "wheel",
      (ev) => {
        ev.preventDefault();
        ev.stopPropagation();
        zoomFnsAt(ev.clientX, ev.clientY, ev.deltaY);
      },
      { passive: false }
    );
  }
  if (els.diagGroupMode) {
    els.diagGroupMode.addEventListener("click", (ev) => {
      const btn = ev.target.closest("button[data-group]");
      if (!btn) return;
      const mode = btn.dataset.group;
      if (mode !== "reason" && mode !== "file") return;
      diagGroupMode = mode;
      for (const b of els.diagGroupMode.querySelectorAll("button[data-group]")) {
        b.classList.toggle("on", b.dataset.group === mode);
      }
      renderDiagnostics();
    });
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
    if (kind !== "entry" && kind !== "file" && kind !== "fn" && kind !== "struct") {
      return;
    }
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

  /** @type {number|null} */
  let analysePollTimer = null;
  /** @type {number|null} */
  let analyseTickTimer = null;
  let analyseStartedAt = 0;

  function setAnalyseOverlay(visible, pathText) {
    if (!els.analyseOverlay) return;
    els.analyseOverlay.hidden = !visible;
    if (els.analyseOverlayPath && pathText != null) {
      els.analyseOverlayPath.textContent = pathText;
    }
  }

  function stopAnalyseTimers() {
    if (analysePollTimer != null) {
      clearInterval(analysePollTimer);
      analysePollTimer = null;
    }
    if (analyseTickTimer != null) {
      clearInterval(analyseTickTimer);
      analyseTickTimer = null;
    }
  }

  function updateAnalyseElapsed() {
    if (!els.analyseOverlayElapsed || !analyseStartedAt) return;
    const sec = Math.max(0, Math.floor((Date.now() - analyseStartedAt) / 1000));
    els.analyseOverlayElapsed.textContent = `${sec}s`;
  }

  /**
   * Poll GET /api/analyse until the job leaves `running`, then load the map
   * or surface the failure. Resolves only when the job finishes (or contact
   * is lost) so callers can await a complete run. Elapsed seconds tick on the
   * overlay so a long analysis never looks frozen.
   */
  function watchAnalyseJob() {
    stopAnalyseTimers();
    analyseTickTimer = setInterval(updateAnalyseElapsed, 250);
    return new Promise((resolve) => {
      const finish = (result) => {
        stopAnalyseTimers();
        setAnalyseOverlay(false);
        if (els.analyseRun) els.analyseRun.disabled = false;
        resolve(result);
      };
      const poll = async () => {
        let body;
        try {
          const res = await fetch("/api/analyse");
          body = await res.json();
        } catch (err) {
          showImport(`Lost contact with /api/analyse: ${err.message || err}`);
          finish({ status: "error", error: String(err.message || err) });
          return;
        }
        if (body.status === "running") {
          if (body.path && els.analyseOverlayPath) {
            els.analyseOverlayPath.textContent = body.path;
          }
          if (typeof body.elapsed_ms === "number" && els.analyseOverlayElapsed) {
            els.analyseOverlayElapsed.textContent = `${Math.floor(
              body.elapsed_ms / 1000
            )}s`;
          }
          return;
        }
        if (body.status === "failed") {
          showImport(
            `Analysis failed${body.path ? ` for ${body.path}` : ""}: ${
              body.error || "unknown error"
            }`
          );
          finish(body);
          return;
        }
        if (body.status === "done") {
          try {
            const mapRes = await fetch("/api/map");
            if (!mapRes.ok) {
              showImport(
                `Analysis finished but /api/map returned ${mapRes.status}`
              );
              finish({ status: "error", error: `map ${mapRes.status}` });
              return;
            }
            const map = await mapRes.json();
            loadMap(map, body.path || "analyse");
            if (els.analyseForm) els.analyseForm.hidden = true;
            finish(body);
          } catch (err) {
            showImport(
              `Analysis finished but map load failed: ${err.message || err}`
            );
            finish({ status: "error", error: String(err.message || err) });
          }
          return;
        }
        // idle — nothing to load
        finish(body);
      };
      poll();
      analysePollTimer = setInterval(poll, 400);
    });
  }

  /**
   * Start POST /api/analyse for `path`. Returns the accepted status body.
   * @param {string} path
   */
  async function startAnalyse(path) {
    const res = await fetch("/api/analyse", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ path }),
    });
    const body = await res.json().catch(() => ({}));
    if (res.status === 409) {
      // Already running — attach to the in-flight job rather than erroring.
      analyseStartedAt = Date.now() - (Number(body.elapsed_ms) || 0);
      setAnalyseOverlay(true, body.path || path);
      if (els.analyseRun) els.analyseRun.disabled = true;
      return watchAnalyseJob();
    }
    if (!res.ok) {
      throw new Error(body.error || `HTTP ${res.status}`);
    }
    analyseStartedAt = Date.now();
    setAnalyseOverlay(true, body.path || path);
    updateAnalyseElapsed();
    if (els.analyseRun) els.analyseRun.disabled = true;
    return watchAnalyseJob();
  }

  if (els.openFolder) {
    els.openFolder.addEventListener("click", () => {
      if (!els.analyseForm) return;
      els.analyseForm.hidden = false;
      els.analysePath?.focus();
      // Sensible default: the workspace that produced the current map, if any.
      if (els.analysePath && !els.analysePath.value) {
        els.analysePath.value = "";
      }
    });
  }
  if (els.analyseCancelForm) {
    els.analyseCancelForm.addEventListener("click", () => {
      if (els.analyseForm) els.analyseForm.hidden = true;
    });
  }
  if (els.analyseForm) {
    els.analyseForm.addEventListener("submit", async (ev) => {
      ev.preventDefault();
      const path = (els.analysePath?.value || "").trim();
      if (!path) {
        showImport("Enter a repository path to analyse.");
        return;
      }
      els.importError.hidden = true;
      try {
        await startAnalyse(path);
      } catch (err) {
        setAnalyseOverlay(false);
        if (els.analyseRun) els.analyseRun.disabled = false;
        showImport(`Could not start analysis: ${err.message || err}`);
      }
    });
  }

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
    bootStatus = { phase: "start" };
    try {
      const q = new URLSearchParams(location.search);
      if (q.get("theme") === "dark") {
        dark = true;
        userSetTheme = true;
      } else if (q.get("theme") === "light") {
        dark = false;
        userSetTheme = true;
      } else {
        let stored = null;
        try {
          stored = localStorage.getItem(THEME_KEY);
        } catch (_) {
          /* ignore */
        }
        if (stored === "dark" || stored === "light") {
          dark = stored === "dark";
          userSetTheme = true;
        } else {
          dark = window.matchMedia("(prefers-color-scheme: dark)").matches;
        }
      }
    } catch (_) {
      dark = false;
    }
    applyTheme();
    renderRecent();
    // Publish defaults without treating the initial occupied width as a delta.
    lastLeftOccupied = null;
    lastBottomOccupied = null;
    setLeftWidth(LEFT_DEFAULT);
    setRightWidth(RIGHT_DEFAULT);
    publishBottomVars();
    setBottomOpen(false);
    syncPagesActive();

    // Side-rail / window resize changes chrome width — re-apply compact classes.
    const bottomChrome = els.bottomPanel
      ? els.bottomPanel.querySelector(".bottom-chrome")
      : null;
    if (bottomChrome && typeof ResizeObserver !== "undefined") {
      const ro = new ResizeObserver(() => syncBottomChromeCompact());
      ro.observe(bottomChrome);
    }

    try {
      window
        .matchMedia("(prefers-color-scheme: dark)")
        .addEventListener("change", (e) => {
          if (userSetTheme) return;
          dark = e.matches;
          applyTheme();
        });
    } catch (_) {}

    // Expose selection / layout for verification / later slices.
    window.HorizonViewer = {
      selectFile: (id, opts) => selectFile(id, opts || {}),
      selectFunction: (id, opts) => selectFunction(id, opts || {}),
      goBack,
      getSelection: () => snapshotSelection(),
      loadMap: (map, label) => loadMap(map, label || "api"),
      /** Start in-process analysis of a repo path (POST /api/analyse). */
      startAnalyse: (path) => startAnalyse(String(path || "")),
      /** Whether the analyse progress overlay is visible. */
      analyseOverlayVisible: () =>
        !!(els.analyseOverlay && !els.analyseOverlay.hidden),
      /** Current theme: `'light'` | `'dark'`. */
      getTheme: () => (dark ? "dark" : "light"),
      /** Toggle or set theme; persists when the user sets it. */
      setTheme: (next) => {
        if (next === "dark" || next === "light") dark = next === "dark";
        else dark = !dark;
        userSetTheme = true;
        applyTheme();
        persistTheme();
        return dark ? "dark" : "light";
      },
      /** Recent maps stored in localStorage (labels only — not full JSON). */
      getRecent: () =>
        readRecent().map((e) => ({
          id: e.id,
          label: e.label,
          root: e.root,
          savedAt: e.savedAt,
        })),
      getUiState: () => ({
        bootStatus,
        importHidden: !!els.importScreen?.hidden,
        mainHidden: !!els.mainView?.hidden,
        fileCount: fileNodes.length,
        edgeCount: fileEdges.length,
        hasMap: !!currentMap,
        emptyHidden: !!els.canvasEmpty?.hidden,
        leftOccupied: leftOccupiedPx(),
        rightOccupied: rightOccupiedPx(),
        filters: { ...filters },
      }),
      getFilters: () => ({ ...filters }),
      setFilter: (kind, on) => {
        if (!(kind in filters)) return false;
        filters[kind] = !!on;
        const btn = els.filterChips?.querySelector(`[data-kind="${kind}"]`);
        if (btn && !btn.disabled) btn.classList.toggle("on", filters[kind]);
        renderLayers();
        refreshFocus();
        return true;
      },
      getRailWidths: () => ({
        // Stored panel widths (authoritative when open; retained while collapsed).
        leftWidth: leftW,
        rightWidth: rightW,
        // Back-compat aliases — NOT occupied space; use *Occupied for that.
        left: leftW,
        right: rightW,
        // Space currently taken in the workspace (0 when collapsed).
        leftOccupied: leftOccupiedPx(),
        rightOccupied: rightOccupiedPx(),
        // Squeeze-restore memory (passive conquest target).
        leftHome: leftHomeW,
        rightHome: rightHomeW,
        leftMin: LEFT_MIN,
        rightMin: RIGHT_MIN,
        leftMax: maxLeftWidth(),
        rightMax: maxRightWidth(),
        canvas: canvasWidth(),
        leftOpen,
        rightOpen,
        workspace: workspaceWidth(),
      }),
      getTransform: () => ({
        zoom,
        panX,
        panY,
        world: els.world?.style?.transform || "",
      }),
      /** Dock Functions canvas transform — independent of getTransform(). */
      getBottomTransform,
      setBottomZoom,
      screenXOfWorld,
      /** Dock equivalents of screenXOfWorld — rail-drift / pan compensation probes. */
      screenXOfFnsWorld,
      screenYOfFnsWorld,
      getFnsNodeScreenRect,
      getFnsPaneMetrics,
      /** Pure JS layout — the runtime that must stay correct (Rust is the oracle). */
      computeRightAggressorLayout,
      computeLeftAggressorLayout,
      /** Shared fixture table vs JS twin (async — fetches the rail cases JSON). */
      runRailFixtureTable,
      setRightWidth,
      setLeftWidth,
      setBottomOpen,
      setBottomHeight,
      setBottomTab,
      getBottom: () => ({
        open: bottomOpen,
        height: bottomH,
        home: bottomHomeH,
        min: BOTTOM_MIN,
        tab: bottomTab,
        groupMode: diagGroupMode,
        entryCount: diagEntries.length,
        reconcile: diagReconcile,
      }),
      /**
       * Bottom-rail geometry for hit-test verification after rebuild.
       * Expect width ≈ center-col width, height ≈ 11, top ≈ panel top.
       */
      getBottomRailHit: () => {
        const rail = els.bottomRail;
        const panel = els.bottomPanel;
        const col = els.centerCol;
        if (!rail || !panel) return null;
        const r = rail.getBoundingClientRect();
        const p = panel.getBoundingClientRect();
        const c = col ? col.getBoundingClientRect() : null;
        return {
          hidden: !!rail.hidden,
          width: r.width,
          height: r.height,
          top: r.top,
          left: r.left,
          bottom: r.bottom,
          panelTop: p.top,
          panelLeft: p.left,
          panelWidth: p.width,
          centerWidth: c ? c.width : null,
          // Inset strip: rail top aligns with panel top (±2px).
          alignedWithDockTop: Math.abs(r.top - p.top) <= 2,
          stretchesAcross: c ? Math.abs(r.width - c.width) <= 2 : false,
          heightOk: Math.abs(r.height - 11) <= 1,
          ok:
            !rail.hidden &&
            Math.abs(r.height - 11) <= 1 &&
            Math.abs(r.top - p.top) <= 2 &&
            (c ? Math.abs(r.width - c.width) <= 2 : r.width > 100),
        };
      },
      /** Function DAG snapshot for the current selection (Functions tab). */
      getFunctionDag: () => {
        if (!fnsGraph) return null;
        return {
          scope: fnsGraph.scope,
          nodeCount: fnsGraph.nodes.length,
          edgeCount: fnsGraph.edges.length,
          nodes: fnsGraph.nodes.map((n) => ({
            id: n.id,
            kind: n.kind,
            role: n.role,
            name: n.name,
            fnId: n.fnId || null,
          })),
          edges: fnsGraph.edges.map((e) => ({
            id: e.id,
            from: e.from,
            to: e.to,
            kind: e.kind,
            callPath: e.callPath,
            line: e.line,
          })),
          selectedFnId,
          selectedFileId: selectedId,
          tab: bottomTab,
          depth: fnsDepth,
          meta: fnsGraph.meta || null,
          fullNodeCount: fnsFullGraph ? fnsFullGraph.nodes.length : null,
          fullEdgeCount: fnsFullGraph ? fnsFullGraph.edges.length : null,
        };
      },
      /** Current Functions neighborhood depth (`1` | `2` | `'all'`). */
      getFnsDepth: () => fnsDepth,
      /**
       * Set neighborhood depth and re-render. Busy files default via
       * `HorizonFunctionDag.defaultDepth` (node-budget growth); pass `'all'`
       * for the full subgraph.
       */
      setFnsDepth: (depth, opts) => setFnsDepth(depth, opts || {}),
      renderFunctionDag: (opts) => renderFunctionDag(opts || {}),
      fitFunctionDag,
      openDiagnosticEntry,
      renderDiagnostics,
      /**
       * Deliberate Inspector-open policy (owner rule).
       * Selection opens; pan / card-drag / fns-node-drag / rail-resize / zoom must not.
       */
      inspectorOpenPolicy: () => ({
        selectionOpens: true,
        viewManipulationOpens: false,
        cardDragThresholdPx: DRAG_MOVE,
        // Functions dock: click selects; drag rearranges; pan only from background.
        fnsNodePressSelects: true,
        fnsNodeDragThresholdPx: DRAG_MOVE,
        fnsPanFromBackgroundOnly: true,
        openHelper: "openInspectorForSelection",
      }),
      /** Pure click-vs-drag-vs-pan rule for the Functions dock (no DOM required). */
      resolveFnsPointerGesture,
      /** Last Functions-node activation — honest null before any click. */
      getLastFnsNodeActivation: () =>
        lastFnsNodeActivation ? { ...lastFnsNodeActivation } : null,
      /** Current call-site / stub focus — honest null when none. */
      getDiagFocus: () => (diagFocus ? { ...diagFocus } : null),
      /** Outcome of the last card press/release (+ click if any). For hand-drag checks. */
      getLastCardGesture: () => {
        const g = lastCardGesture;
        if (!g) return null;
        const selNow = selectedId;
        return {
          ...g,
          selectedIdNow: selNow,
          selectionChanged: g.selectedIdAtStart !== selNow,
          inspectorOpened:
            g.rightOpenAtStart === false && rightOpen === true,
          rightOpenNow: rightOpen,
        };
      },
      /**
       * Outcome of the last Functions-node press/release (+ click if any).
       * Honest null before any gesture.
       */
      getLastFnsNodeGesture: () => {
        const g = lastFnsNodeGesture;
        if (!g) return null;
        const selNow = selectedFnId;
        return {
          ...g,
          selectedIdNow: selNow,
          selectionChanged: g.selectedIdAtStart !== selNow,
          inspectorOpened:
            g.rightOpenAtStart === false && rightOpen === true,
          rightOpenNow: rightOpen,
        };
      },
      /**
       * Dock-world position of a Functions node. Honest null when the graph is
       * absent or the id is not in the current layout / overlays.
       * @param {string} id
       */
      getFnsNodePosition: (id) => {
        if (!id || !fnsGraph) return null;
        const key = String(id);
        if (
          !(fnsGraph.positions && fnsGraph.positions[key]) &&
          !fnsNodePos.has(key)
        ) {
          return null;
        }
        const p = fnsNodePosition(key);
        return { id: key, x: p.x, y: p.y, handPlaced: fnsNodePos.has(key) };
      },
      /**
       * Set a Functions node's dock-world position (for console drag arithmetic).
       * Updates the overlay map, live DOM, and edges. Returns the new position
       * or null when the graph / id is unavailable.
       * @param {string} id
       * @param {number} x
       * @param {number} y
       */
      setFnsNodePosition: (id, x, y) => {
        if (!id || !fnsGraph) return null;
        const key = String(id);
        if (
          !(fnsGraph.positions && fnsGraph.positions[key]) &&
          !fnsNodePos.has(key)
        ) {
          return null;
        }
        const nx = Number(x);
        const ny = Number(y);
        if (!Number.isFinite(nx) || !Number.isFinite(ny)) return null;
        fnsNodePos.set(key, { x: nx, y: ny });
        const el = fnsNodeEls.get(key);
        if (el) {
          el.style.left = `${nx}px`;
          el.style.top = `${ny}px`;
        }
        renderFnsEdges();
        return { id: key, x: nx, y: ny, handPlaced: true };
      },
      /**
       * Cheap smoke: invoke exported helpers so a missing binding throws here
       * instead of mid-gesture in production. Call after boot.
       */
      smokeCheck: () => {
        const names = [
          "getRailWidths",
          "getTransform",
          "getBottomTransform",
          "setBottomZoom",
          "screenXOfFnsWorld",
          "screenYOfFnsWorld",
          "getFnsNodeScreenRect",
          "getFnsPaneMetrics",
          "getBottom",
          "getBottomRailHit",
          "getFunctionDag",
          "getFnsDepth",
          "setFnsDepth",
          "startAnalyse",
          "analyseOverlayVisible",
          "getTheme",
          "setTheme",
          "getRecent",
          "getFilters",
          "setFilter",
          "getSelection",
          "getUiState",
          "inspectorOpenPolicy",
          "getLastCardGesture",
          "getLastFnsNodeGesture",
          "getFnsNodePosition",
          "setFnsNodePosition",
          "getLastFnsNodeActivation",
          "getDiagFocus",
          "resolveFnsPointerGesture",
          "runLayoutAcceptance",
          "computeRightAggressorLayout",
          "computeLeftAggressorLayout",
        ];
        const checks = [];
        for (const name of names) {
          try {
            const fn = window.HorizonViewer[name];
            if (typeof fn !== "function") {
              checks.push({ name, ok: false, error: "not a function" });
              continue;
            }
            if (name === "computeRightAggressorLayout") {
              fn(1600, 316, true, 236);
            } else if (name === "computeLeftAggressorLayout") {
              fn(1600, 236, true, 316);
            } else if (name === "setBottomZoom") {
              const before = getBottomTransform();
              const after = fn(before.zoom);
              if (
                !after ||
                typeof after.zoom !== "number" ||
                !Number.isFinite(after.zoom)
              ) {
                checks.push({
                  name,
                  ok: false,
                  error: "setBottomZoom did not return a finite transform",
                });
                continue;
              }
            } else if (name === "screenXOfFnsWorld") {
              const x = fn(100, 10);
              if (typeof x !== "number" || !Number.isFinite(x)) {
                checks.push({
                  name,
                  ok: false,
                  error: "screenXOfFnsWorld did not return a finite number",
                });
                continue;
              }
            } else if (name === "screenYOfFnsWorld") {
              const y = fn(100, 10);
              if (typeof y !== "number" || !Number.isFinite(y)) {
                checks.push({
                  name,
                  ok: false,
                  error: "screenYOfFnsWorld did not return a finite number",
                });
                continue;
              }
            } else if (name === "getFnsNodeScreenRect") {
              // Honest null when no DAG — must not throw.
              fn("__missing__");
            } else if (name === "getLastFnsNodeActivation") {
              // Honest null before any click — must not read undefined fields.
              const g = fn();
              if (g !== null && (typeof g !== "object" || !g.kind)) {
                checks.push({
                  name,
                  ok: false,
                  error: "getLastFnsNodeActivation returned unexpected value",
                });
                continue;
              }
            } else if (name === "getLastFnsNodeGesture") {
              // Honest null before any node gesture.
              const g = fn();
              if (g !== null && (typeof g !== "object" || g.id == null)) {
                checks.push({
                  name,
                  ok: false,
                  error: "getLastFnsNodeGesture returned unexpected value",
                });
                continue;
              }
            } else if (name === "getFnsNodePosition") {
              // Honest null when no DAG / unknown id — must not throw.
              const g = fn("__missing__");
              if (g !== null) {
                checks.push({
                  name,
                  ok: false,
                  error: "getFnsNodePosition should be null for missing id",
                });
                continue;
              }
            } else if (name === "setFnsNodePosition") {
              // Side-effecting DOM write — probe presence only.
            } else if (name === "getDiagFocus") {
              const g = fn();
              if (g !== null && typeof g !== "object") {
                checks.push({
                  name,
                  ok: false,
                  error: "getDiagFocus returned unexpected value",
                });
                continue;
              }
            } else if (name === "resolveFnsPointerGesture") {
              const cases = [
                {
                  name: "nodeTap",
                  opts: {
                    startedOn: "node",
                    movePx: 0,
                    releasedInsideNode: true,
                  },
                  expect: { action: "select", pan: false, suppressClick: false },
                },
                {
                  name: "nodeDrift4",
                  opts: {
                    startedOn: "node",
                    movePx: 4,
                    releasedInsideNode: true,
                  },
                  expect: { action: "select", pan: false, suppressClick: false },
                },
                {
                  name: "nodeDrag8",
                  opts: {
                    startedOn: "node",
                    movePx: 8,
                    releasedInsideNode: true,
                  },
                  expect: {
                    action: "drag",
                    pan: false,
                    select: false,
                    suppressClick: true,
                  },
                },
                {
                  name: "nodeDrag20",
                  opts: {
                    startedOn: "node",
                    movePx: 20,
                    releasedInsideNode: true,
                  },
                  expect: {
                    action: "drag",
                    pan: false,
                    select: false,
                    suppressClick: true,
                  },
                },
                {
                  name: "nodeReleaseOutside",
                  opts: {
                    startedOn: "node",
                    movePx: 3,
                    releasedInsideNode: false,
                  },
                  expect: { action: "none", pan: false, select: false },
                },
                {
                  name: "backgroundPan",
                  opts: { startedOn: "background", movePx: 8 },
                  expect: { action: "pan", pan: true, suppressClick: true },
                },
                {
                  name: "backgroundTap",
                  opts: { startedOn: "background", movePx: 0 },
                  expect: { action: "none", pan: false },
                },
              ];
              for (const c of cases) {
                const got = fn(c.opts);
                const bad = Object.keys(c.expect).some(
                  (k) => got[k] !== c.expect[k]
                );
                if (bad) {
                  checks.push({
                    name: `resolveFnsPointerGesture:${c.name}`,
                    ok: false,
                    error: `expected ${JSON.stringify(c.expect)} got ${JSON.stringify(got)}`,
                  });
                }
              }
              if (checks.some((c) => c.name.startsWith("resolveFnsPointerGesture:") && !c.ok)) {
                continue;
              }
            } else if (name === "getFnsPaneMetrics") {
              const m = fn();
              if (
                !m ||
                typeof m.bannerHeight !== "number" ||
                typeof m.viewportHeight !== "number" ||
                typeof m.viewportTop !== "number" ||
                typeof m.chromeWidth !== "number" ||
                typeof m.compact !== "boolean" ||
                typeof m.tight !== "boolean" ||
                m.viewportMinPx !== FNS_VIEWPORT_MIN
              ) {
                checks.push({
                  name,
                  ok: false,
                  error: "getFnsPaneMetrics returned incomplete metrics",
                });
                continue;
              }
              // While Functions pane is visible the canvas must not collapse.
              if (m.fnsVisible && !(m.viewportHeight > 0 && m.viewportAlive)) {
                checks.push({
                  name,
                  ok: false,
                  error: `viewport collapsed while Functions open (h=${m.viewportHeight})`,
                });
                continue;
              }
            } else if (name === "inspectorOpenPolicy") {
              const p = fn();
              if (
                !p ||
                p.selectionOpens !== true ||
                p.viewManipulationOpens !== false ||
                p.fnsNodePressSelects !== true ||
                p.fnsNodeDragThresholdPx !== DRAG_MOVE ||
                p.fnsPanFromBackgroundOnly !== true
              ) {
                checks.push({
                  name,
                  ok: false,
                  error: "inspectorOpenPolicy missing Functions gesture flags",
                });
                continue;
              }
            } else if (name === "startAnalyse") {
              // Side-effecting network call — probe presence only.
            } else if (name === "setFnsDepth") {
              // Would re-render the DAG; presence is enough here.
            } else if (name === "setFilter") {
              // Would re-dim cards; presence is enough here.
            } else if (name === "setTheme") {
              // Persist + swap tokens — presence is enough; boot already applied.
            } else {
              fn();
            }
            checks.push({ name, ok: true });
          } catch (err) {
            checks.push({
              name,
              ok: false,
              error: String(err && err.message ? err.message : err),
            });
          }
        }
        // Dead symbols from a superseded policy must stay gone.
        // Name is split so asset needles can ban the contiguous identifier.
        const deadName = ["diag", "Click", "Opens", "Inspector"].join("");
        const leaked = typeof window.HorizonViewer[deadName] === "function";
        checks.push({
          name: "noDeadDiagAccessor",
          ok: !leaked,
          error: leaked ? "dead accessor still exported" : undefined,
        });
        return { ok: checks.every((c) => c.ok), checks };
      },
      /** Live drag (does not rewrite home — for continuous aggressor simulation). */
      applyRightWidth: (w) => layoutRightAggressor(w),
      applyLeftWidth: (w) => layoutLeftAggressor(w),
      commitLeftHome: () => {
        leftHomeW = leftW;
      },
      commitRightHome: () => {
        rightHomeW = rightW;
      },
      /**
       * Run the owner worked example against the live JS pure functions.
       * Returns { ok, steps } — evaluate this in the embedded browser.
       */
      runLayoutAcceptance: () => {
        const W = 1600;
        const Lmin = LEFT_MIN;
        const steps = [];
        let leftHome = 236;
        let left = 236;
        let right = 316;
        const applyR = (desired) => {
          const next = computeRightAggressorLayout(W, desired, true, leftHome);
          left = next.left;
          right = next.right;
          return {
            left,
            right,
            canvas: Math.max(0, W - left - right),
            leftHome,
          };
        };
        steps.push({ name: "start", ...applyR(316) });
        steps.push({ name: "canvasZero", ...applyR(1364) });
        steps.push({ name: "leftMin", ...applyR(1420) });
        steps.push({ name: "hardStop", ...applyR(2000) });
        steps.push({ name: "restoreHome", ...applyR(1364) });
        steps.push({ name: "canvasGrows", ...applyR(1363) });
        const expect = [
          [236, 316, 1048, 236],
          [236, 1364, 0, 236],
          [Lmin, 1420, 0, 236],
          [Lmin, 1420, 0, 236],
          [236, 1364, 0, 236],
          [236, 1363, 1, 236],
        ];
        const ok = steps.every(
          (s, i) =>
            s.left === expect[i][0] &&
            s.right === expect[i][1] &&
            s.canvas === expect[i][2] &&
            s.leftHome === expect[i][3]
        );
        return { ok, steps, expect };
      },
      fitView,
    };

    // Catch missing bindings at boot — ReferenceErrors in accessors used to
    // ship undetected because Rust/http needles never evaluate JS.
    try {
      const smoke = window.HorizonViewer.smokeCheck();
      if (!smoke.ok) {
        console.error("[HorizonViewer.smokeCheck] failed", smoke);
      }
    } catch (err) {
      console.error("[HorizonViewer.smokeCheck] threw", err);
    }

    try {
      const res = await fetch("/api/map");
      bootStatus = { phase: "fetched", status: res.status, ok: res.ok };
      if (res.ok) {
        const map = await res.json();
        try {
          loadMap(map, "startup");
          bootStatus = {
            phase: "loaded",
            status: res.status,
            files: fileNodes.length,
            importHidden: !!els.importScreen?.hidden,
            mainHidden: !!els.mainView?.hidden,
          };
          return;
        } catch (err) {
          bootStatus = {
            phase: "loadMapError",
            error: String(err && err.message ? err.message : err),
          };
          showImport(`Failed to load map: ${bootStatus.error}`);
          return;
        }
      }
    } catch (err) {
      bootStatus = {
        phase: "fetchError",
        error: String(err && err.message ? err.message : err),
      };
      showImport(`Failed to fetch /api/map: ${bootStatus.error}`);
      return;
    }
    bootStatus = { phase: "noMap" };
    showImport(null);
  }

  boot();
})();
