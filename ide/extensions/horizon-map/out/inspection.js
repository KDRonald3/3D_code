"use strict";
/**
 * Read-only inspection canvas.
 *
 * Opens the real workspace file in a VS Code text editor (not webview Monaco)
 * so rust-analyzer attaches: hover, go-to-def, diagnostics, completions.
 * The editor session is marked read-only — no edits on the canvas.
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
exports.openInspection = openInspection;
exports.openFileReadonly = openFileReadonly;
exports.resolveFunctionRange = resolveFunctionRange;
exports.byteOffsetToPosition = byteOffsetToPosition;
exports.isWorkspaceEditor = isWorkspaceEditor;
const crypto = __importStar(require("crypto"));
const fs = __importStar(require("fs"));
const vscode = __importStar(require("vscode"));
const paths_1 = require("./paths");
/** View column used for inspection (beside the active group). */
const INSPECTION_COLUMN = vscode.ViewColumn.Beside;
/**
 * Open / reveal a free function in a read-only editor on the real file URI.
 */
async function openInspection(payload) {
    const root = (0, paths_1.workspaceRootFsPath)();
    if (!root) {
        void vscode.window.showErrorMessage("Horizon: open a workspace folder before inspecting a function.");
        return;
    }
    const filePath = payload.filePath;
    if (!filePath) {
        void vscode.window.showWarningMessage("Horizon: selectFunction payload has no filePath.");
        return;
    }
    const resolved = (0, paths_1.resolveUnderRoot)(root, filePath);
    if (!resolved) {
        void vscode.window.showErrorMessage(`Horizon: refusing path outside workspace: ${filePath}`);
        return;
    }
    if (!(0, paths_1.isExistingFile)(resolved)) {
        void vscode.window.showErrorMessage(`Horizon: file not found: ${resolved}`);
        return;
    }
    if (payload.contentHash) {
        const stale = await contentHashMismatch(resolved, payload.contentHash);
        if (stale) {
            void vscode.window.showWarningMessage("Horizon: file changed since analysis — offsets may be wrong. Re-analyse the workspace.");
        }
    }
    const uri = vscode.Uri.file(resolved);
    const document = await vscode.workspace.openTextDocument(uri);
    // Ensure Rust language so rust-analyzer binds (*.rs is usually enough;
    // force the id when the file has no extension / wrong association).
    if (document.languageId !== "rust" && resolved.endsWith(".rs")) {
        await vscode.languages.setTextDocumentLanguage(document, "rust");
    }
    const range = resolveFunctionRange(document, resolved, payload);
    const editor = await vscode.window.showTextDocument(document, {
        viewColumn: INSPECTION_COLUMN,
        preview: false,
        preserveFocus: false,
        selection: range,
    });
    if (range) {
        editor.revealRange(range, vscode.TextEditorRevealType.InCenter);
        editor.selection = new vscode.Selection(range.start, range.end);
    }
    await setActiveEditorReadonly();
}
/**
 * Open a file read-only without a function range (file-card selection).
 */
async function openFileReadonly(filePath) {
    const root = (0, paths_1.workspaceRootFsPath)();
    if (!root) {
        return;
    }
    const resolved = (0, paths_1.resolveUnderRoot)(root, filePath);
    if (!resolved || !(0, paths_1.isExistingFile)(resolved)) {
        void vscode.window.showErrorMessage(`Horizon: cannot open file: ${filePath}`);
        return;
    }
    const document = await vscode.workspace.openTextDocument(vscode.Uri.file(resolved));
    if (document.languageId !== "rust" && resolved.endsWith(".rs")) {
        await vscode.languages.setTextDocumentLanguage(document, "rust");
    }
    await vscode.window.showTextDocument(document, {
        viewColumn: INSPECTION_COLUMN,
        preview: true,
    });
    await setActiveEditorReadonly();
}
async function setActiveEditorReadonly() {
    try {
        await vscode.commands.executeCommand("workbench.action.files.setActiveEditorReadonlyInSession");
    }
    catch (err) {
        console.warn("[Horizon] setActiveEditorReadonlyInSession failed; inspection may be editable", err);
    }
}
/**
 * Map UTF-8 byte offsets (from horizon-map Function nodes) to document Positions.
 * Prefer byte range when present and non-sentinel; fall back to 1-based `line`.
 */
function resolveFunctionRange(document, filePath, payload) {
    const byteStart = payload.byteStart;
    const byteEnd = payload.byteEnd;
    const hasBytes = typeof byteStart === "number" &&
        typeof byteEnd === "number" &&
        !(byteStart === 0 && byteEnd === 0) &&
        byteEnd > byteStart;
    if (hasBytes) {
        try {
            const raw = fs.readFileSync(filePath);
            const start = byteOffsetToPosition(raw, byteStart);
            const end = byteOffsetToPosition(raw, byteEnd);
            const startPos = clampPosition(document, start);
            const endPos = clampPosition(document, end);
            if (startPos.isBeforeOrEqual(endPos)) {
                return new vscode.Range(startPos, endPos);
            }
        }
        catch (err) {
            console.warn("[Horizon] byte→position conversion failed", err);
        }
    }
    if (typeof payload.line === "number" && payload.line > 0) {
        const lineIdx = Math.min(payload.line - 1, document.lineCount - 1);
        const line = document.lineAt(lineIdx);
        return line.range;
    }
    return undefined;
}
/**
 * Convert a UTF-8 byte offset into a (line, UTF-16 character) position using
 * raw file bytes — matching how horizon-map stores `byte_start` / `byte_end`.
 */
function byteOffsetToPosition(raw, byteOffset) {
    const clamped = Math.max(0, Math.min(byteOffset, raw.length));
    let line = 0;
    let lineStart = 0;
    for (let i = 0; i < clamped; i++) {
        if (raw[i] === 0x0a) {
            line++;
            lineStart = i + 1;
        }
    }
    const lineBytes = raw.subarray(lineStart, clamped);
    // JS string length is UTF-16 code units — what VS Code Position expects.
    const character = lineBytes.toString("utf8").length;
    return new vscode.Position(line, character);
}
function clampPosition(document, pos) {
    const line = Math.max(0, Math.min(pos.line, document.lineCount - 1));
    const maxChar = document.lineAt(line).text.length;
    const character = Math.max(0, Math.min(pos.character, maxChar));
    return new vscode.Position(line, character);
}
async function contentHashMismatch(filePath, expectedHex) {
    const expected = expectedHex.trim().toLowerCase();
    if (!expected || expected.length !== 64) {
        return false;
    }
    try {
        const buf = await fs.promises.readFile(filePath);
        const actual = crypto.createHash("sha256").update(buf).digest("hex");
        return actual !== expected;
    }
    catch {
        return true;
    }
}
/** True when the given editor is inside the active workspace (classic or inspection). */
function isWorkspaceEditor(editor) {
    if (!editor) {
        return false;
    }
    const folder = (0, paths_1.activeWorkspaceFolder)();
    if (!folder) {
        return false;
    }
    return editor.document.uri.scheme === "file";
}
//# sourceMappingURL=inspection.js.map