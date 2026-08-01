"use strict";
/**
 * Path helpers — keep inspection / analyse targets inside the workspace.
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
exports.activeWorkspaceFolder = activeWorkspaceFolder;
exports.workspaceRootFsPath = workspaceRootFsPath;
exports.resolveUnderRoot = resolveUnderRoot;
exports.isExistingFile = isExistingFile;
exports.findHorizonCargoWorkspace = findHorizonCargoWorkspace;
exports.findBuiltServerBinary = findBuiltServerBinary;
exports.findOnPath = findOnPath;
const fs = __importStar(require("fs"));
const path = __importStar(require("path"));
const vscode = __importStar(require("vscode"));
/** Active workspace folder, or `undefined` when none is open. */
function activeWorkspaceFolder() {
    const editor = vscode.window.activeTextEditor;
    if (editor) {
        const folder = vscode.workspace.getWorkspaceFolder(editor.document.uri);
        if (folder) {
            return folder;
        }
    }
    return vscode.workspace.workspaceFolders?.[0];
}
/** Absolute fs path of the active workspace folder. */
function workspaceRootFsPath() {
    return activeWorkspaceFolder()?.uri.fsPath;
}
/**
 * Resolve `candidate` to an absolute path and ensure it stays under `root`.
 * Rejects `..` escapes and paths outside the workspace.
 */
function resolveUnderRoot(root, candidate) {
    if (!candidate || !root) {
        return undefined;
    }
    const rootResolved = path.resolve(root);
    const abs = path.isAbsolute(candidate)
        ? path.resolve(candidate)
        : path.resolve(rootResolved, candidate);
    const rel = path.relative(rootResolved, abs);
    if (rel.startsWith("..") || path.isAbsolute(rel)) {
        return undefined;
    }
    return abs;
}
/** True when `filePath` exists and is a regular file. */
function isExistingFile(filePath) {
    try {
        return fs.statSync(filePath).isFile();
    }
    catch {
        return false;
    }
}
/**
 * Locate the Horizon Cargo workspace that contains `horizon-server`.
 * Prefer configuration, then walk up from the extension install path.
 */
function findHorizonCargoWorkspace(extensionPath, configured) {
    if (configured && configured.trim()) {
        const p = path.resolve(configured.trim());
        if (isHorizonWorkspace(p)) {
            return p;
        }
    }
    let dir = path.resolve(extensionPath);
    for (let i = 0; i < 8; i++) {
        if (isHorizonWorkspace(dir)) {
            return dir;
        }
        const parent = path.dirname(dir);
        if (parent === dir) {
            break;
        }
        dir = parent;
    }
    return undefined;
}
function isHorizonWorkspace(dir) {
    const cargo = path.join(dir, "Cargo.toml");
    if (!fs.existsSync(cargo)) {
        return false;
    }
    try {
        const text = fs.readFileSync(cargo, "utf8");
        return (text.includes("horizon-server") ||
            fs.existsSync(path.join(dir, "crates", "horizon-server", "Cargo.toml")));
    }
    catch {
        return false;
    }
}
/**
 * Prefer release, then debug, built binaries under a cargo workspace.
 * W4 prefers `target/release/horizon-server` when present.
 */
function findBuiltServerBinary(cargoWorkspace) {
    const candidates = [
        path.join(cargoWorkspace, "target", "release", "horizon-server"),
        path.join(cargoWorkspace, "target", "debug", "horizon-server"),
    ];
    for (const c of candidates) {
        try {
            if (fs.existsSync(c) && fs.statSync(c).isFile()) {
                return c;
            }
        }
        catch {
            /* ignore */
        }
    }
    return undefined;
}
/** Absolute path to an executable on PATH, or `undefined`. */
function findOnPath(binary) {
    if (!binary || binary.includes("/") || binary.includes("\\")) {
        return undefined;
    }
    const pathEnv = process.env.PATH || "";
    const sep = process.platform === "win32" ? ";" : ":";
    const exts = process.platform === "win32"
        ? (process.env.PATHEXT || ".EXE;.CMD;.BAT").split(";").filter(Boolean)
        : [""];
    for (const dir of pathEnv.split(sep)) {
        if (!dir) {
            continue;
        }
        for (const ext of exts) {
            const candidate = path.join(dir, binary + ext);
            try {
                if (fs.existsSync(candidate) && fs.statSync(candidate).isFile()) {
                    if (process.platform === "win32") {
                        return candidate;
                    }
                    try {
                        fs.accessSync(candidate, fs.constants.X_OK);
                        return candidate;
                    }
                    catch {
                        /* not executable */
                    }
                }
            }
            catch {
                /* ignore */
            }
        }
    }
    return undefined;
}
//# sourceMappingURL=paths.js.map