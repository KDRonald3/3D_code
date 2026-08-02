/**
 * Horizon Map extension entry — activate, commands, sidecar + inspection.
 *
 * W2 owns webview media (`media/*`) and the bridge protocol.
 * Host TypeScript: sidecar client, Map ↔ Classic toggle, inspection bridge.
 */

import * as vscode from "vscode";
import { InspectionController } from "./inspection";
import { HorizonMapViewProvider } from "./mapView";
import { HorizonSidecar } from "./sidecar";
import { MapToggle } from "./toggle";

let sidecar: HorizonSidecar | undefined;

export function activate(context: vscode.ExtensionContext): void {
  // Shared "Horizon" channel for sidecar + host diagnostics (W4).
  const output = vscode.window.createOutputChannel("Horizon");
  sidecar = new HorizonSidecar(context.extensionPath, output);
  const toggle = new MapToggle();
  const inspection = new InspectionController();
  const provider = new HorizonMapViewProvider(
    context.extensionUri,
    sidecar,
    toggle,
    inspection
  );

  context.subscriptions.push(
    output,
    sidecar,
    toggle,
    inspection,
    vscode.window.registerWebviewViewProvider(
      HorizonMapViewProvider.viewType,
      provider,
      { webviewOptions: { retainContextWhenHidden: true } }
    ),
    // Canonical W2 commands
    vscode.commands.registerCommand("horizon.map.open", () => toggle.showMap()),
    vscode.commands.registerCommand("horizon.map.toggle", () => toggle.toggle()),
    vscode.commands.registerCommand("horizon.map.analyse", () =>
      provider.analyseWorkspace()
    ),
    // Aliases kept for docs / earlier host wiring
    vscode.commands.registerCommand("horizon.map.show", () => toggle.showMap()),
    vscode.commands.registerCommand("horizon.map.hide", () =>
      toggle.showClassic()
    ),
    vscode.commands.registerCommand("horizon.map.analyseWorkspace", () =>
      provider.analyseWorkspace()
    )
  );

  const autoStart = vscode.workspace
    .getConfiguration("horizon.map")
    .get<boolean>("autoStartSidecar", true);
  if (autoStart) {
    void sidecar.ensureRunning().catch((err) => {
      const message = err instanceof Error ? err.message : String(err);
      output.appendLine(`[activate] sidecar warm-start failed: ${message}`);
      // Soft failure — first analyse will retry. Surface once in the UI.
      void vscode.window
        .showWarningMessage(
          `Horizon sidecar did not start: ${message}`,
          "Show Horizon output"
        )
        .then((choice) => {
          if (choice === "Show Horizon output") {
            output.show(true);
          }
        });
    });
  }
}

export async function deactivate(): Promise<void> {
  if (sidecar) {
    await sidecar.stop();
    sidecar = undefined;
  }
}
