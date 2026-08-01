/**
 * Map ↔ Classic editor toggle.
 *
 * Shows / focuses the Horizon Map webview view, or leaves the map and focuses
 * a classic (non-webview) text editor group.
 */

import * as vscode from "vscode";

const MAP_VIEW_FOCUS = "horizon.map.view.focus";
const FOCUS_EDITOR = "workbench.action.focusActiveEditorGroup";

/** Context key mirrored for when-clauses / debugging. */
export const MAP_VISIBLE_CONTEXT = "horizon.map.visible";

export class MapToggle implements vscode.Disposable {
  private mapVisible = false;
  private readonly disposables: vscode.Disposable[] = [];

  constructor() {
    void vscode.commands.executeCommand(
      "setContext",
      MAP_VISIBLE_CONTEXT,
      false
    );
  }

  /** Whether the map side is considered active after the last toggle/show. */
  get isMapVisible(): boolean {
    return this.mapVisible;
  }

  /** Show and focus the Horizon Map view. */
  async showMap(): Promise<void> {
    try {
      await vscode.commands.executeCommand(MAP_VIEW_FOCUS);
    } catch {
      // Fallback: reveal the activity-bar container.
      await vscode.commands.executeCommand("workbench.view.extension.horizon");
    }
    this.mapVisible = true;
    await vscode.commands.executeCommand(
      "setContext",
      MAP_VISIBLE_CONTEXT,
      true
    );
  }

  /**
   * Leave the map and focus the classic editor.
   * Prefers an existing text editor tab over the map webview.
   */
  async showClassic(): Promise<void> {
    this.mapVisible = false;
    await vscode.commands.executeCommand(
      "setContext",
      MAP_VISIBLE_CONTEXT,
      false
    );

    const classic = vscode.window.visibleTextEditors.find(
      (e) => e.document.uri.scheme === "file"
    );
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
  async toggle(): Promise<void> {
    if (this.mapVisible) {
      await this.showClassic();
    } else {
      await this.showMap();
    }
  }

  /** Called when the webview view becomes visible (user opened the Horizon icon). */
  markMapShown(): void {
    this.mapVisible = true;
    void vscode.commands.executeCommand(
      "setContext",
      MAP_VISIBLE_CONTEXT,
      true
    );
  }

  dispose(): void {
    for (const d of this.disposables) {
      d.dispose();
    }
  }
}
