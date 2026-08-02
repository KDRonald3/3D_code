"use strict";
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
exports.getMapWebviewHtml = getMapWebviewHtml;
const fs = __importStar(require("fs"));
const vscode = __importStar(require("vscode"));
/**
 * Build webview HTML from media/index.html with CSP + asWebviewUri rewrites.
 * Supports `{{cspSource}}` / `{{media}}` placeholders and `/static/*` paths.
 */
function getMapWebviewHtml(webview, extensionUri) {
    const mediaRoot = vscode.Uri.joinPath(extensionUri, "media");
    const mediaUri = webview.asWebviewUri(mediaRoot);
    const htmlPath = vscode.Uri.joinPath(mediaRoot, "index.html").fsPath;
    let html = fs.readFileSync(htmlPath, "utf8");
    const csp = [
        `default-src 'none'`,
        `img-src ${webview.cspSource} data:`,
        `style-src ${webview.cspSource} 'unsafe-inline' https://fonts.googleapis.com`,
        `font-src ${webview.cspSource} https://fonts.gstatic.com data:`,
        `script-src ${webview.cspSource}`,
    ].join("; ");
    if (!/Content-Security-Policy/i.test(html)) {
        html = html.replace(/<head([^>]*)>/i, `<head$1>\n  <meta http-equiv="Content-Security-Policy" content="${csp}" />`);
    }
    else {
        html = html.replace(/\{\{cspSource\}\}/g, webview.cspSource);
    }
    html = html.replace(/\{\{media\}\}/g, mediaUri.toString());
    html = html.replace(/(?:src|href)="\/static\/([^"]+)"/g, (match, file) => {
        const attr = match.startsWith("href") ? "href" : "src";
        return `${attr}="${mediaUri.toString()}/${file}"`;
    });
    return html;
}
//# sourceMappingURL=webviewHost.js.map