/**
 * Diagnostics collection & grouping for the Horizon function map.
 *
 * Reused by the Slice 3 bottom Diagnostics tab. Walks Conflict and Unresolved
 * CallSites only — deliberate drops (external / constructor / associated) are
 * never listed (see MapSummary counters elsewhere).
 *
 * Contract matches `horizon_map::CallTarget` adjacent tagging (`kind` + `data`)
 * and `CallSite` fields including `byte_start` / `byte_end` / `from_macro`.
 *
 * Global: window.HorizonDiagnostics
 */
(() => {
  "use strict";

  function basename(path) {
    if (!path) return "";
    const parts = String(path).replace(/\\/g, "/").split("/");
    return parts[parts.length - 1] || path;
  }

  /**
   * Walk every Conflict and Unresolved CallSite in the loaded map.
   * @param {object} map Repository JSON
   * @returns {{entries: object[], reconcile: object}}
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
        byteStart: site.byte_start ?? 0,
        byteEnd: site.byte_end ?? 0,
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

  window.HorizonDiagnostics = {
    basename,
    collectDiagnostics,
    groupByReason,
    groupByFile,
    countClassForKinds,
  };
})();
