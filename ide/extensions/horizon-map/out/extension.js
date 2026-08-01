"use strict";
/**
 * Horizon Map extension entry — activate, commands, sidecar + inspection.
 *
 * W2 owns webview media (`media/*`) and the bridge protocol.
 * Host TypeScript: sidecar client, Map ↔ Classic toggle, inspection bridge.
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
exports.activate = activate;
exports.deactivate = deactivate;
const vscode = __importStar(require("vscode"));
const mapView_1 = require("./mapView");
const sidecar_1 = require("./sidecar");
const toggle_1 = require("./toggle");
let sidecar;
function activate(context) {
    const output = vscode.window.createOutputChannel("Horizon Map");
    sidecar = new sidecar_1.HorizonSidecar(context.extensionPath, output);
    const toggle = new toggle_1.MapToggle();
    const provider = new mapView_1.HorizonMapViewProvider(context.extensionUri, sidecar, toggle);
    context.subscriptions.push(output, sidecar, toggle, vscode.window.registerWebviewViewProvider(mapView_1.HorizonMapViewProvider.viewType, provider, { webviewOptions: { retainContextWhenHidden: true } }), 
    // Canonical W2 commands
    vscode.commands.registerCommand("horizon.map.open", () => toggle.showMap()), vscode.commands.registerCommand("horizon.map.toggle", () => toggle.toggle()), vscode.commands.registerCommand("horizon.map.analyse", () => provider.analyseWorkspace()), 
    // Aliases kept for docs / earlier host wiring
    vscode.commands.registerCommand("horizon.map.show", () => toggle.showMap()), vscode.commands.registerCommand("horizon.map.hide", () => toggle.showClassic()), vscode.commands.registerCommand("horizon.map.analyseWorkspace", () => provider.analyseWorkspace()));
    const autoStart = vscode.workspace
        .getConfiguration("horizon.map")
        .get("autoStartSidecar", true);
    if (autoStart) {
        void sidecar.ensureRunning().catch((err) => {
            output.appendLine(`[activate] sidecar warm-start deferred: ${err instanceof Error ? err.message : String(err)}`);
        });
    }
}
async function deactivate() {
    if (sidecar) {
        await sidecar.stop();
        sidecar = undefined;
    }
}
//# sourceMappingURL=extension.js.map