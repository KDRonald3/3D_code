/*---------------------------------------------------------------------------------------------
 *  Copyright (c) Horizon contributors. All rights reserved.
 *--------------------------------------------------------------------------------------------*/

/**
 * Read-only inspection canvas (W3′).
 *
 * Opens the real workspace file in a workbench text editor (not the map webview)
 * so rust-analyzer attaches: hover, go-to-def, diagnostics, completions.
 * The editor session is marked read-only — no edits on the canvas.
 *
 * Tab chrome still shows the real filename (required: real `file:` URI for RA).
 * Status bar shows `Inspect: <fn>`. Completions may open via
 * `editor.action.triggerSuggest`; accepting them cannot mutate a readonly buffer,
 * and a change-guard reverts any leftover edits.
 *
 * Inspection does **not** require the Horizon sidecar when the file is on disk.
 * Sidecar `sourceRequest` slices remain an optional webview inspector fallback.
 */

import { VSBuffer, encodeHex } from '../../../../base/common/buffer.js';
import { Disposable, MutableDisposable } from '../../../../base/common/lifecycle.js';
import { Schemas } from '../../../../base/common/network.js';
import { basename, isEqual, isEqualOrParent, joinPath, resolvePath } from '../../../../base/common/resources.js';
import { URI } from '../../../../base/common/uri.js';
import { ICodeEditor } from '../../../../editor/browser/editorBrowser.js';
import { IPosition } from '../../../../editor/common/core/position.js';
import { IRange } from '../../../../editor/common/core/range.js';
import { ScrollType } from '../../../../editor/common/editorCommon.js';
import { ILanguageService } from '../../../../editor/common/languages/language.js';
import { ITextModel } from '../../../../editor/common/model.js';
import { IModelService } from '../../../../editor/common/services/model.js';
import { localize } from '../../../../nls.js';
import { ICommandService } from '../../../../platform/commands/common/commands.js';
import { IContextKey, IContextKeyService, RawContextKey } from '../../../../platform/contextkey/common/contextkey.js';
import { ITextResourceEditorInput, TextEditorSelectionRevealType } from '../../../../platform/editor/common/editor.js';
import { IFileService } from '../../../../platform/files/common/files.js';
import { createDecorator } from '../../../../platform/instantiation/common/instantiation.js';
import { INotificationService, Severity } from '../../../../platform/notification/common/notification.js';
import { IWorkspaceContextService } from '../../../../platform/workspace/common/workspace.js';
import { EditorResourceAccessor, GroupIdentifier, SideBySideEditor } from '../../../common/editor.js';
import { IEditorGroupsService } from '../../../services/editor/common/editorGroupsService.js';
import { IEditorService, SIDE_GROUP } from '../../../services/editor/common/editorService.js';
import { IFilesConfigurationService } from '../../../services/filesConfiguration/common/filesConfigurationService.js';
import { IStatusbarEntryAccessor, IStatusbarService, StatusbarAlignment } from '../../../services/statusbar/browser/statusbar.js';
import { ITextFileService } from '../../../services/textfile/common/textfiles.js';
import {
	HORIZON_CMD_TRIGGER_INSPECT_SUGGEST,
	HORIZON_INSPECTION_ACTIVE_CONTEXT,
	SelectFunctionPayload,
} from '../common/horizon.js';

export const IHorizonInspectionService = createDecorator<IHorizonInspectionService>('horizonInspectionService');

export interface IHorizonInspectionService {
	readonly _serviceBrand: undefined;

	/** Open / reveal a free function in a read-only editor on the real file URI. */
	openInspection(payload: SelectFunctionPayload): Promise<void>;

	/** Open a file read-only without a function range (file-card selection). */
	openFileReadonly(filePath: string): Promise<void>;

	/**
	 * Trigger the suggest widget in the active inspection editor.
	 * Read-only buffers still allow the widget; accepting a completion cannot
	 * write when the session is readonly (and the edit guard rejects leftovers).
	 */
	triggerSuggest(): Promise<void>;

	/** True when `uri` is the current inspection document. */
	isInspectionUri(uri: URI): boolean;
}

interface InspectionSession {
	uri: URI;
	functionId: string | null;
	functionName: string;
	range: IRange | undefined;
	groupId: GroupIdentifier | undefined;
}

export const HorizonInspectionActiveContext = new RawContextKey<boolean>(HORIZON_INSPECTION_ACTIVE_CONTEXT, false);

/**
 * Owns the inspection editor lifecycle: open, reveal, readonly, edit guard.
 */
export class HorizonInspectionService extends Disposable implements IHorizonInspectionService {

	declare readonly _serviceBrand: undefined;

	private session: InspectionSession | undefined;
	private readonly status: IStatusbarEntryAccessor;
	private readonly inspectionActive: IContextKey<boolean>;
	/** Document versions we last observed — used to undo sneaky edits. */
	private readonly lockedVersions = new Map<string, number>();
	private undoing = false;
	private readonly modelListener = this._register(new MutableDisposable());

	constructor(
		@IEditorService private readonly editorService: IEditorService,
		@IEditorGroupsService private readonly editorGroupsService: IEditorGroupsService,
		@IFileService private readonly fileService: IFileService,
		@IWorkspaceContextService private readonly workspaceService: IWorkspaceContextService,
		@IFilesConfigurationService private readonly filesConfigurationService: IFilesConfigurationService,
		@INotificationService private readonly notificationService: INotificationService,
		@IStatusbarService statusbarService: IStatusbarService,
		@IContextKeyService contextKeyService: IContextKeyService,
		@ICommandService private readonly commandService: ICommandService,
		@ILanguageService private readonly languageService: ILanguageService,
		@IModelService private readonly modelService: IModelService,
		@ITextFileService private readonly textFileService: ITextFileService,
	) {
		super();

		this.inspectionActive = HorizonInspectionActiveContext.bindTo(contextKeyService);

		this.status = this._register(statusbarService.addEntry(
			{
				name: localize('status.horizonInspect.name', 'Horizon Inspection'),
				text: '',
				ariaLabel: localize('status.horizonInspect.aria', 'Horizon Inspection'),
				tooltip: localize(
					'status.horizonInspect.tip',
					'Horizon Inspection (read-only). Click to trigger completions (informational).'
				),
				command: HORIZON_CMD_TRIGGER_INSPECT_SUGGEST,
				showBeak: false,
			},
			'status.horizonInspect',
			StatusbarAlignment.RIGHT,
			100
		));
		this.status.update({
			name: localize('status.horizonInspect.name', 'Horizon Inspection'),
			text: '',
			ariaLabel: localize('status.horizonInspect.aria', 'Horizon Inspection'),
			command: HORIZON_CMD_TRIGGER_INSPECT_SUGGEST,
		});
		// Hide until a session exists (empty text still shows; use dispose pattern via update).
		this.hideStatus();

		this._register(this.editorService.onDidActiveEditorChange(() => {
			void this.syncActiveContext();
		}));
	}

	async openInspection(payload: SelectFunctionPayload): Promise<void> {
		const filePath = payload.filePath;
		if (!filePath) {
			this.notificationService.notify({
				severity: Severity.Warning,
				message: localize('horizonInspectNoPath', "Horizon: selectFunction payload has no filePath."),
			});
			return;
		}

		const uri = await this.resolveWorkspaceFile(filePath);
		if (!uri) {
			return;
		}

		if (payload.contentHash) {
			const stale = await this.contentHashMismatch(uri, payload.contentHash);
			if (stale) {
				this.notificationService.notify({
					severity: Severity.Warning,
					message: localize(
						'horizonInspectStaleHash',
						"Horizon: file changed since analysis — offsets may be wrong. Re-analyse the workspace."
					),
				});
			}
		}

		const range = await this.resolveFunctionRange(uri, payload);
		const functionName = resolveFunctionName(payload);
		await this.openReadonlyEditor(uri, {
			functionId: payload.functionId,
			functionName,
			range,
			preview: false,
		});
	}

	async openFileReadonly(filePath: string): Promise<void> {
		const uri = await this.resolveWorkspaceFile(filePath);
		if (!uri) {
			return;
		}
		const label = basename(uri);
		await this.openReadonlyEditor(uri, {
			functionId: null,
			functionName: label,
			range: undefined,
			preview: true,
		});
	}

	async triggerSuggest(): Promise<void> {
		const active = this.getActiveInspectionEditor();
		if (!active) {
			this.notificationService.notify({
				severity: Severity.Info,
				message: localize(
					'horizonInspectSuggestFocus',
					"Horizon: focus an Inspection editor first, then trigger completions."
				),
			});
			return;
		}
		await this.setReadonly(active.uri);
		await this.commandService.executeCommand('editor.action.triggerSuggest');
	}

	isInspectionUri(uri: URI): boolean {
		return !!this.session && isEqual(uri, this.session.uri);
	}

	private async openReadonlyEditor(
		uri: URI,
		opts: {
			functionId: string | null;
			functionName: string;
			range: IRange | undefined;
			preview: boolean;
		}
	): Promise<void> {
		const group = this.pickInspectionGroup();
		const selection = opts.range
			? {
				startLineNumber: opts.range.startLineNumber,
				startColumn: opts.range.startColumn,
				endLineNumber: opts.range.startLineNumber,
				endColumn: opts.range.startColumn,
			}
			: undefined;

		const input: ITextResourceEditorInput = {
			resource: uri,
			options: {
				pinned: !opts.preview,
				preserveFocus: false,
				selection,
				selectionRevealType: opts.range ? TextEditorSelectionRevealType.Center : undefined,
			},
		};
		const pane = await this.editorService.openEditor(input, group);

		const groupId = pane?.group?.id ?? (typeof group === 'number' && group >= 0 ? group : undefined);
		const codeEditor = this.getCodeEditorFromPane(pane);

		await this.ensureRustLanguage(uri, codeEditor);

		if (opts.range && codeEditor) {
			codeEditor.revealRangeInCenter(opts.range, ScrollType.Smooth);
			codeEditor.setPosition({
				lineNumber: opts.range.startLineNumber,
				column: opts.range.startColumn,
			});
			codeEditor.setSelection({
				startLineNumber: opts.range.startLineNumber,
				startColumn: opts.range.startColumn,
				endLineNumber: opts.range.startLineNumber,
				endColumn: opts.range.startColumn,
			});
		}

		await this.setReadonly(uri);
		this.lockModel(uri);

		this.session = {
			uri,
			functionId: opts.functionId,
			functionName: opts.functionName,
			range: opts.range,
			groupId,
		};
		this.updateStatus(opts.functionName);
		this.attachModelGuard(uri);
		await this.syncActiveContext();

		this.notificationService.notify({
			severity: Severity.Info,
			message: localize('horizonInspectOpened', "Inspect: {0}", opts.functionName),
		});
	}

	/**
	 * Resolve `filePath` to a `file:` URI under an open workspace folder.
	 * Rejects path escapes (`..`) and paths outside the workspace.
	 */
	private async resolveWorkspaceFile(filePath: string): Promise<URI | undefined> {
		const folders = this.workspaceService.getWorkspace().folders;
		if (!folders.length) {
			this.notificationService.error(
				localize('horizonInspectNoWorkspace', "Horizon: open a workspace folder before inspecting a function.")
			);
			return undefined;
		}

		const trimmed = filePath.trim();
		if (!trimmed) {
			this.notificationService.error(
				localize('horizonInspectBadPath', "Horizon: refusing empty file path.")
			);
			return undefined;
		}

		const candidates: URI[] = [];
		const asUri = URI.file(trimmed);
		if (asUri.scheme === Schemas.file && (trimmed.startsWith('/') || /^[A-Za-z]:[\\/]/.test(trimmed))) {
			candidates.push(asUri);
		}
		for (const folder of folders) {
			candidates.push(resolvePath(folder.uri, trimmed));
			candidates.push(joinPath(folder.uri, trimmed));
		}

		const seen = new Set<string>();
		for (const candidate of candidates) {
			const key = candidate.toString();
			if (seen.has(key)) {
				continue;
			}
			seen.add(key);

			const folder = folders.find(f => isEqualOrParent(candidate, f.uri));
			if (!folder) {
				continue;
			}
			// Reject if relative path from folder escapes (resolvePath may normalize away `..`
			// but still land outside — isEqualOrParent already failed those).
			try {
				const stat = await this.fileService.stat(candidate);
				if (!stat.isDirectory) {
					return candidate;
				}
			} catch {
				// try next candidate
			}
		}

		// Distinguish escape vs missing for clearer errors.
		const absTry = URI.file(trimmed);
		const underAny = folders.some(f => {
			try {
				return isEqualOrParent(absTry, f.uri) || isEqualOrParent(resolvePath(f.uri, trimmed), f.uri);
			} catch {
				return false;
			}
		});
		if (!underAny && (trimmed.includes('..') || trimmed.startsWith('/') || /^[A-Za-z]:[\\/]/.test(trimmed))) {
			this.notificationService.error(
				localize('horizonInspectPathEscape', "Horizon: refusing path outside workspace: {0}", trimmed)
			);
		} else {
			this.notificationService.error(
				localize('horizonInspectFileMissing', "Horizon: file not found: {0}", trimmed)
			);
		}
		return undefined;
	}

	private async resolveFunctionRange(
		uri: URI,
		payload: SelectFunctionPayload
	): Promise<IRange | undefined> {
		const byteStart = payload.byteStart;
		const byteEnd = payload.byteEnd;
		const hasBytes =
			typeof byteStart === 'number' &&
			typeof byteEnd === 'number' &&
			!(byteStart === 0 && byteEnd === 0) &&
			byteEnd > byteStart;

		if (hasBytes) {
			try {
				const raw = await this.fileService.readFile(uri);
				const start = byteOffsetToPosition(raw.value.buffer, byteStart!);
				const end = byteOffsetToPosition(raw.value.buffer, byteEnd!);
				const model = this.modelService.getModel(uri);
				const startPos = clampPosition(start, model);
				const endPos = clampPosition(end, model);
				if (
					startPos.lineNumber < endPos.lineNumber ||
					(startPos.lineNumber === endPos.lineNumber && startPos.column <= endPos.column)
				) {
					return {
						startLineNumber: startPos.lineNumber,
						startColumn: startPos.column,
						endLineNumber: endPos.lineNumber,
						endColumn: endPos.column,
					};
				}
			} catch (err) {
				console.warn('[Horizon] byte→position conversion failed', err);
			}
		}

		if (typeof payload.line === 'number' && payload.line > 0) {
			const model = this.modelService.getModel(uri);
			const lineCount = model?.getLineCount() ?? payload.line;
			const lineNumber = Math.min(payload.line, lineCount);
			const maxCol = model?.getLineMaxColumn(lineNumber) ?? 1;
			return {
				startLineNumber: lineNumber,
				startColumn: 1,
				endLineNumber: lineNumber,
				endColumn: maxCol,
			};
		}
		return undefined;
	}

	private pickInspectionGroup(): GroupIdentifier | typeof SIDE_GROUP {
		if (this.session?.groupId !== undefined) {
			const existing = this.editorGroupsService.getGroup(this.session.groupId);
			if (existing) {
				return existing.id;
			}
		}
		if (this.session) {
			for (const group of this.editorGroupsService.groups) {
				if (group.editors.some(e => {
					const uri = EditorResourceAccessor.getCanonicalUri(e, { supportSideBySide: SideBySideEditor.PRIMARY });
					return uri && isEqual(uri, this.session!.uri);
				})) {
					return group.id;
				}
			}
		}
		return SIDE_GROUP;
	}

	private async ensureRustLanguage(uri: URI, codeEditor: ICodeEditor | undefined): Promise<void> {
		const path = uri.path || uri.fsPath;
		if (!path.endsWith('.rs')) {
			return;
		}
		const model = codeEditor?.getModel() ?? this.modelService.getModel(uri);
		if (!model) {
			return;
		}
		if (model.getLanguageId() !== 'rust') {
			model.setLanguage(this.languageService.createById('rust'));
		}
	}

	private async setReadonly(uri: URI): Promise<void> {
		try {
			await this.filesConfigurationService.updateReadonly(uri, true);
		} catch (err) {
			console.warn('[Horizon] updateReadonly failed; falling back to command', err);
			try {
				await this.commandService.executeCommand(
					'workbench.action.files.setActiveEditorReadonlyInSession'
				);
			} catch (cmdErr) {
				console.warn(
					'[Horizon] setActiveEditorReadonlyInSession failed; inspection may be editable',
					cmdErr
				);
			}
		}
	}

	private lockModel(uri: URI): void {
		const model = this.modelService.getModel(uri);
		if (model) {
			this.lockedVersions.set(uri.toString(), model.getVersionId());
		}
	}

	private attachModelGuard(uri: URI): void {
		this.modelListener.clear();
		const model = this.modelService.getModel(uri);
		if (!model) {
			return;
		}
		this.modelListener.value = model.onDidChangeContent(() => {
			void this.onModelChanged(model);
		});
	}

	private async onModelChanged(model: ITextModel): Promise<void> {
		if (this.undoing) {
			return;
		}
		if (!this.isInspectionUri(model.uri)) {
			return;
		}
		const key = model.uri.toString();
		const locked = this.lockedVersions.get(key);
		if (locked === undefined || model.getVersionId() <= locked) {
			return;
		}

		this.undoing = true;
		try {
			await this.commandService.executeCommand('undo');
			if (this.textFileService.isDirty(model.uri)) {
				await this.textFileService.revert(model.uri);
			}
			this.lockModel(model.uri);
			this.notificationService.notify({
				severity: Severity.Warning,
				message: localize(
					'horizonInspectReadonlyGuard',
					"Horizon Inspection is read-only — edits were discarded."
				),
			});
		} catch (err) {
			console.warn('[Horizon] failed to revert inspection edit', err);
		} finally {
			this.undoing = false;
			if (this.isInspectionUri(model.uri)) {
				await this.setReadonly(model.uri);
			}
		}
	}

	private updateStatus(functionName: string): void {
		this.status.update({
			name: localize('status.horizonInspect.name', 'Horizon Inspection'),
			text: `$(lock) Inspect: ${functionName}`,
			ariaLabel: localize('status.horizonInspect.ariaNamed', 'Inspect: {0}', functionName),
			tooltip: localize(
				'status.horizonInspect.tip',
				'Horizon Inspection (read-only). Click to trigger completions (informational).'
			),
			command: HORIZON_CMD_TRIGGER_INSPECT_SUGGEST,
		});
	}

	private hideStatus(): void {
		this.status.update({
			name: localize('status.horizonInspect.name', 'Horizon Inspection'),
			text: '',
			ariaLabel: localize('status.horizonInspect.aria', 'Horizon Inspection'),
			command: undefined,
		});
	}

	private async syncActiveContext(): Promise<void> {
		const active = this.getActiveInspectionEditor();
		this.inspectionActive.set(!!active);
		if (active && this.session) {
			this.updateStatus(this.session.functionName);
			await this.setReadonly(active.uri);
		} else if (!this.session) {
			this.hideStatus();
		}
	}

	private getActiveInspectionEditor(): { uri: URI; editor: ICodeEditor } | undefined {
		if (!this.session) {
			return undefined;
		}
		const pane = this.editorService.activeEditorPane;
		const uri = EditorResourceAccessor.getCanonicalUri(this.editorService.activeEditor, {
			supportSideBySide: SideBySideEditor.PRIMARY,
		});
		if (!uri || !this.isInspectionUri(uri)) {
			return undefined;
		}
		const editor = this.getCodeEditorFromPane(pane);
		if (!editor) {
			return undefined;
		}
		return { uri, editor };
	}

	private getCodeEditorFromPane(pane: { getControl(): unknown } | undefined): ICodeEditor | undefined {
		if (!pane) {
			return undefined;
		}
		const control = pane.getControl();
		if (control && typeof (control as ICodeEditor).getModel === 'function') {
			return control as ICodeEditor;
		}
		return undefined;
	}

	private async contentHashMismatch(uri: URI, expectedHex: string): Promise<boolean> {
		const expected = expectedHex.trim().toLowerCase();
		if (!expected || expected.length !== 64) {
			return false;
		}
		try {
			const raw = await this.fileService.readFile(uri);
			const digest = await crypto.subtle.digest('SHA-256', raw.value.buffer);
			const actual = encodeHex(VSBuffer.wrap(new Uint8Array(digest)));
			return actual !== expected;
		} catch {
			return true;
		}
	}
}

/** Display name for status / notification from payload or FunctionId. */
export function resolveFunctionName(payload: SelectFunctionPayload): string {
	const named = payload.functionName?.trim();
	if (named) {
		return named;
	}
	const id = payload.functionId?.trim();
	if (id) {
		const parts = id.split('::').filter(Boolean);
		const last = parts[parts.length - 1];
		if (last) {
			return last;
		}
	}
	if (payload.filePath) {
		const slash = Math.max(payload.filePath.lastIndexOf('/'), payload.filePath.lastIndexOf('\\'));
		return slash >= 0 ? payload.filePath.slice(slash + 1) : payload.filePath;
	}
	return 'function';
}

/**
 * Convert a UTF-8 byte offset into a 1-based (lineNumber, column) position using
 * raw file bytes — matching how horizon-map stores `byte_start` / `byte_end`.
 * Column uses UTF-16 code units (Monaco Position).
 */
export function byteOffsetToPosition(
	raw: Uint8Array,
	byteOffset: number
): IPosition {
	const clamped = Math.max(0, Math.min(byteOffset, raw.length));
	let line = 0;
	let lineStart = 0;
	for (let i = 0; i < clamped; i++) {
		if (raw[i] === 0x0a) {
			line++;
			lineStart = i + 1;
		}
	}
	const lineBytes = raw.subarray(lineStart, clamped);
	const character = new TextDecoder('utf-8').decode(lineBytes).length;
	return { lineNumber: line + 1, column: character + 1 };
}

function clampPosition(pos: IPosition, model: ITextModel | null): IPosition {
	if (!model) {
		return pos;
	}
	const lineNumber = Math.max(1, Math.min(pos.lineNumber, model.getLineCount()));
	const maxCol = model.getLineMaxColumn(lineNumber);
	const column = Math.max(1, Math.min(pos.column, maxCol));
	return { lineNumber, column };
}
