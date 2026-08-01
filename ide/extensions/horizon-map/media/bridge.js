/**
 * Horizon Map ↔ extension-host message bridge (VS Code webview).
 *
 * The webview never talks to the sidecar HTTP API directly. All analyse /
 * map / source / inspection traffic goes through acquireVsCodeApi().postMessage.
 *
 * See README.md for the full protocol. W3 owns inspection + sidecar wiring.
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
     * Ask host to open a map JSON from disk (IDE file picker).
     */
    openMapJson() {
      post({ type: "openMapJson" });
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
