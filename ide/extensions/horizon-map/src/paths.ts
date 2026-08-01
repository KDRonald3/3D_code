/**
 * Path helpers — keep inspection / analyse targets inside the workspace.
 */

import * as fs from "fs";
import * as path from "path";
import * as vscode from "vscode";

/** Active workspace folder, or `undefined` when none is open. */
export function activeWorkspaceFolder(): vscode.WorkspaceFolder | undefined {
  const editor = vscode.window.activeTextEditor;
  if (editor) {
    const folder = vscode.workspace.getWorkspaceFolder(editor.document.uri);
    if (folder) {
      return folder;
    }
  }
  return vscode.workspace.workspaceFolders?.[0];
}

/** Absolute fs path of the active workspace folder. */
export function workspaceRootFsPath(): string | undefined {
  return activeWorkspaceFolder()?.uri.fsPath;
}

/**
 * Resolve `candidate` to an absolute path and ensure it stays under `root`.
 * Rejects `..` escapes and paths outside the workspace.
 */
export function resolveUnderRoot(
  root: string,
  candidate: string
): string | undefined {
  if (!candidate || !root) {
    return undefined;
  }
  const rootResolved = path.resolve(root);
  const abs = path.isAbsolute(candidate)
    ? path.resolve(candidate)
    : path.resolve(rootResolved, candidate);

  const rel = path.relative(rootResolved, abs);
  if (rel.startsWith("..") || path.isAbsolute(rel)) {
    return undefined;
  }
  return abs;
}

/** True when `filePath` exists and is a regular file. */
export function isExistingFile(filePath: string): boolean {
  try {
    return fs.statSync(filePath).isFile();
  } catch {
    return false;
  }
}

/**
 * Locate the Horizon Cargo workspace that contains `horizon-server`.
 * Prefer configuration, then walk up from the extension install path.
 */
export function findHorizonCargoWorkspace(
  extensionPath: string,
  configured?: string
): string | undefined {
  if (configured && configured.trim()) {
    const p = path.resolve(configured.trim());
    if (isHorizonWorkspace(p)) {
      return p;
    }
  }

  let dir = path.resolve(extensionPath);
  for (let i = 0; i < 8; i++) {
    if (isHorizonWorkspace(dir)) {
      return dir;
    }
    const parent = path.dirname(dir);
    if (parent === dir) {
      break;
    }
    dir = parent;
  }
  return undefined;
}

function isHorizonWorkspace(dir: string): boolean {
  const cargo = path.join(dir, "Cargo.toml");
  if (!fs.existsSync(cargo)) {
    return false;
  }
  try {
    const text = fs.readFileSync(cargo, "utf8");
    return (
      text.includes("horizon-server") ||
      fs.existsSync(path.join(dir, "crates", "horizon-server", "Cargo.toml"))
    );
  } catch {
    return false;
  }
}

/** Prefer release, then debug, built binaries under a cargo workspace. */
export function findBuiltServerBinary(cargoWorkspace: string): string | undefined {
  const candidates = [
    path.join(cargoWorkspace, "target", "release", "horizon-server"),
    path.join(cargoWorkspace, "target", "debug", "horizon-server"),
  ];
  for (const c of candidates) {
    if (fs.existsSync(c)) {
      return c;
    }
  }
  return undefined;
}
