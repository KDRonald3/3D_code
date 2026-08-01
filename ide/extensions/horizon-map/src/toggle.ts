/**
 * Map ↔ Classic editor toggle.
 *
 * Shows / focuses the Horizon Map webview view, or leaves the map and restores
 * the last classic (non-webview) text editor without closing the workspace.
 */

import * as vscode from "vscode";

const MAP_VIEW_FOCUS = "horizon.map.view.focus";
const FOCUS_EDITOR = "workbench.action.focusActiveEditorGroup";

/** Context key mirrored for when-clauses / debugging. */
export const MAP_VISIBLE_CONTEXT = "horizon.map.visible";

interface ClassicFocus {
  uri: vscode.Uri;
  viewColumn: vscode.ViewColumn | undefined;
  selection: vscode.Selection;
}

export class MapToggle implements vscode.Disposable {
  private mapVisible = false;
  private classicFocus: ClassicFocus | undefined;
  private readonly disposables: vscode.Disposable[] = [];

  constructor() {
    void vscode.commands.executeCommand(
      "setContext",
      MAP_VISIBLE_CONTEXT,
      false
    );

    this.disposables.push(
      vscode.window.onDidChangeActiveTextEditor((editor) => {
        // Remember classic focus only while map is not the active mode target.
        if (!this.mapVisible && editor && editor.document.uri.scheme === "file") {
          this.classicFocus = {
            uri: editor.document.uri,
            viewColumn: editor.viewColumn,
            selection: editor.selection,
          };
        }
      })
    );

    const active = vscode.window.activeTextEditor;
    if (active && active.document.uri.scheme === "file") {
      this.classicFocus = {
        uri: active.document.uri,
        viewColumn: active.viewColumn,
        selection: active.selection,
      };
    }
  }

  /** Whether the map side is considered active after the last toggle/show. */
  get isMapVisible(): boolean {
    return this.mapVisible;
  }

  /** Show and focus the Horizon Map view. */
  async showMap(): Promise<void> {
    // Snapshot classic focus before leaving the editor group.
    const active = vscode.window.activeTextEditor;
    if (active && active.document.uri.scheme === "file") {
      this.classicFocus = {
        uri: active.document.uri,
        viewColumn: active.viewColumn,
        selection: active.selection,
      };
    }

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
   * Restores the remembered text editor when possible; does not close tabs
   * or change the workspace folder.
   */
  async showClassic(): Promise<void> {
    this.mapVisible = false;
    await vscode.commands.executeCommand(
      "setContext",
      MAP_VISIBLE_CONTEXT,
      false
    );

    if (this.classicFocus) {
      try {
        const doc = await vscode.workspace.openTextDocument(this.classicFocus.uri);
        await vscode.window.showTextDocument(doc, {
          viewColumn: this.classicFocus.viewColumn ?? vscode.ViewColumn.One,
          preview: false,
          preserveFocus: false,
          selection: this.classicFocus.selection,
        });
        return;
      } catch (err) {
        console.debug("[Horizon] classic focus restore failed", err);
      }
    }

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
