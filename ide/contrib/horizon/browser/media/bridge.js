/**
 * Horizon Map ↔ workbench host message bridge (built-in EditorPane webview).
 *
 * The webview never talks to the sidecar HTTP API directly. All analyse /
 * map / source / inspection traffic goes through acquireVsCodeApi().postMessage
 * (provided by the Code-OSS webview host for this workbench contrib).
 *
 * Protocol: see ide/contrib/horizon/README.md and common/horizon.ts.
 */
(() => {
  "use strict";

  /** @type {ReturnType<typeof acquireVsCodeApi>|null} */
  let vscodeApi = null;
  try {
    if (typeof acquireVsCodeApi === "function") {
      vscodeApi = acquireVsCodeApi();
    }
  } catch (_) {
    vscodeApi = null;
  }

  /** @type {Set<(msg: object) => void>} */
  const listeners = new Set();

  /** @type {Map<string, {resolve: Function, reject: Function, timer: number}>} */
  const pendingSource = new Map();

  /** @type {Map<string, {resolve: Function, reject: Function, timer: number}>} */
  const pendingHover = new Map();

  /** @type {Map<string, {resolve: Function, reject: Function, timer: number}>} */
  const pendingSemantic = new Map();

  /** @type {Map<string, {resolve: Function, reject: Function, timer: number}>} */
  const pendingDefinition = new Map();

  /** @type {null | ((result: object) => void)} */
  let analyseWaiter = null;

  function post(message) {
    if (!vscodeApi) {
      console.warn("[HorizonBridge] no vscode API; drop", message);
      return;
    }
    vscodeApi.postMessage(message);
  }

  window.addEventListener("message", (event) => {
    const msg = event.data;
    if (!msg || typeof msg !== "object" || typeof msg.type !== "string") return;

    if (msg.type === "sourceResult" && msg.requestId) {
      const pending = pendingSource.get(String(msg.requestId));
      if (pending) {
        clearTimeout(pending.timer);
        pendingSource.delete(String(msg.requestId));
        // Prefer W3 errorKind (stale/missing/…) when present for SOURCE_MESSAGES.
        const normalized = msg.errorKind
          ? Object.assign({}, msg, { error: msg.errorKind })
          : msg;
        pending.resolve(normalized);
      }
    }

    if (msg.type === "hoverResult" && msg.requestId) {
      const pending = pendingHover.get(String(msg.requestId));
      if (pending) {
        clearTimeout(pending.timer);
        pendingHover.delete(String(msg.requestId));
        pending.resolve(msg);
      }
    }

    if (msg.type === "semanticTokensResult" && msg.requestId) {
      const pending = pendingSemantic.get(String(msg.requestId));
      if (pending) {
        clearTimeout(pending.timer);
        pendingSemantic.delete(String(msg.requestId));
        pending.resolve(msg);
      }
    }

    if (msg.type === "definitionAtResult" && msg.requestId) {
      const pending = pendingDefinition.get(String(msg.requestId));
      if (pending) {
        clearTimeout(pending.timer);
        pendingDefinition.delete(String(msg.requestId));
        pending.resolve(msg);
      }
    }

    if (msg.type === "analyseResult" && analyseWaiter) {
      const status = msg.status;
      if (status === "done" || status === "failed" || status === "error" || status === "idle") {
        const wait = analyseWaiter;
        analyseWaiter = null;
        wait(msg);
      }
    }

    for (const fn of listeners) {
      try {
        fn(msg);
      } catch (err) {
        console.error("[HorizonBridge] listener error", err);
      }
    }
  });

  /**
   * @typedef {object} HorizonBridge
   */
  window.HorizonBridge = {
    /** Whether running inside a VS Code webview with messaging. */
    isVsCode: !!vscodeApi,

    /** Post an outbound message to the extension host. */
    post,

    /**
     * Subscribe to inbound host messages. Returns an unsubscribe function.
     * @param {(msg: object) => void} fn
     */
    onMessage(fn) {
      listeners.add(fn);
      return () => listeners.delete(fn);
    },

    /** Webview finished bootstrapping — host may push mapData / workspaceInfo. */
    ready() {
      post({ type: "ready" });
    },

    /**
     * Request workspace analysis. Host (W3/W4) runs the sidecar.
     * @param {string} [path] optional override; omit for workspace root
     */
    analyse(path) {
      const payload = { type: "analyse" };
      if (path) payload.path = String(path);
      post(payload);
    },

    /**
     * Wait for a terminal analyseResult after posting analyse.
     * Progress (`running`) is delivered via onMessage; this resolves on
     * done / failed / error / idle.
     * @param {string} [path]
     * @returns {Promise<object>}
     */
    analyseAndWait(path) {
      return new Promise((resolve) => {
        analyseWaiter = resolve;
        this.analyse(path);
      });
    },

    /**
     * Notify host that a free function was selected (W3 opens inspection).
     * @param {object} detail
     */
    selectFunction(detail) {
      post({
        type: "selectFunction",
        functionId: detail.functionId != null ? String(detail.functionId) : null,
        fileId: detail.fileId != null ? String(detail.fileId) : null,
        filePath: detail.filePath != null ? String(detail.filePath) : null,
        functionName:
          detail.functionName != null ? String(detail.functionName) : null,
        line: detail.line ?? null,
        byteStart: detail.byteStart ?? null,
        byteEnd: detail.byteEnd ?? null,
        contentHash: detail.contentHash != null ? String(detail.contentHash) : null,
      });
    },

    /**
     * Notify host that a file card / layer row was selected.
     * @param {object} detail
     */
    selectFile(detail) {
      post({
        type: "selectFile",
        fileId: detail.fileId != null ? String(detail.fileId) : null,
        filePath: detail.filePath != null ? String(detail.filePath) : null,
      });
    },

    /**
     * Ask host to go to the definition of a call target that is not in the map.
     * The host resolves it with rust-analyzer at the call site's own position
     * in the enclosing file, so external / method / associated targets work
     * without the map re-implementing name resolution.
     * @param {object} detail
     */
    openDefinition(detail) {
      post({
        type: "openDefinition",
        filePath: detail.filePath != null ? String(detail.filePath) : null,
        callPath: detail.callPath != null ? String(detail.callPath) : null,
        line: detail.line ?? null,
        byteStart: detail.byteStart ?? null,
        byteEnd: detail.byteEnd ?? null,
      });
    },

    /**
     * Ask the host what rust-analyzer knows about the symbol at a byte offset
     * in a workspace file. Resolves to { contents?: string[] (markdown),
     * error?: string }.
     * @param {{filePath: string, byteOffset: number}} req
     * @param {number} [timeoutMs]
     * @returns {Promise<object>}
     */
    requestHover(req, timeoutMs = 10000) {
      const requestId = `hov-${Date.now()}-${Math.random().toString(36).slice(2, 9)}`;
      return new Promise((resolve, reject) => {
        const timer = setTimeout(() => {
          pendingHover.delete(requestId);
          reject(new Error("hover request timed out"));
        }, timeoutMs);
        pendingHover.set(requestId, { resolve, reject, timer });
        post({
          type: "hoverRequest",
          requestId,
          filePath: String(req.filePath || ""),
          byteOffset: Number(req.byteOffset) || 0,
        });
      });
    },

    /**
     * Ask the host where the definition of the symbol at a byte offset lives,
     * without opening anything. Resolves to { target?: {path, line,
     * byteOffset}, error?: string } — `path` is workspace-relative or null.
     * @param {{filePath: string, byteOffset: number}} req
     * @param {number} [timeoutMs]
     * @returns {Promise<object>}
     */
    requestDefinitionAt(req, timeoutMs = 10000) {
      const requestId = `def-${Date.now()}-${Math.random().toString(36).slice(2, 9)}`;
      return new Promise((resolve, reject) => {
        const timer = setTimeout(() => {
          pendingDefinition.delete(requestId);
          reject(new Error("definition request timed out"));
        }, timeoutMs);
        pendingDefinition.set(requestId, { resolve, reject, timer });
        post({
          type: "definitionAtRequest",
          requestId,
          filePath: String(req.filePath || ""),
          byteOffset: Number(req.byteOffset) || 0,
        });
      });
    },

    /**
     * Ask the host for rust-analyzer semantic tokens covering a byte range of
     * a workspace file. Resolves to { tokens?: {b,l,t,m}[], error?: string }.
     * @param {{filePath: string, byteStart: number, byteEnd: number}} req
     * @param {number} [timeoutMs]
     * @returns {Promise<object>}
     */
    requestSemanticTokens(req, timeoutMs = 15000) {
      const requestId = `sem-${Date.now()}-${Math.random().toString(36).slice(2, 9)}`;
      return new Promise((resolve, reject) => {
        const timer = setTimeout(() => {
          pendingSemantic.delete(requestId);
          reject(new Error("semantic tokens request timed out"));
        }, timeoutMs);
        pendingSemantic.set(requestId, { resolve, reject, timer });
        post({
          type: "semanticTokensRequest",
          requestId,
          filePath: String(req.filePath || ""),
          byteStart: Number(req.byteStart) || 0,
          byteEnd: Number(req.byteEnd) || 0,
        });
      });
    },

    /**
     * Ask host to open a map JSON from disk (IDE file picker).
     */
    openMapJson() {
      post({ type: "openMapJson" });
    },

    /**
     * Ask host to pick a different workspace / analyse root (IDE folder picker).
     */
    chooseFolder() {
      post({ type: "chooseFolder" });
    },

    /**
     * Request a tokenized source slice for the Inspector preview.
     * W3/W4 may fulfill via sourceResult, or leave it unanswered (timeout).
     * Prefer selectFunction → Inspection canvas for rust-analyzer.
     * @param {{path:string, byteStart:number, byteEnd:number, expectedHash:string}} req
     * @param {number} [timeoutMs]
     * @returns {Promise<object>}
     */
    requestSource(req, timeoutMs = 15000) {
      const requestId = `src-${Date.now()}-${Math.random().toString(36).slice(2, 9)}`;
      return new Promise((resolve, reject) => {
        const timer = setTimeout(() => {
          pendingSource.delete(requestId);
          reject(new Error("source request timed out"));
        }, timeoutMs);
        pendingSource.set(requestId, { resolve, reject, timer });
        post({
          type: "sourceRequest",
          requestId,
          path: String(req.path || ""),
          byteStart: Number(req.byteStart) || 0,
          byteEnd: Number(req.byteEnd) || 0,
          expectedHash: String(req.expectedHash || ""),
        });
      });
    },
  };
})();
