/**
 * Local `horizon-server` sidecar client (W2 stub; W4 fleshes out lifecycle).
 *
 * Prefer attaching to an already-running server via `HORIZON_SIDECAR_URL` /
 * `horizon.map.sidecarUrl` (e.g. `http://127.0.0.1:PORT`). Otherwise spawn
 * the Axum binary on loopback. Health-checks `/api/health` and drives
 * `/api/analyse`, `/api/map`, `/api/source`. Never accepts non-localhost URLs.
 */

import { ChildProcess, spawn } from "child_process";
import * as fs from "fs";
import * as path from "path";
import * as vscode from "vscode";
import {
  findBuiltServerBinary,
  findHorizonCargoWorkspace,
} from "./paths";

const LOCAL_HOST_RE = /^https?:\/\/(127\.0\.0\.1|localhost|\[::1\])(:\d+)?\/?$/i;
const HEALTH_TIMEOUT_MS = 120_000;
const ANALYSE_POLL_MS = 400;

export type AnalyseProgress = {
  status: "idle" | "running" | "done" | "failed" | "error";
  path?: string;
  elapsed_ms?: number;
  error?: string;
};

export class HorizonSidecar implements vscode.Disposable {
  private process: ChildProcess | undefined;
  private baseUrl: string | undefined;
  /** True when using an externally started server (do not kill on stop). */
  private attachedExternal = false;
  private starting: Promise<string> | undefined;
  private readonly output: vscode.OutputChannel;
  private disposed = false;

  constructor(
    private readonly extensionPath: string,
    output?: vscode.OutputChannel
  ) {
    this.output = output ?? vscode.window.createOutputChannel("Horizon Sidecar");
  }

  /** Loopback base URL (`http://127.0.0.1:PORT`), attaching or starting as needed. */
  async ensureRunning(): Promise<string> {
    if (this.disposed) {
      throw new Error("sidecar disposed");
    }

    // Prefer an explicit URL (env / setting) — attach without spawning.
    const configured = this.resolveConfiguredUrl();
    if (configured) {
      assertLocalhostUrl(configured);
      if (await this.healthOk(configured)) {
        this.baseUrl = configured.replace(/\/?$/, "");
        this.attachedExternal = true;
        this.log(`attached to ${this.baseUrl}`);
        return this.baseUrl;
      }
      throw new Error(
        `HORIZON_SIDECAR_URL / horizon.map.sidecarUrl is set (${configured}) but /api/health failed`
      );
    }

    if (this.baseUrl) {
      if (await this.healthOk(this.baseUrl)) {
        return this.baseUrl;
      }
      if (this.attachedExternal) {
        throw new Error(`attached sidecar at ${this.baseUrl} is no longer healthy`);
      }
      this.log("health check failed; restarting sidecar");
      await this.stop();
    }

    if (!this.starting) {
      this.starting = this.spawnServer().finally(() => {
        this.starting = undefined;
      });
    }
    return this.starting;
  }

  /**
   * Resolve attach URL from setting or `HORIZON_SIDECAR_URL`.
   * Empty / unset → spawn path.
   */
  private resolveConfiguredUrl(): string | undefined {
    const config = vscode.workspace.getConfiguration("horizon.map");
    const fromConfig = (config.get<string>("sidecarUrl") || "").trim();
    const fromEnv = (process.env.HORIZON_SIDECAR_URL || "").trim();
    const raw = fromConfig || fromEnv;
    if (!raw) {
      return undefined;
    }
    try {
      const u = new URL(raw);
      return `${u.protocol}//${u.host}`;
    } catch {
      throw new Error(`invalid HORIZON_SIDECAR_URL / sidecarUrl: ${raw}`);
    }
  }

  /** POST `/api/analyse` and poll until done/failed; then GET `/api/map`. */
  async analyse(
    repoPath: string,
    onProgress?: (p: AnalyseProgress) => void
  ): Promise<{ map: unknown; path: string }> {
    const root = path.resolve(repoPath);
    if (!fs.existsSync(root) || !fs.statSync(root).isDirectory()) {
      throw new Error(`analyse path is not a directory: ${root}`);
    }

    const base = await this.ensureRunning();
    const post = await this.fetchJson(`${base}/api/analyse`, {
      method: "POST",
      headers: { "Content-Type": "application/json", Accept: "application/json" },
      body: JSON.stringify({ path: root }),
    });

    if (post.status === 409) {
      // Attach to the in-flight job.
      this.log("analyse already running; polling existing job");
    } else if (post.status !== 202 && post.status !== 200) {
      const err =
        typeof post.body?.error === "string"
          ? post.body.error
          : `analyse rejected (${post.status})`;
      throw new Error(err);
    }

    const terminal = await this.pollAnalyse(base, onProgress);
    if (terminal.status === "failed" || terminal.status === "error") {
      throw new Error(terminal.error || "analysis failed");
    }
    if (terminal.status !== "done") {
      throw new Error(`unexpected analyse status: ${terminal.status}`);
    }

    const mapRes = await this.fetchJson(`${base}/api/map`, {
      headers: { Accept: "application/json" },
    });
    if (mapRes.status !== 200) {
      throw new Error(
        typeof mapRes.body?.error === "string"
          ? mapRes.body.error
          : "map not available after analyse"
      );
    }
    return { map: mapRes.body, path: terminal.path || root };
  }

  /** GET `/api/map` when a map is already loaded in the sidecar. */
  async getMap(): Promise<unknown | undefined> {
    const base = await this.ensureRunning();
    const res = await this.fetchJson(`${base}/api/map`, {
      headers: { Accept: "application/json" },
    });
    if (res.status === 404) {
      return undefined;
    }
    if (res.status !== 200) {
      throw new Error(
        typeof res.body?.error === "string" ? res.body.error : "failed to load map"
      );
    }
    return res.body;
  }

  /** POST raw map JSON into the sidecar (e.g. after Open map JSON). */
  async postMap(mapJson: string | Buffer): Promise<unknown> {
    const base = await this.ensureRunning();
    const res = await this.fetchJson(`${base}/api/map`, {
      method: "POST",
      headers: { "Content-Type": "application/json", Accept: "application/json" },
      body: typeof mapJson === "string" ? mapJson : mapJson.toString("utf8"),
    });
    if (res.status !== 200) {
      throw new Error(
        typeof res.body?.error === "string" ? res.body.error : "failed to post map"
      );
    }
    return res.body;
  }

  /** GET `/api/source` for Inspector token preview (optional; inspection uses RA). */
  async getSource(query: {
    path: string;
    byteStart: number;
    byteEnd: number;
    expectedHash: string;
  }): Promise<{ ok: true; tokens: unknown } | { ok: false; error: string; errorKind: string }> {
    const base = await this.ensureRunning();
    const params = new URLSearchParams({
      path: query.path,
      byte_start: String(query.byteStart),
      byte_end: String(query.byteEnd),
      expected_hash: query.expectedHash || "",
    });
    const res = await this.fetchJson(`${base}/api/source?${params}`, {
      headers: { Accept: "application/json" },
    });
    if (res.status === 200 && res.body && Array.isArray(res.body.tokens)) {
      return { ok: true, tokens: res.body.tokens };
    }
    const errorKind =
      typeof res.body?.error === "string" ? res.body.error : "error";
    const message =
      typeof res.body?.message === "string"
        ? res.body.message
        : typeof res.body?.error === "string"
          ? res.body.error
          : `source request failed (${res.status})`;
    return { ok: false, error: message, errorKind };
  }

  async stop(): Promise<void> {
    const proc = this.process;
    const wasExternal = this.attachedExternal;
    this.process = undefined;
    this.baseUrl = undefined;
    this.attachedExternal = false;
    // Never kill a server we only attached to.
    if (wasExternal || !proc || proc.killed) {
      return;
    }
    await new Promise<void>((resolve) => {
      const done = () => resolve();
      proc.once("exit", done);
      proc.kill("SIGTERM");
      setTimeout(() => {
        if (!proc.killed) {
          try {
            proc.kill("SIGKILL");
          } catch {
            /* ignore */
          }
        }
        done();
      }, 3000).unref?.();
    });
  }

  dispose(): void {
    this.disposed = true;
    void this.stop();
    this.output.dispose();
  }

  private async spawnServer(): Promise<string> {
    const { command, args, cwd } = this.resolveLaunch();
    this.log(`starting: ${command} ${args.join(" ")} (cwd=${cwd ?? "default"})`);
    this.attachedExternal = false;

    const child = spawn(command, args, {
      cwd,
      env: { ...process.env },
      stdio: ["ignore", "pipe", "pipe"],
    });
    this.process = child;

    let stdoutBuf = "";
    let stderrBuf = "";

    child.stdout?.on("data", (chunk: Buffer) => {
      const text = chunk.toString("utf8");
      stdoutBuf += text;
      this.output.append(text);
    });
    child.stderr?.on("data", (chunk: Buffer) => {
      const text = chunk.toString("utf8");
      stderrBuf += text;
      this.output.append(text);
    });

    child.on("exit", (code, signal) => {
      this.log(`sidecar exited code=${code} signal=${signal}`);
      if (this.process === child) {
        this.process = undefined;
        this.baseUrl = undefined;
      }
    });

    const url = await this.waitForUrl(() => stdoutBuf, child, stderrBuf);
    assertLocalhostUrl(url);
    await this.waitForHealth(url);
    this.baseUrl = url.replace(/\/?$/, "");
    this.log(`ready at ${this.baseUrl}`);
    return this.baseUrl;
  }

  private resolveLaunch(): { command: string; args: string[]; cwd?: string } {
    const config = vscode.workspace.getConfiguration("horizon.map");
    const configuredBinary = (config.get<string>("serverPath") || "").trim();
    const envBinary = (process.env.HORIZON_SERVER_PATH || "").trim();

    for (const bin of [configuredBinary, envBinary]) {
      if (bin && fs.existsSync(bin)) {
        return { command: bin, args: ["--no-open"] };
      }
    }

    const cargoWs = findHorizonCargoWorkspace(
      this.extensionPath,
      config.get<string>("cargoWorkspace") || ""
    );
    if (cargoWs) {
      const built = findBuiltServerBinary(cargoWs);
      if (built) {
        return { command: built, args: ["--no-open"], cwd: cargoWs };
      }
      return {
        command: "cargo",
        args: ["run", "-p", "horizon-server", "--", "--no-open"],
        cwd: cargoWs,
      };
    }

    throw new Error(
      "Could not locate horizon-server. Set horizon.map.serverPath or horizon.map.cargoWorkspace."
    );
  }

  private waitForUrl(
    getStdout: () => string,
    child: ChildProcess,
    _stderr: string
  ): Promise<string> {
    return new Promise((resolve, reject) => {
      const started = Date.now();
      const timer = setInterval(() => {
        if (this.disposed) {
          clearInterval(timer);
          reject(new Error("sidecar disposed while starting"));
          return;
        }
        if (child.exitCode !== null) {
          clearInterval(timer);
          reject(
            new Error(
              `horizon-server exited before printing URL (code ${child.exitCode})`
            )
          );
          return;
        }
        const match = getStdout().match(/https?:\/\/[^\s]+/);
        if (match) {
          clearInterval(timer);
          resolve(match[0].trim());
          return;
        }
        if (Date.now() - started > HEALTH_TIMEOUT_MS) {
          clearInterval(timer);
          reject(new Error("timed out waiting for horizon-server URL on stdout"));
        }
      }, 100);
    });
  }

  private async waitForHealth(baseUrl: string): Promise<void> {
    const started = Date.now();
    while (Date.now() - started < HEALTH_TIMEOUT_MS) {
      if (await this.healthOk(baseUrl)) {
        return;
      }
      await delay(200);
    }
    throw new Error("timed out waiting for /api/health");
  }

  private async healthOk(baseUrl: string): Promise<boolean> {
    try {
      assertLocalhostUrl(baseUrl);
      const res = await fetch(joinUrl(baseUrl, "/api/health"), {
        headers: { Accept: "application/json" },
      });
      if (!res.ok) {
        return false;
      }
      const body = (await res.json()) as { ok?: boolean };
      return body.ok === true;
    } catch {
      return false;
    }
  }

  private async pollAnalyse(
    base: string,
    onProgress?: (p: AnalyseProgress) => void
  ): Promise<AnalyseProgress> {
    for (;;) {
      const res = await this.fetchJson(`${base}/api/analyse`, {
        headers: { Accept: "application/json" },
      });
      const body = res.body || {};
      const status = String(body.status || "error") as AnalyseProgress["status"];
      const progress: AnalyseProgress = {
        status: ["idle", "running", "done", "failed", "error"].includes(status)
          ? status
          : "error",
        path: typeof body.path === "string" ? body.path : undefined,
        elapsed_ms:
          typeof body.elapsed_ms === "number" ? body.elapsed_ms : undefined,
        error: typeof body.error === "string" ? body.error : undefined,
      };
      onProgress?.(progress);
      if (progress.status !== "running") {
        return progress;
      }
      await delay(ANALYSE_POLL_MS);
    }
  }

  private async fetchJson(
    url: string,
    init?: RequestInit
  ): Promise<{ status: number; body: any }> {
    assertLocalhostUrl(url);
    const res = await fetch(url, init);
    let body: any = undefined;
    const text = await res.text();
    if (text) {
      try {
        body = JSON.parse(text);
      } catch {
        body = { error: text };
      }
    }
    return { status: res.status, body };
  }

  private log(message: string): void {
    this.output.appendLine(`[sidecar] ${message}`);
  }
}

function assertLocalhostUrl(url: string): void {
  // Allow path/query after origin for API calls.
  let origin: string;
  try {
    const u = new URL(url);
    origin = `${u.protocol}//${u.host}`;
  } catch {
    throw new Error(`invalid sidecar URL: ${url}`);
  }
  if (!LOCAL_HOST_RE.test(origin) && !LOCAL_HOST_RE.test(origin + "/")) {
    throw new Error(`refusing non-localhost sidecar URL: ${origin}`);
  }
  if (origin.startsWith("https://") === false && !origin.startsWith("http://")) {
    throw new Error(`refusing non-http sidecar URL: ${origin}`);
  }
}

function joinUrl(base: string, route: string): string {
  return `${base.replace(/\/?$/, "")}${route.startsWith("/") ? route : `/${route}`}`;
}

function delay(ms: number): Promise<void> {
  return new Promise((resolve) => setTimeout(resolve, ms));
}
