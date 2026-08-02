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

import { timeout } from '../../../../base/common/async.js';
import { VSBuffer, encodeHex } from '../../../../base/common/buffer.js';
import { CancellationToken } from '../../../../base/common/cancellation.js';
import { Disposable, MutableDisposable } from '../../../../base/common/lifecycle.js';
import { Schemas } from '../../../../base/common/network.js';
import { isEqual, isEqualOrParent, joinPath, resolvePath } from '../../../../base/common/resources.js';
import { URI } from '../../../../base/common/uri.js';
import { ICodeEditor } from '../../../../editor/browser/editorBrowser.js';
import { IPosition, Position } from '../../../../editor/common/core/position.js';
import { IRange, Range } from '../../../../editor/common/core/range.js';
import { ScrollType } from '../../../../editor/common/editorCommon.js';
import { ILanguageService } from '../../../../editor/common/languages/language.js';
import { LocationLink, Location } from '../../../../editor/common/languages.js';
import { ITextModel } from '../../../../editor/common/model.js';
import { ILanguageFeaturesService } from '../../../../editor/common/services/languageFeatures.js';
import { IModelService } from '../../../../editor/common/services/model.js';
import { ITextModelService } from '../../../../editor/common/services/resolverService.js';
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

	/**
	 * Record the Map's current function selection without opening anything.
	 * Selecting in the Map is a browsing gesture; taking over the editor area
	 * belongs to an explicit command.
	 */
	setPendingSelection(payload: SelectFunctionPayload | undefined): void;

	/** Open the remembered selection — the explicit counterpart to the above. */
	openPendingSelection(): Promise<void>;

	/**
	 * Go to the definition of a call target that is not in the map, by asking
	 * the definition providers (rust-analyzer) at the call site's own position
	 * in the enclosing file.
	 */
	openDefinitionAt(payload: {
		filePath: string | null;
		callPath?: string | null;
		line?: number | null;
		byteStart?: number | null;
		byteEnd?: number | null;
	}): Promise<void>;

	/**
	 * What the hover providers (rust-analyzer) say about the symbol at a UTF-8
	 * byte offset in a workspace file. Markdown blocks, or undefined when no
	 * provider answers. Never raises notifications — hover must stay quiet.
	 */
	hoverAt(filePath: string | null, byteOffset: number | null): Promise<string[] | undefined>;

	/**
	 * Where the definition of the symbol at a UTF-8 byte offset lives, without
	 * opening anything. `path` is workspace-relative (forward slashes) when the
	 * target is in the workspace, else null. Quiet like hoverAt — the webview
	 * uses this to decide between an in-map jump and `openDefinitionAt`.
	 */
	definitionAt(
		filePath: string | null,
		byteOffset: number | null
	): Promise<{ path: string | null; line: number; byteOffset: number | null } | undefined>;

	/**
	 * rust-analyzer semantic tokens covering `[byteStart, byteEnd)` of a
	 * workspace file, as absolute UTF-8 byte ranges. Quiet like hoverAt.
	 */
	semanticTokensFor(
		filePath: string | null,
		byteStart: number | null,
		byteEnd: number | null
	): Promise<{ b: number; l: number; t: string; m?: string[] }[] | undefined>;

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
	/** Last function selected in the Map; opened only on explicit request. */
	private pendingSelection: SelectFunctionPayload | undefined;
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
		@ITextModelService private readonly textModelService: ITextModelService,
		@ILanguageFeaturesService private readonly languageFeaturesService: ILanguageFeaturesService,
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

	setPendingSelection(payload: SelectFunctionPayload | undefined): void {
		this.pendingSelection = payload;
	}

	async openPendingSelection(): Promise<void> {
		if (!this.pendingSelection) {
			this.notificationService.notify({
				severity: Severity.Info,
				message: localize(
					'horizonInspectNoSelection',
					"Horizon: select a function in the Map first, then run this command."
				),
			});
			return;
		}
		await this.openInspection(this.pendingSelection);
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

	async openDefinitionAt(payload: {
		filePath: string | null;
		callPath?: string | null;
		line?: number | null;
		byteStart?: number | null;
		byteEnd?: number | null;
	}): Promise<void> {
		if (!payload.filePath) {
			return;
		}
		const uri = await this.resolveWorkspaceFile(payload.filePath);
		if (!uri) {
			return;
		}

		// Hold a model reference so the extension host opens the document and
		// rust-analyzer attaches its providers to it.
		const ref = await this.textModelService.createModelReference(uri);
		try {
			const model = ref.object.textEditorModel;
			const position = await this.resolveCalleePosition(uri, model, payload);
			if (!position) {
				this.notifyNoDefinition(payload.callPath);
				return;
			}

			let providers = this.languageFeaturesService.definitionProvider.ordered(model);
			if (!providers.length) {
				// rust-analyzer may still be starting; give registration a moment.
				await timeout(2500);
				providers = this.languageFeaturesService.definitionProvider.ordered(model);
			}
			if (!providers.length) {
				this.notificationService.notify({
					severity: Severity.Info,
					message: localize(
						'horizonNoDefProvider',
						"Horizon: no definition provider for Rust — is rust-analyzer installed and ready?"
					),
				});
				return;
			}

			let target: { uri: URI; range: IRange } | undefined;
			for (const provider of providers) {
				try {
					const result = await provider.provideDefinition(
						model,
						new Position(position.lineNumber, position.column),
						CancellationToken.None
					);
					target = firstDefinitionLocation(result ?? undefined);
					if (target) {
						break;
					}
				} catch (err) {
					console.warn('[Horizon] definition provider failed', err);
				}
			}
			if (!target) {
				this.notifyNoDefinition(payload.callPath);
				return;
			}

			await this.editorService.openEditor({
				resource: target.uri,
				options: {
					pinned: false,
					preserveFocus: false,
					selection: {
						startLineNumber: target.range.startLineNumber,
						startColumn: target.range.startColumn,
						endLineNumber: target.range.startLineNumber,
						endColumn: target.range.startColumn,
					},
					selectionRevealType: TextEditorSelectionRevealType.Center,
				},
			});
		} finally {
			ref.dispose();
		}
	}

	async hoverAt(filePath: string | null, byteOffset: number | null): Promise<string[] | undefined> {
		if (!filePath || typeof byteOffset !== 'number' || byteOffset < 0) {
			return undefined;
		}
		const uri = await this.resolveWorkspaceFile(filePath, true);
		if (!uri) {
			return undefined;
		}
		const ref = await this.textModelService.createModelReference(uri);
		try {
			const model = ref.object.textEditorModel;
			const raw = await this.fileService.readFile(uri);
			const pos = clampPosition(byteOffsetToPosition(raw.value.buffer, byteOffset), model);
			const providers = this.languageFeaturesService.hoverProvider.ordered(model);
			for (const provider of providers) {
				try {
					const hover = await provider.provideHover(
						model,
						new Position(pos.lineNumber, pos.column),
						CancellationToken.None
					);
					const contents = hover?.contents
						?.map(c => (typeof c === 'string' ? c : c.value))
						.filter((v): v is string => !!v && !!v.trim());
					if (contents && contents.length) {
						return contents;
					}
				} catch (err) {
					console.warn('[Horizon] hover provider failed', err);
				}
			}
			return undefined;
		} finally {
			ref.dispose();
		}
	}

	async definitionAt(
		filePath: string | null,
		byteOffset: number | null
	): Promise<{ path: string | null; line: number; byteOffset: number | null } | undefined> {
		if (!filePath || typeof byteOffset !== 'number' || byteOffset < 0) {
			return undefined;
		}
		const uri = await this.resolveWorkspaceFile(filePath, true);
		if (!uri) {
			return undefined;
		}
		const ref = await this.textModelService.createModelReference(uri);
		try {
			const model = ref.object.textEditorModel;
			const raw = await this.fileService.readFile(uri);
			const pos = clampPosition(byteOffsetToPosition(raw.value.buffer, byteOffset), model);

			let providers = this.languageFeaturesService.definitionProvider.ordered(model);
			if (!providers.length) {
				await timeout(2000);
				providers = this.languageFeaturesService.definitionProvider.ordered(model);
			}
			let target: { uri: URI; range: IRange } | undefined;
			for (const provider of providers) {
				try {
					const result = await provider.provideDefinition(
						model,
						new Position(pos.lineNumber, pos.column),
						CancellationToken.None
					);
					target = firstDefinitionLocation(result ?? undefined);
					if (target) {
						break;
					}
				} catch (err) {
					console.warn('[Horizon] definition provider failed', err);
				}
			}
			if (!target) {
				return undefined;
			}

			// Providers may return URIs with different drive-letter casing than the
			// workspace folder (`c:/…` vs `C:/…`); compare case-insensitively.
			let relPath: string | null = null;
			let targetByte: number | null = null;
			const targetPath = target.uri.scheme === Schemas.file ? target.uri.path : null;
			if (targetPath) {
				for (const f of this.workspaceService.getWorkspace().folders) {
					if (f.uri.scheme !== Schemas.file) {
						continue;
					}
					const base = f.uri.path.replace(/\/+$/, '');
					if (targetPath.toLowerCase().startsWith(base.toLowerCase() + '/')) {
						relPath = targetPath.slice(base.length + 1);
						break;
					}
				}
			}
			if (relPath) {
				try {
					const targetRaw = await this.fileService.readFile(target.uri);
					targetByte = positionToByteOffset(targetRaw.value.buffer, {
						lineNumber: target.range.startLineNumber,
						column: target.range.startColumn,
					});
				} catch {
					// in-map matching degrades to path+line; the open fallback still works
				}
			}
			return { path: relPath, line: target.range.startLineNumber, byteOffset: targetByte };
		} finally {
			ref.dispose();
		}
	}

	async semanticTokensFor(
		filePath: string | null,
		byteStart: number | null,
		byteEnd: number | null
	): Promise<{ b: number; l: number; t: string; m?: string[] }[] | undefined> {
		if (!filePath || typeof byteStart !== 'number' || typeof byteEnd !== 'number' || byteEnd <= byteStart) {
			return undefined;
		}
		const uri = await this.resolveWorkspaceFile(filePath, true);
		if (!uri) {
			return undefined;
		}
		const ref = await this.textModelService.createModelReference(uri);
		try {
			const model = ref.object.textEditorModel;
			let providers = this.languageFeaturesService.documentRangeSemanticTokensProvider.ordered(model);
			if (!providers.length) {
				await timeout(2000);
				providers = this.languageFeaturesService.documentRangeSemanticTokensProvider.ordered(model);
			}
			const provider = providers[0];
			if (!provider) {
				return undefined;
			}

			const raw = await this.fileService.readFile(uri);
			const bytes = raw.value.buffer;
			const startPos = clampPosition(byteOffsetToPosition(bytes, byteStart), model);
			const endPos = clampPosition(byteOffsetToPosition(bytes, byteEnd), model);
			const range = new Range(startPos.lineNumber, 1, endPos.lineNumber, model.getLineMaxColumn(endPos.lineNumber));

			const result = await provider.provideDocumentRangeSemanticTokens(model, range, CancellationToken.None);
			if (!result || !result.data || !result.data.length) {
				return undefined;
			}
			const legend = provider.getLegend();

			// Byte offset of each line start, from the raw bytes (handles CRLF).
			const lineStartByte: number[] = [0];
			for (let i = 0; i < bytes.length; i++) {
				if (bytes[i] === 0x0a) {
					lineStartByte.push(i + 1);
				}
			}

			const out: { b: number; l: number; t: string; m?: string[] }[] = [];
			const data = result.data;
			let line = 0;
			let char = 0;
			for (let i = 0; i + 4 < data.length && out.length < 4000; i += 5) {
				const deltaLine = data[i];
				const deltaChar = data[i + 1];
				const length = data[i + 2];
				const typeIdx = data[i + 3];
				const modBits = data[i + 4];
				line += deltaLine;
				char = deltaLine === 0 ? char + deltaChar : deltaChar;
				const lineNumber = line + 1;
				if (lineNumber > model.getLineCount() || line >= lineStartByte.length) {
					continue;
				}
				const lineText = model.getLineContent(lineNumber);
				const b = lineStartByte[line] + utf8ByteLength(lineText.substring(0, char));
				if (b < byteStart || b >= byteEnd) {
					continue;
				}
				const l = utf8ByteLength(lineText.substring(char, char + length));
				const t = legend.tokenTypes[typeIdx] ?? 'unknown';
				const m: string[] = [];
				for (let bit = 0; bit < legend.tokenModifiers.length; bit++) {
					if (modBits & (1 << bit)) {
						m.push(legend.tokenModifiers[bit]);
					}
				}
				out.push(m.length ? { b, l, t, m } : { b, l, t });
			}
			return out.length ? out : undefined;
		} finally {
			ref.dispose();
		}
	}

	/**
	 * Position of the callee identifier inside the call expression. `byte_start`
	 * covers the whole expression, so anchoring there would resolve the leading
	 * crate/module segment; advance to the final path segment instead.
	 */
	private async resolveCalleePosition(
		uri: URI,
		model: ITextModel,
		payload: { callPath?: string | null; line?: number | null; byteStart?: number | null }
	): Promise<IPosition | undefined> {
		const byteStart = payload.byteStart;
		if (typeof byteStart === 'number' && byteStart > 0) {
			try {
				const raw = await this.fileService.readFile(uri);
				let anchor = byteStart;
				const callPath = (payload.callPath || '').trim();
				if (callPath) {
					const lastSegment = callPath.split('::').filter(Boolean).pop() ?? '';
					const idx = lastSegment ? callPath.lastIndexOf(lastSegment) : -1;
					if (idx > 0) {
						anchor += utf8ByteLength(callPath.slice(0, idx));
					}
				}
				return clampPosition(byteOffsetToPosition(raw.value.buffer, anchor), model);
			} catch (err) {
				console.warn('[Horizon] callee byte→position failed', err);
			}
		}
		if (typeof payload.line === 'number' && payload.line > 0) {
			const lineNumber = Math.min(payload.line, model.getLineCount());
			return { lineNumber, column: 1 };
		}
		return undefined;
	}

	private notifyNoDefinition(callPath: string | null | undefined): void {
		this.notificationService.notify({
			severity: Severity.Info,
			message: callPath
				? localize('horizonNoDefFor', "Horizon: no definition found for {0} (rust-analyzer may still be indexing).", callPath)
				: localize('horizonNoDef', "Horizon: no definition found (rust-analyzer may still be indexing)."),
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
	private async resolveWorkspaceFile(filePath: string, quiet = false): Promise<URI | undefined> {
		const folders = this.workspaceService.getWorkspace().folders;
		if (!folders.length) {
			if (!quiet) {
				this.notificationService.error(
					localize('horizonInspectNoWorkspace', "Horizon: open a workspace folder before inspecting a function.")
				);
			}
			return undefined;
		}

		const trimmed = filePath.trim();
		if (!trimmed) {
			if (!quiet) {
				this.notificationService.error(
					localize('horizonInspectBadPath', "Horizon: refusing empty file path.")
				);
			}
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
		if (quiet) {
			return undefined;
		}
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

/** Bytes `s` occupies in UTF-8 — matches the map's byte-offset space. */
function utf8ByteLength(s: string): number {
	return new TextEncoder().encode(s).length;
}

/** Inverse of `byteOffsetToPosition`: 1-based (line, UTF-16 column) → UTF-8 byte offset. */
export function positionToByteOffset(raw: Uint8Array, pos: IPosition): number {
	let lineStart = 0;
	let line = 1;
	while (line < pos.lineNumber && lineStart < raw.length) {
		const nl = raw.indexOf(0x0a, lineStart);
		if (nl < 0) {
			break;
		}
		lineStart = nl + 1;
		line++;
	}
	let lineEnd = raw.indexOf(0x0a, lineStart);
	if (lineEnd < 0) {
		lineEnd = raw.length;
	}
	const lineText = new TextDecoder('utf-8').decode(raw.subarray(lineStart, lineEnd));
	return lineStart + utf8ByteLength(lineText.substring(0, Math.max(0, pos.column - 1)));
}

/** Normalize a Definition result (Location | Location[] | LocationLink[]) to its first target. */
function firstDefinitionLocation(
	result: Location | Location[] | LocationLink[] | undefined
): { uri: URI; range: IRange } | undefined {
	const first = Array.isArray(result) ? result[0] : result;
	if (!first || !first.uri || !first.range) {
		return undefined;
	}
	const link = first as LocationLink;
	return { uri: first.uri, range: link.targetSelectionRange ?? first.range };
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
