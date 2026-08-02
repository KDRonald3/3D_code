/*---------------------------------------------------------------------------------------------
 *  Copyright (c) Horizon contributors. All rights reserved.
 *--------------------------------------------------------------------------------------------*/

/**
 * Shared Horizon Map identifiers and webview message protocol.
 * Keep unions aligned with `browser/media/bridge.js`.
 */

export const HORIZON_MAP_EDITOR_ID = 'workbench.editor.horizonMap';
export const HORIZON_MAP_INPUT_ID = 'workbench.input.horizonMap';
export const HORIZON_MAP_SCHEME = 'horizon-map';

/** Context key: Map EditorPane is the active mode target. */
export const HORIZON_MAP_VISIBLE_CONTEXT = 'horizon.map.visible';

/** Commands (workbench Action2 / Command Palette). */
export const HORIZON_CMD_OPEN = 'horizon.map.open';
export const HORIZON_CMD_TOGGLE = 'horizon.map.toggle';
export const HORIZON_CMD_ANALYSE = 'horizon.map.analyse';
export const HORIZON_CMD_SHOW = 'horizon.map.show';
export const HORIZON_CMD_HIDE = 'horizon.map.hide';
/** QuickPick workspace folders + Browse… (primary analyse-root UX). */
export const HORIZON_CMD_CHOOSE_FOLDER = 'horizon.map.chooseFolder';
/** Trigger suggest in the active Inspection editor (informational completions). */
export const HORIZON_CMD_TRIGGER_INSPECT_SUGGEST = 'horizon.inspection.triggerSuggest';

/** Context key: an Inspection editor is the active text editor. */
export const HORIZON_INSPECTION_ACTIVE_CONTEXT = 'horizon.inspection.active';

/** Workspace-scoped storage key for the Horizon analyse root path. */
export const HORIZON_FOLDER_STORAGE_KEY = 'horizon.analyseFolder';

/** Messages the webview posts to the workbench host. */
export type WebviewToHostMessage =
	| { type: 'ready' }
	| { type: 'analyse'; path?: string }
	| {
		type: 'selectFunction';
		functionId: string | null;
		fileId?: string | null;
		filePath?: string | null;
		functionName?: string | null;
		line?: number | null;
		byteStart?: number | null;
		byteEnd?: number | null;
		contentHash?: string | null;
	}
	| {
		type: 'selectFile';
		fileId: string | null;
		filePath?: string | null;
	}
	| { type: 'openMapJson' }
	| { type: 'chooseFolder' }
	| {
		type: 'sourceRequest';
		requestId: string;
		path: string;
		byteStart: number;
		byteEnd: number;
		expectedHash: string;
	};

/** Messages the workbench host posts into the webview. */
export type HostToWebviewMessage =
	| { type: 'mapData'; map: unknown; label?: string; error?: string }
	| {
		type: 'analyseResult';
		status: 'running' | 'done' | 'failed' | 'error' | 'idle';
		path?: string;
		label?: string;
		error?: string;
		elapsed_ms?: number;
		map?: unknown;
	}
	| { type: 'selectFunction'; functionId: string }
	| { type: 'selectFile'; fileId: string }
	| { type: 'workspaceInfo'; root?: string; name?: string }
	| { type: 'theme'; theme: 'light' | 'dark' }
	| {
		type: 'sourceResult';
		requestId: string;
		tokens?: [string, string][] | unknown;
		error?: string;
		errorKind?: string;
		message?: string;
	}
	| { type: 'error'; message: string };

/** Normalised selectFunction payload for the inspection canvas (W3). */
export interface SelectFunctionPayload {
	type: 'selectFunction';
	functionId: string | null;
	fileId: string | null;
	filePath: string | null;
	functionName: string | null;
	line: number | null;
	byteStart: number | null;
	byteEnd: number | null;
	contentHash: string | null;
}
