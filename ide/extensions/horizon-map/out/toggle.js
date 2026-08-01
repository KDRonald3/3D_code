"use strict";
/**
 * Map ↔ Classic editor toggle.
 *
 * Shows / focuses the Horizon Map webview view, or leaves the map and focuses
 * a classic (non-webview) text editor group.
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
exports.MapToggle = exports.MAP_VISIBLE_CONTEXT = void 0;
const vscode = __importStar(require("vscode"));
const MAP_VIEW_FOCUS = "horizon.map.view.focus";
const FOCUS_EDITOR = "workbench.action.focusActiveEditorGroup";
/** Context key mirrored for when-clauses / debugging. */
exports.MAP_VISIBLE_CONTEXT = "horizon.map.visible";
class MapToggle {
    mapVisible = false;
    disposables = [];
    constructor() {
        void vscode.commands.executeCommand("setContext", exports.MAP_VISIBLE_CONTEXT, false);
    }
    /** Whether the map side is considered active after the last toggle/show. */
    get isMapVisible() {
        return this.mapVisible;
    }
    /** Show and focus the Horizon Map view. */
    async showMap() {
        try {
            await vscode.commands.executeCommand(MAP_VIEW_FOCUS);
        }
        catch {
            // Fallback: reveal the activity-bar container.
            await vscode.commands.executeCommand("workbench.view.extension.horizon");
        }
        this.mapVisible = true;
        await vscode.commands.executeCommand("setContext", exports.MAP_VISIBLE_CONTEXT, true);
    }
    /**
     * Leave the map and focus the classic editor.
     * Prefers an existing text editor tab over the map webview.
     */
    async showClassic() {
        this.mapVisible = false;
        await vscode.commands.executeCommand("setContext", exports.MAP_VISIBLE_CONTEXT, false);
        const classic = vscode.window.visibleTextEditors.find((e) => e.document.uri.scheme === "file");
        if (classic) {
            await vscode.window.showTextDocument(classic.document, {
                viewColumn: classic.viewColumn,
                preview: false,
                preserveFocus: false,
            });
            return;
        }
        await vscode.commands.executeCommand(FOCUS_EDITOR);
    }
    /** Toggle Map ↔ Classic. */
    async toggle() {
        if (this.mapVisible) {
            await this.showClassic();
        }
        else {
            await this.showMap();
        }
    }
    /** Called when the webview view becomes visible (user opened the Horizon icon). */
    markMapShown() {
        this.mapVisible = true;
        void vscode.commands.executeCommand("setContext", exports.MAP_VISIBLE_CONTEXT, true);
    }
    dispose() {
        for (const d of this.disposables) {
            d.dispose();
        }
    }
}
exports.MapToggle = MapToggle;
//# sourceMappingURL=toggle.js.map