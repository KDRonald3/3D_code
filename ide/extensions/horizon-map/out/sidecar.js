"use strict";
/**
 * Local `horizon-server` sidecar client (W4).
 *
 * Prefer attaching via `HORIZON_SIDECAR_URL` / `horizon.map.sidecarUrl`
 * (`http://127.0.0.1:PORT`). Otherwise spawn the Axum binary on loopback:
 * configured path → `target/release/horizon-server` → PATH →
 * `cargo run -p horizon-server -- --no-open`.
 *
 * Health-checks `/api/health` and drives `/api/analyse`, `/api/map`,
 * `/api/source`. Never accepts non-localhost URLs.
 */
var __createBinding = (this && this.__createBinding) || (Object.create ? (function(o, m, k, k2) {
    if (k2 === undefined) k2 = k;
    var desc = Object.getOwnPropertyDescriptor(m, k);
    if (!desc || ("get" in desc ? !m.__esModule : desc.writable || desc.configurable)) {
      desc = { enumerable: true, get: function() { return m[k]; } };
    }
    Object.defineProperty(o, k2, desc);
}) : (function(o, m, k, k2) {
    if (k2 === undefined) k2 = k;
    o[k2] = m[k];
}));
var __setModuleDefault = (this && this.__setModuleDefault) || (Object.create ? (function(o, v) {
    Object.defineProperty(o, "default", { enumerable: true, value: v });
}) : function(o, v) {
    o["default"] = v;
});
var __importStar = (this && this.__importStar) || (function () {
    var ownKeys = function(o) {
        ownKeys = Object.getOwnPropertyNames || function (o) {
            var ar = [];
            for (var k in o) if (Object.prototype.hasOwnProperty.call(o, k)) ar[ar.length] = k;
            return ar;
        };
        return ownKeys(o);
    };
    return function (mod) {
        if (mod && mod.__esModule) return mod;
        var result = {};
        if (mod != null) for (var k = ownKeys(mod), i = 0; i < k.length; i++) if (k[i] !== "default") __createBinding(result, mod, k[i]);
        __setModuleDefault(result, mod);
        return result;
    };
})();
Object.defineProperty(exports, "__esModule", { value: true });
exports.HorizonSidecar = void 0;
const child_process_1 = require("child_process");
const fs = __importStar(require("fs"));
const path = __importStar(require("path"));
const vscode = __importStar(require("vscode"));
const paths_1 = require("./paths");
const LOCAL_HOST_RE = /^https?:\/\/(127\.0\.0\.1|localhost|\[::1\])(:\d+)?\/?$/i;
/** Match a loopback HTTP URL printed by horizon-server (stdout or stderr). */
const LISTEN_URL_RE = /https?:\/\/(?:127\.0\.0\.1|localhost|\[::1\]):\d+\/?/i;
const HEALTH_TIMEOUT_MS = 120_000;
const ANALYSE_POLL_MS = 400;
const STOP_GRACE_MS = 3_000;
class HorizonSidecar {
    extensionPath;
    process;
    baseUrl;
    /** True when using an externally started server (do not kill on stop). */
    attachedExternal = false;
    starting;
    output;
    /** When true, dispose() owns and disposes the output channel. */
    ownsOutput;
    disposed = false;
    constructor(extensionPath, output) {
        this.extensionPath = extensionPath;
        this.ownsOutput = !output;
        this.output =
            output ?? vscode.window.createOutputChannel("Horizon");
    }
    /** Loopback base URL (`http://127.0.0.1:PORT`), attaching or starting as needed. */
    async ensureRunning() {
        if (this.disposed) {
            throw new Error("Horizon sidecar is disposed");
        }
        // Prefer an explicit URL (env / setting) — attach without spawning.
        const configured = this.resolveConfiguredUrl();
        if (configured) {
            assertLocalhostUrl(configured);
            if (await this.healthOk(configured)) {
                this.baseUrl = stripTrailingSlash(configured);
                this.attachedExternal = true;
                this.log(`attached to ${this.baseUrl}`);
                return this.baseUrl;
            }
            throw new Error(`horizon.map.sidecarUrl / HORIZON_SIDECAR_URL is set (${configured}) ` +
                `but GET /api/health failed. Start the server (./ide/scripts/run-sidecar.sh) ` +
                `or clear the setting to let the extension spawn one.`);
        }
        if (this.baseUrl) {
            if (await this.healthOk(this.baseUrl)) {
                return this.baseUrl;
            }
            if (this.attachedExternal) {
                throw new Error(`Attached sidecar at ${this.baseUrl} is no longer healthy. ` +
                    `Restart it or clear horizon.map.sidecarUrl.`);
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
    resolveConfiguredUrl() {
        const config = vscode.workspace.getConfiguration("horizon.map");
        const fromConfig = (config.get("sidecarUrl") || "").trim();
        const fromEnv = (process.env.HORIZON_SIDECAR_URL || "").trim();
        const raw = fromConfig || fromEnv;
        if (!raw) {
            return undefined;
        }
        try {
            const u = new URL(raw);
            return `${u.protocol}//${u.host}`;
        }
        catch {
            throw new Error(`Invalid horizon.map.sidecarUrl / HORIZON_SIDECAR_URL: ${raw}`);
        }
    }
    /**
     * POST `/api/analyse` and poll until done/failed; then GET `/api/map`.
     * When `workspaceRoot` is set, `repoPath` must resolve under that root.
     */
    async analyse(repoPath, onProgress, workspaceRoot) {
        const root = this.validateAnalysePath(repoPath, workspaceRoot);
        const base = await this.ensureRunning();
        this.log(`POST /api/analyse path=${root}`);
        const post = await this.fetchJson(`${base}/api/analyse`, {
            method: "POST",
            headers: { "Content-Type": "application/json", Accept: "application/json" },
            body: JSON.stringify({ path: root }),
        });
        if (post.status === 409) {
            // Attach to the in-flight job.
            this.log("analyse already running; polling existing job");
        }
        else if (post.status !== 202 && post.status !== 200) {
            const err = typeof post.body?.error === "string"
                ? post.body.error
                : `analyse rejected (HTTP ${post.status})`;
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
            throw new Error(typeof mapRes.body?.error === "string"
                ? mapRes.body.error
                : "map not available after analyse");
        }
        return { map: mapRes.body, path: terminal.path || root };
    }
    /** Resolve and sanity-check an analyse target directory. */
    validateAnalysePath(repoPath, workspaceRoot) {
        const trimmed = (repoPath || "").trim();
        if (!trimmed) {
            throw new Error("analyse path is empty");
        }
        let root;
        if (workspaceRoot) {
            const ws = path.resolve(workspaceRoot);
            const candidate = path.isAbsolute(trimmed)
                ? path.resolve(trimmed)
                : undefined;
            if (candidate && (candidate === ws || candidate.startsWith(ws + path.sep))) {
                root = candidate;
            }
            else {
                const under = (0, paths_1.resolveUnderRoot)(ws, trimmed);
                if (!under) {
                    throw new Error(`Analyse path must stay under the workspace folder (${ws}): ${trimmed}`);
                }
                root = under;
            }
        }
        else {
            root = path.resolve(trimmed);
        }
        if (!fs.existsSync(root) || !fs.statSync(root).isDirectory()) {
            throw new Error(`analyse path is not a directory: ${root}`);
        }
        return root;
    }
    /** GET `/api/map` when a map is already loaded in the sidecar. */
    async getMap() {
        const base = await this.ensureRunning();
        const res = await this.fetchJson(`${base}/api/map`, {
            headers: { Accept: "application/json" },
        });
        if (res.status === 404) {
            return undefined;
        }
        if (res.status !== 200) {
            throw new Error(typeof res.body?.error === "string" ? res.body.error : "failed to load map");
        }
        return res.body;
    }
    /** POST raw map JSON into the sidecar (e.g. after Open map JSON). */
    async postMap(mapJson) {
        const base = await this.ensureRunning();
        const res = await this.fetchJson(`${base}/api/map`, {
            method: "POST",
            headers: { "Content-Type": "application/json", Accept: "application/json" },
            body: typeof mapJson === "string" ? mapJson : mapJson.toString("utf8"),
        });
        if (res.status !== 200) {
            throw new Error(typeof res.body?.error === "string" ? res.body.error : "failed to post map");
        }
        return res.body;
    }
    /** GET `/api/source` for Inspector token preview (optional; inspection uses RA). */
    async getSource(query) {
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
        const errorKind = typeof res.body?.error === "string" ? res.body.error : "error";
        const message = typeof res.body?.message === "string"
            ? res.body.message
            : typeof res.body?.error === "string"
                ? res.body.error
                : `source request failed (${res.status})`;
        return { ok: false, error: message, errorKind };
    }
    async stop() {
        const proc = this.process;
        const wasExternal = this.attachedExternal;
        this.process = undefined;
        this.baseUrl = undefined;
        this.attachedExternal = false;
        // Never kill a server we only attached to.
        if (wasExternal || !proc || proc.killed) {
            return;
        }
        this.log("stopping sidecar process");
        await killSidecarProcess(proc);
    }
    dispose() {
        this.disposed = true;
        void this.stop();
        if (this.ownsOutput) {
            this.output.dispose();
        }
    }
    async spawnServer() {
        const { command, args, cwd, label } = this.resolveLaunch();
        this.log(`starting (${label}): ${command} ${args.join(" ")} (cwd=${cwd ?? "default"})`);
        this.attachedExternal = false;
        const child = (0, child_process_1.spawn)(command, args, {
            cwd,
            env: {
                ...process.env,
                // Quiet cargo progress noise when falling back to `cargo run`.
                CARGO_TERM_PROGRESS_WHEN: "never",
            },
            stdio: ["ignore", "pipe", "pipe"],
            // Own process group on Unix so SIGTERM reaches `cargo run` children.
            detached: process.platform !== "win32",
        });
        this.process = child;
        let combined = "";
        const onChunk = (chunk) => {
            const text = chunk.toString("utf8");
            combined += text;
            this.output.append(text);
        };
        child.stdout?.on("data", onChunk);
        child.stderr?.on("data", onChunk);
        child.on("error", (err) => {
            this.log(`spawn error: ${err.message}`);
        });
        child.on("exit", (code, signal) => {
            this.log(`sidecar exited code=${code} signal=${signal}`);
            if (this.process === child) {
                this.process = undefined;
                this.baseUrl = undefined;
            }
        });
        let url;
        try {
            url = await this.waitForUrl(() => combined, child);
        }
        catch (err) {
            await killSidecarProcess(child);
            if (this.process === child) {
                this.process = undefined;
            }
            const hint = formatSpawnFailure(command, args, cwd, combined);
            const message = err instanceof Error ? err.message : String(err);
            throw new Error(`${message}. ${hint}`);
        }
        assertLocalhostUrl(url);
        try {
            await this.waitForHealth(url);
        }
        catch (err) {
            await killSidecarProcess(child);
            if (this.process === child) {
                this.process = undefined;
            }
            throw err;
        }
        this.baseUrl = stripTrailingSlash(url);
        this.log(`ready at ${this.baseUrl}`);
        return this.baseUrl;
    }
    resolveLaunch() {
        const config = vscode.workspace.getConfiguration("horizon.map");
        const configuredBinary = (config.get("serverPath") || "").trim();
        const envBinary = (process.env.HORIZON_SERVER_PATH || "").trim();
        for (const bin of [configuredBinary, envBinary]) {
            if (!bin) {
                continue;
            }
            if (fs.existsSync(bin) && isExecutableFile(bin)) {
                return {
                    command: bin,
                    args: ["--no-open"],
                    label: "serverPath",
                };
            }
            if (bin) {
                this.log(`configured serverPath not found or not executable: ${bin}`);
            }
        }
        const cargoWs = (0, paths_1.findHorizonCargoWorkspace)(this.extensionPath, config.get("cargoWorkspace") || "");
        if (cargoWs) {
            // Prefer release; fall back to debug if already built.
            const built = (0, paths_1.findBuiltServerBinary)(cargoWs);
            if (built) {
                return {
                    command: built,
                    args: ["--no-open"],
                    cwd: cargoWs,
                    label: built.includes(`${path.sep}release${path.sep}`)
                        ? "target/release"
                        : "target/debug",
                };
            }
        }
        const onPath = (0, paths_1.findOnPath)("horizon-server");
        if (onPath) {
            return {
                command: onPath,
                args: ["--no-open"],
                label: "PATH",
            };
        }
        if (cargoWs) {
            return {
                command: "cargo",
                args: ["run", "-p", "horizon-server", "--", "--no-open"],
                cwd: cargoWs,
                label: "cargo run",
            };
        }
        throw new Error("Could not locate horizon-server. Build it (`cargo build -p horizon-server --release`), " +
            "put it on PATH, or set horizon.map.serverPath / horizon.map.cargoWorkspace. " +
            "For manual testing: ./ide/scripts/run-sidecar.sh");
    }
    waitForUrl(getCombined, child) {
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
                    reject(new Error(`horizon-server exited before printing a listen URL (code ${child.exitCode})`));
                    return;
                }
                const match = getCombined().match(LISTEN_URL_RE);
                if (match) {
                    clearInterval(timer);
                    resolve(match[0].trim());
                    return;
                }
                if (Date.now() - started > HEALTH_TIMEOUT_MS) {
                    clearInterval(timer);
                    reject(new Error("timed out waiting for horizon-server listen URL on stdout/stderr"));
                }
            }, 100);
        });
    }
    async waitForHealth(baseUrl) {
        const started = Date.now();
        while (Date.now() - started < HEALTH_TIMEOUT_MS) {
            if (this.disposed) {
                throw new Error("sidecar disposed while waiting for /api/health");
            }
            if (await this.healthOk(baseUrl)) {
                return;
            }
            await delay(200);
        }
        throw new Error(`timed out waiting for GET /api/health at ${stripTrailingSlash(baseUrl)}`);
    }
    async healthOk(baseUrl) {
        try {
            assertLocalhostUrl(baseUrl);
            const controller = new AbortController();
            const timer = setTimeout(() => controller.abort(), 2_000);
            try {
                const res = await fetch(joinUrl(baseUrl, "/api/health"), {
                    headers: { Accept: "application/json" },
                    signal: controller.signal,
                });
                if (!res.ok) {
                    return false;
                }
                const body = (await res.json());
                return body.ok === true;
            }
            finally {
                clearTimeout(timer);
            }
        }
        catch {
            return false;
        }
    }
    async pollAnalyse(base, onProgress) {
        for (;;) {
            if (this.disposed) {
                throw new Error("sidecar disposed during analyse");
            }
            const res = await this.fetchJson(`${base}/api/analyse`, {
                headers: { Accept: "application/json" },
            });
            const body = res.body || {};
            const status = String(body.status || "error");
            const progress = {
                status: ["idle", "running", "done", "failed", "error"].includes(status)
                    ? status
                    : "error",
                path: typeof body.path === "string" ? body.path : undefined,
                elapsed_ms: typeof body.elapsed_ms === "number" ? body.elapsed_ms : undefined,
                error: typeof body.error === "string" ? body.error : undefined,
            };
            onProgress?.(progress);
            if (progress.status !== "running") {
                return progress;
            }
            await delay(ANALYSE_POLL_MS);
        }
    }
    async fetchJson(url, init) {
        assertLocalhostUrl(url);
        const res = await fetch(url, init);
        let body = undefined;
        const text = await res.text();
        if (text) {
            try {
                body = JSON.parse(text);
            }
            catch {
                body = { error: text };
            }
        }
        return { status: res.status, body };
    }
    log(message) {
        this.output.appendLine(`[sidecar] ${message}`);
    }
}
exports.HorizonSidecar = HorizonSidecar;
/** Kill a spawned sidecar, including the process group when detached. */
function killSidecarProcess(proc) {
    return new Promise((resolve) => {
        if (proc.exitCode !== null || proc.killed) {
            resolve();
            return;
        }
        const done = () => resolve();
        proc.once("exit", done);
        const pid = proc.pid;
        try {
            if (pid && process.platform !== "win32") {
                // Negative PID → process group (spawned with detached: true).
                try {
                    process.kill(-pid, "SIGTERM");
                }
                catch {
                    proc.kill("SIGTERM");
                }
            }
            else {
                proc.kill("SIGTERM");
            }
        }
        catch {
            done();
            return;
        }
        setTimeout(() => {
            if (proc.exitCode !== null) {
                done();
                return;
            }
            try {
                if (pid && process.platform !== "win32") {
                    try {
                        process.kill(-pid, "SIGKILL");
                    }
                    catch {
                        proc.kill("SIGKILL");
                    }
                }
                else if (!proc.killed) {
                    proc.kill("SIGKILL");
                }
            }
            catch {
                /* ignore */
            }
            done();
        }, STOP_GRACE_MS).unref?.();
    });
}
function formatSpawnFailure(command, args, cwd, combined) {
    const tail = combined.trim().split(/\r?\n/).slice(-8).join(" | ");
    const parts = [
        `Launch was: ${command} ${args.join(" ")}`,
        cwd ? `cwd=${cwd}` : undefined,
        tail ? `last output: ${tail}` : "no output captured",
        "See the Horizon output channel for full logs.",
    ];
    return parts.filter(Boolean).join("; ");
}
function isExecutableFile(filePath) {
    try {
        const st = fs.statSync(filePath);
        if (!st.isFile()) {
            return false;
        }
        if (process.platform === "win32") {
            return true;
        }
        // Probe execute bit; fall back to spawnSync --help if needed.
        try {
            fs.accessSync(filePath, fs.constants.X_OK);
            return true;
        }
        catch {
            const probe = (0, child_process_1.spawnSync)(filePath, ["--help"], {
                encoding: "utf8",
                timeout: 3_000,
            });
            return probe.status === 0 || probe.status === null;
        }
    }
    catch {
        return false;
    }
}
function assertLocalhostUrl(url) {
    // Allow path/query after origin for API calls.
    let origin;
    try {
        const u = new URL(url);
        origin = `${u.protocol}//${u.host}`;
    }
    catch {
        throw new Error(`invalid sidecar URL: ${url}`);
    }
    if (!LOCAL_HOST_RE.test(origin) && !LOCAL_HOST_RE.test(origin + "/")) {
        throw new Error(`refusing non-localhost sidecar URL: ${origin} (Horizon only talks to 127.0.0.1 / localhost / ::1)`);
    }
    if (!origin.startsWith("http://") && !origin.startsWith("https://")) {
        throw new Error(`refusing non-http sidecar URL: ${origin}`);
    }
}
function stripTrailingSlash(url) {
    return url.replace(/\/?$/, "");
}
function joinUrl(base, route) {
    return `${stripTrailingSlash(base)}${route.startsWith("/") ? route : `/${route}`}`;
}
function delay(ms) {
    return new Promise((resolve) => setTimeout(resolve, ms));
}
//# sourceMappingURL=sidecar.js.map