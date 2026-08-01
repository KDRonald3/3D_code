/*---------------------------------------------------------------------------------------------
 *  Copyright (c) Horizon contributors. All rights reserved.
 *--------------------------------------------------------------------------------------------*/

import { FileAccess } from '../../../../base/common/network.js';
import { joinPath } from '../../../../base/common/resources.js';
import { URI } from '../../../../base/common/uri.js';
import { IFileService } from '../../../../platform/files/common/files.js';
import { asWebviewUri, webviewGenericCspSource } from '../../webview/common/webview.js';

/** Resource path under `vs/` for FileAccess (compiled into `out/vs/...`). */
export const HORIZON_MEDIA_RESOURCE_ROOT = 'vs/workbench/contrib/horizon/browser/media/' as const;

export function horizonMediaFileRoot(): URI {
	return FileAccess.asFileUri(HORIZON_MEDIA_RESOURCE_ROOT);
}

/**
 * Build Map EditorPane webview HTML from media/index.html with CSP + asWebviewUri rewrites.
 */
export async function buildHorizonMapHtml(fileService: IFileService): Promise<string> {
	const mediaRoot = horizonMediaFileRoot();
	const indexUri = joinPath(mediaRoot, 'index.html');
	const content = await fileService.readFile(indexUri);
	let html = content.value.toString();

	const mediaWebviewUri = asWebviewUri(mediaRoot);
	const csp = [
		`default-src 'none'`,
		`img-src ${webviewGenericCspSource} data:`,
		`style-src ${webviewGenericCspSource} 'unsafe-inline' https://fonts.googleapis.com`,
		`font-src ${webviewGenericCspSource} https://fonts.gstatic.com data:`,
		`script-src ${webviewGenericCspSource}`,
	].join('; ');

	if (!/Content-Security-Policy/i.test(html)) {
		html = html.replace(
			/<head([^>]*)>/i,
			`<head$1>\n  <meta http-equiv="Content-Security-Policy" content="${csp}" />`
		);
	} else {
		html = html.replace(/\{\{cspSource\}\}/g, webviewGenericCspSource);
	}

	html = html.replace(/\{\{media\}\}/g, mediaWebviewUri.toString(true));
	html = html.replace(
		/(?:src|href)="\/static\/([^"]+)"/g,
		(_match, file: string) => {
			const attr = _match.startsWith('href') ? 'href' : 'src';
			return `${attr}="${asWebviewUri(joinPath(mediaRoot, file)).toString(true)}"`;
		}
	);

	return html;
}

/** Minimal stub when media/index.html is missing (dev / incomplete sync). */
export function buildHorizonMapFallbackHtml(): string {
	const nonce = String(Date.now());
	const csp = [
		`default-src 'none'`,
		`style-src 'unsafe-inline'`,
		`script-src 'nonce-${nonce}'`,
	].join('; ');

	return `<!DOCTYPE html>
<html lang="en">
<head>
  <meta charset="utf-8" />
  <meta http-equiv="Content-Security-Policy" content="${csp}" />
  <meta name="viewport" content="width=device-width, initial-scale=1" />
  <title>Horizon Map</title>
  <style>
    :root { color-scheme: light dark; }
    body {
      margin: 0; padding: 24px;
      font-family: "IBM Plex Sans", "Segoe UI", sans-serif;
      background: radial-gradient(120% 80% at 10% 0%, #1a3038 0%, #0e1619 55%, #0a1012 100%);
      color: #e7eef0; min-height: 100vh; box-sizing: border-box;
    }
    h1 { font-size: 1.4rem; font-weight: 600; margin: 0 0 8px; letter-spacing: 0.02em; }
    p { margin: 0 0 16px; opacity: 0.8; line-height: 1.45; max-width: 36rem; }
    button {
      background: #3d8f7a; color: #04110e; border: 0; border-radius: 4px;
      padding: 8px 14px; font-weight: 600; cursor: pointer;
    }
    #status { margin-top: 16px; font-family: "IBM Plex Mono", monospace; font-size: 12px; opacity: 0.7; }
  </style>
</head>
<body>
  <h1>Horizon Map</h1>
  <p>
    Map media not found under <code>contrib/horizon/browser/media/</code>.
    Re-run <code>./ide/scripts/bootstrap.sh</code> or sync the workbench contrib.
  </p>
  <button id="analyse" type="button">Analyse workspace</button>
  <div id="status">Waiting…</div>
  <script nonce="${nonce}">
    const vscode = acquireVsCodeApi();
    const status = document.getElementById('status');
    document.getElementById('analyse').addEventListener('click', () => {
      status.textContent = 'Analysing…';
      vscode.postMessage({ type: 'analyse' });
    });
    window.addEventListener('message', (event) => {
      const msg = event.data;
      if (!msg || !msg.type) return;
      if (msg.type === 'analyseResult') {
        status.textContent = msg.status + (msg.error ? (': ' + msg.error) : '');
      } else if (msg.type === 'mapData') {
        const crates = msg.map && msg.map.crates ? msg.map.crates.length : '?';
        status.textContent = 'Map ready (' + crates + ' crates).';
      } else if (msg.type === 'workspaceInfo') {
        status.textContent = 'Workspace: ' + (msg.root || '');
      }
    });
    vscode.postMessage({ type: 'ready' });
  </script>
</body>
</html>`;
}
