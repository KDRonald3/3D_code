/**
 * Horizon Map extension entry — activate, commands, sidecar + inspection.
 *
 * W2 owns webview media (`media/*`) and the bridge protocol.
 * W3 owns host TypeScript: sidecar lifecycle, Map ↔ Classic toggle,
 * and the read-only inspection canvas backed by a real text editor + RA.
 */

import * as vscode from "vscode";
import { HorizonMapViewProvider } from "./mapView";
import { HorizonSidecar } from "./sidecar";
import { MapToggle } from "./toggle";

let sidecar: HorizonSidecar | undefined;

export function activate(context: vscode.ExtensionContext): void {
  const output = vscode.window.createOutputChannel("Horizon Map");
  sidecar = new HorizonSidecar(context.extensionPath, output);
  const toggle = new MapToggle();
  const provider = new HorizonMapViewProvider(
    context.extensionUri,
    sidecar,
    toggle
  );

  context.subscriptions.push(
    output,
    sidecar,
    toggle,
    vscode.window.registerWebviewViewProvider(
      HorizonMapViewProvider.viewType,
      provider,
      { webviewOptions: { retainContextWhenHidden: true } }
    ),
    vscode.commands.registerCommand("horizon.map.toggle", () => toggle.toggle()),
    vscode.commands.registerCommand("horizon.map.analyseWorkspace", () =>
      provider.analyseWorkspace()
    ),
    vscode.commands.registerCommand("horizon.map.show", () => toggle.showMap()),
    vscode.commands.registerCommand("horizon.map.hide", () =>
      toggle.showClassic()
    )
  );

  const autoStart = vscode.workspace
    .getConfiguration("horizon.map")
    .get<boolean>("autoStartSidecar", true);
  if (autoStart) {
    void sidecar.ensureRunning().catch((err) => {
      output.appendLine(
        `[activate] sidecar warm-start deferred: ${
          err instanceof Error ? err.message : String(err)
        }`
      );
    });
  }
}

export async function deactivate(): Promise<void> {
  if (sidecar) {
    await sidecar.stop();
    sidecar = undefined;
  }
}
