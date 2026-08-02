"use strict";
/**
 * Read-only inspection canvas (W3).
 *
 * Opens the real workspace file in a VS Code text editor (not webview Monaco)
 * so rust-analyzer attaches: hover, go-to-def, diagnostics, completions.
 * The editor session is marked read-only — no edits on the canvas.
 *
 * Tab chrome still shows the real filename (required: real `file:` URI for RA).
 * The clear title is `Inspect: <fn>` on the status bar. Completions may open via
 * `editor.action.triggerSuggest`; accepting them cannot mutate a readonly buffer,
 * and a change-guard reverts any leftover edits.
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
exports.InspectionController = exports.TRIGGER_INSPECT_SUGGEST = exports.INSPECTION_ACTIVE_CONTEXT = void 0;
exports.resolveFunctionRange = resolveFunctionRange;
exports.resolveFunctionName = resolveFunctionName;
exports.byteOffsetToPosition = byteOffsetToPosition;
exports.isWorkspaceEditor = isWorkspaceEditor;
const crypto = __importStar(require("crypto"));
const fs = __importStar(require("fs"));
const path = __importStar(require("path"));
const vscode = __importStar(require("vscode"));
const paths_1 = require("./paths");
/** View column used for inspection (beside the active / map group). */
const INSPECTION_COLUMN = vscode.ViewColumn.Beside;
/** Context key: an inspection editor is the active text editor. */
exports.INSPECTION_ACTIVE_CONTEXT = "horizon.inspection.active";
/** Command: trigger suggest in the inspection editor (informational only). */
exports.TRIGGER_INSPECT_SUGGEST = "horizon.inspection.triggerSuggest";
/**
 * Owns the inspection editor lifecycle: open, title, readonly, edit guard.
 */
class InspectionController {
    session;
    status;
    disposables = [];
    /** Document versions we last observed — used to undo sneaky edits. */
    lockedVersions = new Map();
    undoing = false;
    constructor() {
        this.status = vscode.window.createStatusBarItem(vscode.StatusBarAlignment.Right, 100);
        this.status.command = exports.TRIGGER_INSPECT_SUGGEST;
        this.status.tooltip =
            "Horizon Inspection (read-only). Click to trigger completions (informational).";
        this.disposables.push(this.status);
        this.disposables.push(vscode.window.onDidChangeActiveTextEditor((editor) => {
            void this.syncActiveContext(editor);
        }), vscode.workspace.onDidChangeTextDocument((e) => {
            void this.onDocumentChanged(e);
        }), vscode.commands.registerCommand(exports.TRIGGER_INSPECT_SUGGEST, () => this.triggerSuggest()));
        void this.syncActiveContext(vscode.window.activeTextEditor);
    }
    /** Current inspection session, if any. */
    get current() {
        return this.session;
    }
    /**
     * Open / reveal a free function in a read-only editor on the real file URI.
     */
    async openInspection(payload) {
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
        const functionName = resolveFunctionName(payload);
        const viewColumn = this.pickInspectionColumn();
        const editor = await vscode.window.showTextDocument(document, {
            viewColumn,
            preview: false,
            preserveFocus: false,
            selection: range,
        });
        if (range) {
            editor.revealRange(range, vscode.TextEditorRevealType.InCenter);
            // Cursor at fn start so hover / suggest / go-to-def target the item;
            // full range stays revealed for orientation.
            editor.selection = new vscode.Selection(range.start, range.start);
        }
        await setEditorReadonlyInSession();
        this.lockDocument(document);
        this.session = {
            uri,
            functionId: payload.functionId,
            functionName,
            range,
            viewColumn: editor.viewColumn,
        };
        this.updateStatus(functionName);
        await this.syncActiveContext(editor);
    }
    /**
     * Open a file read-only without a function range (file-card selection).
     */
    async openFileReadonly(filePath) {
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
        const editor = await vscode.window.showTextDocument(document, {
            viewColumn: this.pickInspectionColumn(),
            preview: true,
        });
        await setEditorReadonlyInSession();
        this.lockDocument(document);
        const label = path.basename(resolved);
        this.session = {
            uri: document.uri,
            functionId: null,
            functionName: label,
            range: undefined,
            viewColumn: editor.viewColumn,
        };
        this.updateStatus(label);
        await this.syncActiveContext(editor);
    }
    /**
     * Trigger the suggest widget in the active inspection editor.
     * Read-only buffers still allow the widget; accepting a completion cannot
     * write when the session is readonly (and the edit guard rejects leftovers).
     */
    async triggerSuggest() {
        const editor = vscode.window.activeTextEditor;
        if (!editor || !this.isInspectionUri(editor.document.uri)) {
            void vscode.window.showInformationMessage("Horizon: focus an Inspection editor first, then trigger completions.");
            return;
        }
        // Re-assert readonly before suggest so accept cannot apply.
        await setEditorReadonlyInSession();
        await vscode.commands.executeCommand("editor.action.triggerSuggest");
    }
    /** True when `uri` is the current inspection document. */
    isInspectionUri(uri) {
        return !!this.session && uri.toString() === this.session.uri.toString();
    }
    dispose() {
        for (const d of this.disposables) {
            d.dispose();
        }
        this.status.dispose();
        this.session = undefined;
        this.lockedVersions.clear();
    }
    pickInspectionColumn() {
        if (this.session?.viewColumn) {
            return this.session.viewColumn;
        }
        const existing = vscode.window.visibleTextEditors.find((e) => this.session && e.document.uri.toString() === this.session.uri.toString());
        if (existing?.viewColumn) {
            return existing.viewColumn;
        }
        return INSPECTION_COLUMN;
    }
    lockDocument(document) {
        this.lockedVersions.set(document.uri.toString(), document.version);
    }
    updateStatus(functionName) {
        this.status.text = `$(lock) Inspect: ${functionName}`;
        this.status.show();
    }
    async syncActiveContext(editor) {
        const active = !!(editor && this.isInspectionUri(editor.document.uri));
        await vscode.commands.executeCommand("setContext", exports.INSPECTION_ACTIVE_CONTEXT, active);
        if (active && this.session) {
            this.updateStatus(this.session.functionName);
            // Re-assert session readonly whenever inspection regains focus.
            await setEditorReadonlyInSession();
        }
        else if (!this.session) {
            this.status.hide();
        }
    }
    /**
     * If the inspection buffer somehow receives an edit (readonly failed),
     * revert it so completions / typing cannot mutate the canvas.
     */
    async onDocumentChanged(e) {
        if (this.undoing || e.contentChanges.length === 0) {
            return;
        }
        if (!this.isInspectionUri(e.document.uri)) {
            return;
        }
        const key = e.document.uri.toString();
        const locked = this.lockedVersions.get(key);
        if (locked === undefined) {
            return;
        }
        if (e.document.version <= locked) {
            return;
        }
        this.undoing = true;
        try {
            await vscode.commands.executeCommand("undo");
            if (e.document.isDirty) {
                await vscode.commands.executeCommand("workbench.action.files.revert");
            }
            this.lockDocument(e.document);
            void vscode.window.showWarningMessage("Horizon Inspection is read-only — edits were discarded.");
        }
        catch (err) {
            console.warn("[Horizon] failed to revert inspection edit", err);
        }
        finally {
            this.undoing = false;
            if (vscode.window.activeTextEditor?.document.uri.toString() === key) {
                await setEditorReadonlyInSession();
            }
        }
    }
}
exports.InspectionController = InspectionController;
async function setEditorReadonlyInSession() {
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
/** Display name for status / tab label from payload or FunctionId. */
function resolveFunctionName(payload) {
    const named = payload.functionName?.trim();
    if (named) {
        return named;
    }
    const id = payload.functionId?.trim();
    if (id) {
        const parts = id.split("::").filter(Boolean);
        const last = parts[parts.length - 1];
        if (last) {
            return last;
        }
    }
    if (payload.filePath) {
        return path.basename(payload.filePath);
    }
    return "function";
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