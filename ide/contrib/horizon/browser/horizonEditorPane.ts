/*---------------------------------------------------------------------------------------------
 *  Copyright (c) Horizon contributors. All rights reserved.
 *--------------------------------------------------------------------------------------------*/

import * as DOM from '../../../../base/browser/dom.js';
import { CancellationToken } from '../../../../base/common/cancellation.js';
import { Event } from '../../../../base/common/event.js';
import { DisposableStore, MutableDisposable } from '../../../../base/common/lifecycle.js';
import { URI } from '../../../../base/common/uri.js';
import { generateUuid } from '../../../../base/common/uuid.js';
import { localize } from '../../../../nls.js';
import { IContextKey, IContextKeyService, RawContextKey } from '../../../../platform/contextkey/common/contextkey.js';
import { IEditorOptions } from '../../../../platform/editor/common/editor.js';
import { FileFilter, IFileDialogService } from '../../../../platform/dialogs/common/dialogs.js';
import { IFileService } from '../../../../platform/files/common/files.js';
import { INotificationService, Severity } from '../../../../platform/notification/common/notification.js';
import { IStorageService } from '../../../../platform/storage/common/storage.js';
import { ITelemetryService } from '../../../../platform/telemetry/common/telemetry.js';
import { ColorScheme } from '../../../../platform/theme/common/theme.js';
import { IThemeService } from '../../../../platform/theme/common/themeService.js';
import { EditorPane } from '../../../browser/parts/editor/editorPane.js';
import { IEditorOpenContext } from '../../../common/editor.js';
import { EditorInput } from '../../../common/editor/editorInput.js';
import { IEditorGroup, IEditorGroupsService } from '../../../services/editor/common/editorGroupsService.js';
import { IWorkbenchLayoutService, Parts } from '../../../services/layout/browser/layoutService.js';
import { IOverlayWebview, IWebviewService } from '../../webview/browser/webview.js';
import {
	HostToWebviewMessage,
	HORIZON_MAP_EDITOR_ID,
	HORIZON_MAP_VISIBLE_CONTEXT,
	WebviewToHostMessage,
} from '../common/horizon.js';
import { IHorizonSidecarService } from '../common/horizonSidecar.js';
import { HorizonAnalyseProgress, IHorizonAnalysisService } from './horizonAnalysis.js';
import { buildHorizonMapFallbackHtml, buildHorizonMapHtml, horizonMediaFileRoot } from './horizonHtml.js';
import { IHorizonInspectionService } from './horizonInspection.js';
import { HorizonMapInput } from './horizonInput.js';

import './media/horizonEditor.css';

export const HorizonMapVisibleContext = new RawContextKey<boolean>(HORIZON_MAP_VISIBLE_CONTEXT, false);

/**
 * Built-in EditorPane hosting the Horizon Map webview (first-class workbench contrib).
 *
 * Analyse root + sidecar calls go through {@link IHorizonAnalysisService}.
 * Function / file selection opens read-only inspection via {@link IHorizonInspectionService}.
 */
export class HorizonMapEditorPane extends EditorPane {

	static readonly ID = HORIZON_MAP_EDITOR_ID;

	private _element: HTMLElement | undefined;
	private _dimension: DOM.Dimension | undefined;
	private readonly _webview = this._register(new MutableDisposable<IOverlayWebview>());
	private readonly _webviewEvents = this._register(new DisposableStore());
	private readonly _mapVisible: IContextKey<boolean>;
	private _htmlLoaded = false;
	private _visible = false;

	constructor(
		group: IEditorGroup,
		@ITelemetryService telemetryService: ITelemetryService,
		@IThemeService themeService: IThemeService,
		@IStorageService storageService: IStorageService,
		@IWebviewService private readonly _webviewService: IWebviewService,
		@IFileService private readonly _fileService: IFileService,
		@INotificationService private readonly _notificationService: INotificationService,
		@IFileDialogService private readonly _fileDialogService: IFileDialogService,
		@IHorizonAnalysisService private readonly _analysisService: IHorizonAnalysisService,
		@IHorizonInspectionService private readonly _inspectionService: IHorizonInspectionService,
		@IHorizonSidecarService private readonly _sidecarService: IHorizonSidecarService,
		@IContextKeyService contextKeyService: IContextKeyService,
		@IWorkbenchLayoutService private readonly _layoutService: IWorkbenchLayoutService,
		@IEditorGroupsService editorGroupsService: IEditorGroupsService,
	) {
		super(HorizonMapEditorPane.ID, group, telemetryService, themeService, storageService);
		this._mapVisible = HorizonMapVisibleContext.bindTo(contextKeyService);

		// An overlay webview is positioned over the editor rather than parented
		// into it, so anything that moves the editor must re-place it.
		const part = editorGroupsService.getPart(group);
		this._register(Event.any(
			part.onDidScroll,
			part.onDidAddGroup,
			part.onDidRemoveGroup,
			part.onDidMoveGroup
		)(() => {
			if (this._webview.value && this._visible) {
				this.syncWebviewBounds();
			}
		}));

		this._register(this._analysisService.onDidChangeFolder(folder => {
			if (!folder || !this._webview.value) {
				return;
			}
			this.post({
				type: 'workspaceInfo',
				root: folder.path,
				name: folder.name,
			});
		}));

		this._register(this._analysisService.onDidChangeProgress(progress => {
			if (!this._webview.value) {
				return;
			}
			this.postAnalyseProgress(progress);
		}));
	}

	protected createEditor(parent: HTMLElement): void {
		const element = document.createElement('div');
		element.classList.add('horizon-map-editor');
		element.id = `horizon-map-editor-${generateUuid()}`;
		parent.appendChild(element);
		this._element = element;
	}

	override async setInput(input: EditorInput, options: IEditorOptions, context: IEditorOpenContext, token: CancellationToken): Promise<void> {
		await super.setInput(input, options, context, token);
		if (token.isCancellationRequested || !(input instanceof HorizonMapInput)) {
			return;
		}
		await this.ensureWebview();
		// Re-shown after another editor held the group: take the overlay back.
		if (this._visible) {
			this.claimWebview();
		}
		if (this._dimension) {
			this.layout(this._dimension);
		}
	}

	override clearInput(): void {
		this._mapVisible.set(false);
		// Release, never dispose. The workbench calls this whenever another
		// editor takes over the group; destroying the webview here rebuilt the
		// Map from scratch on the way back, losing the selection, the
		// inspector, and the source pane.
		this._webview.value?.release(this);
		super.clearInput();
	}

	protected override setEditorVisible(visible: boolean): void {
		this._visible = visible;
		this._mapVisible.set(visible);
		if (this._webview.value) {
			if (visible) {
				this.claimWebview();
			} else {
				this._webview.value.release(this);
			}
		}
		super.setEditorVisible(visible);
	}

	override layout(dimension: DOM.Dimension): void {
		this._dimension = dimension;
		if (this._element) {
			this._element.style.width = `${dimension.width}px`;
			this._element.style.height = `${dimension.height}px`;
		}
		if (this._webview.value && this._visible) {
			this.syncWebviewBounds();
		}
	}

	private claimWebview(): void {
		const webview = this._webview.value;
		if (!webview || !this._element) {
			return;
		}
		webview.claim(this, DOM.getWindow(this._element), undefined);
		this.syncWebviewBounds();
	}

	/** Keep the overlay sitting exactly over this pane's slot in the editor. */
	private syncWebviewBounds(): void {
		const webview = this._webview.value;
		if (!webview || !this._element?.isConnected) {
			return;
		}
		const root = this._layoutService.getContainer(DOM.getWindow(this._element), Parts.EDITOR_PART);
		webview.layoutWebviewOverElement(this._element.parentElement ?? this._element, this._dimension, root);
	}

	override focus(): void {
		super.focus();
		this._webview.value?.focus();
	}

	override dispose(): void {
		this._mapVisible.set(false);
		this._element?.remove();
		this._element = undefined;
		super.dispose();
	}

	/** Post a typed message into the map webview. */
	post(message: HostToWebviewMessage): void {
		void this._webview.value?.postMessage(message);
	}

	/** Re-run analyse for the current Horizon folder (command / sidebar). */
	async analyseWorkspace(pathOverride?: string): Promise<void> {
		await this.runAnalyse(pathOverride, true);
	}

	private async ensureWebview(): Promise<void> {
		if (!this._element) {
			return;
		}

		if (!this._webview.value) {
			const mediaRoot = horizonMediaFileRoot();
			// Overlay, not element: an element webview is parented into the
			// editor's DOM, and the workbench re-parents that DOM when tabs
			// change - which reloads the iframe and blanks the Map. The overlay
			// lives in its own container and is positioned over the editor.
			const webview = this._webviewService.createWebviewOverlay({
				title: localize('horizonMapWebviewTitle', "Horizon Map"),
				options: {
					retainContextWhenHidden: true,
				},
				contentOptions: {
					allowScripts: true,
					localResourceRoots: [mediaRoot],
				},
				extension: undefined,
			});
			this._webview.value = webview;
			if (this._visible) {
				this.claimWebview();
			}

			this._webviewEvents.clear();
			this._webviewEvents.add(webview.onMessage(e => {
				void this.onWebviewMessage(e.message);
			}));
			this._webviewEvents.add(this.themeService.onDidColorThemeChange(() => {
				this.postTheme();
			}));
		}

		if (!this._htmlLoaded) {
			try {
				const html = await buildHorizonMapHtml(this._fileService);
				this._webview.value.setHtml(html);
			} catch (err) {
				console.warn('[Horizon] failed to load map media; using fallback', err);
				this._webview.value.setHtml(buildHorizonMapFallbackHtml());
			}
			this._htmlLoaded = true;
		}
	}

	private async onWebviewMessage(raw: unknown): Promise<void> {
		if (!raw || typeof raw !== 'object') {
			return;
		}
		const type = (raw as { type?: unknown }).type;
		if (typeof type !== 'string') {
			return;
		}
		const msg = raw as WebviewToHostMessage;
		switch (msg.type) {
			case 'ready':
				await this.onReady();
				break;
			case 'analyse':
				// Webview is untrusted: ignore optional path overrides. Analyse
				// the host-owned Horizon folder only (set via QuickPick / Browse).
				if (msg.path && msg.path.trim()) {
					console.warn('[Horizon] ignoring webview analyse path override');
				}
				await this.runAnalyse(undefined, true);
				break;
			case 'selectFunction':
				// Selection never takes over the editor area on its own: the Map's own
				// Inspector already shows the body, and hovering previews it without
				// even changing selection. The rust-analyzer inspection editor is
				// opened by the explicit "Open Selected Function in Editor" command.
				this._inspectionService.setPendingSelection({
					type: 'selectFunction',
					functionId: msg.functionId,
					fileId: msg.fileId ?? null,
					filePath: msg.filePath ?? null,
					functionName: msg.functionName ?? null,
					line: msg.line ?? null,
					byteStart: msg.byteStart ?? null,
					byteEnd: msg.byteEnd ?? null,
					contentHash: msg.contentHash ?? null,
				});
				break;
			case 'selectFile':
				// Deliberately inert. Selecting a file card is a browsing gesture: it
				// updates the Map's own Inspector and nothing else. Opening an editor
				// here stole focus from the Map and fired a notification per click.
				// Opening code stays tied to selecting a *function*.
				break;
			case 'hoverRequest': {
				try {
					const contents = await this._inspectionService.hoverAt(msg.filePath, msg.byteOffset ?? null);
					this.post(contents && contents.length
						? { type: 'hoverResult', requestId: msg.requestId, contents }
						: { type: 'hoverResult', requestId: msg.requestId, error: 'no_hover' });
				} catch (err) {
					this.post({
						type: 'hoverResult',
						requestId: msg.requestId,
						error: err instanceof Error ? err.message : String(err),
					});
				}
				break;
			}
			case 'definitionAtRequest': {
				try {
					const target = await this._inspectionService.definitionAt(msg.filePath, msg.byteOffset ?? null);
					this.post(target
						? { type: 'definitionAtResult', requestId: msg.requestId, target }
						: { type: 'definitionAtResult', requestId: msg.requestId, error: 'no_definition' });
				} catch (err) {
					this.post({
						type: 'definitionAtResult',
						requestId: msg.requestId,
						error: err instanceof Error ? err.message : String(err),
					});
				}
				break;
			}
			case 'semanticTokensRequest': {
				try {
					const tokens = await this._inspectionService.semanticTokensFor(
						msg.filePath,
						msg.byteStart ?? null,
						msg.byteEnd ?? null
					);
					this.post(tokens && tokens.length
						? { type: 'semanticTokensResult', requestId: msg.requestId, tokens }
						: { type: 'semanticTokensResult', requestId: msg.requestId, error: 'no_tokens' });
				} catch (err) {
					this.post({
						type: 'semanticTokensResult',
						requestId: msg.requestId,
						error: err instanceof Error ? err.message : String(err),
					});
				}
				break;
			}
			case 'openDefinition':
				await this._inspectionService.openDefinitionAt({
					filePath: msg.filePath,
					callPath: msg.callPath ?? null,
					line: msg.line ?? null,
					byteStart: msg.byteStart ?? null,
					byteEnd: msg.byteEnd ?? null,
				});
				break;
			case 'openMapJson':
				await this.openMapJson();
				break;
			case 'chooseFolder':
				await this.chooseFolderFromWebview();
				break;
			case 'sourceRequest':
				await this.handleSourceRequest(msg);
				break;
		}
	}

	private async handleSourceRequest(msg: Extract<WebviewToHostMessage, { type: 'sourceRequest' }>): Promise<void> {
		try {
			const result = await this._sidecarService.getSource({
				path: msg.path,
				byteStart: msg.byteStart,
				byteEnd: msg.byteEnd,
				expectedHash: msg.expectedHash,
			});
			if (result.ok) {
				this.post({
					type: 'sourceResult',
					requestId: msg.requestId,
					tokens: result.tokens,
				});
			} else {
				this.post({
					type: 'sourceResult',
					requestId: msg.requestId,
					error: result.errorKind,
					errorKind: result.errorKind,
					message: result.error,
				});
			}
		} catch (err) {
			const message = err instanceof Error ? err.message : String(err);
			this.post({
				type: 'sourceResult',
				requestId: msg.requestId,
				error: 'unavailable',
				message,
			});
		}
	}

	private async onReady(): Promise<void> {
		const folder = this._analysisService.getFolder();
		if (folder) {
			this.post({
				type: 'workspaceInfo',
				root: folder.path,
				name: folder.name,
			});
		}
		this.postTheme();

		const lastMap = this._analysisService.lastMap;
		if (lastMap !== undefined) {
			this.post({
				type: 'mapData',
				map: lastMap,
				label: folder?.name || folder?.path,
			});
			this.post({
				type: 'analyseResult',
				status: 'done',
				path: folder?.path,
				label: folder?.name,
				map: lastMap,
			});
			return;
		}

		const progress = this._analysisService.progress;
		if (progress.status === 'running' || progress.status === 'failed' || progress.status === 'error') {
			this.postAnalyseProgress(progress);
		}
	}

	private postTheme(): void {
		const scheme = this.themeService.getColorTheme().type;
		const theme =
			scheme === ColorScheme.DARK || scheme === ColorScheme.HIGH_CONTRAST_DARK
				? 'dark'
				: 'light';
		this.post({ type: 'theme', theme });
	}

	private postAnalyseProgress(progress: HorizonAnalyseProgress): void {
		this.post({
			type: 'analyseResult',
			status: progress.status === 'idle' ? 'idle' : progress.status,
			path: progress.path,
			label: progress.label,
			error: progress.error,
			elapsed_ms: progress.elapsed_ms,
			map: progress.map,
		});
		if (progress.status === 'done' && progress.map !== undefined) {
			this.post({
				type: 'mapData',
				map: progress.map,
				label: progress.label || progress.path,
			});
		}
	}

	private async runAnalyse(pathOverride?: string, notifyOnFailure = false): Promise<void> {
		const progress = await this._analysisService.analyse(pathOverride);
		// Progress events already mirror into the webview when mounted; post once
		// more in case the listener raced with ensureWebview.
		if (this._webview.value) {
			this.postAnalyseProgress(progress);
		}
		if (notifyOnFailure && (progress.status === 'failed' || progress.status === 'error')) {
			this._notificationService.notify({
				severity: Severity.Warning,
				message: localize(
					'horizonAnalyseFailed',
					"Horizon analyse: {0}",
					progress.error || 'failed'
				),
			});
		}
	}

	/** Webview "Choose folder…" → same QuickPick path as the sidebar. */
	private async chooseFolderFromWebview(): Promise<void> {
		const folder = await this._analysisService.chooseFolder();
		if (!folder) {
			return;
		}
		await this.runAnalyse(folder.path, true);
	}

	private async openMapJson(): Promise<void> {
		const filters: FileFilter[] = [{ name: 'JSON', extensions: ['json'] }];
		const folder = this._analysisService.getFolder();
		const picked = await this._fileDialogService.showOpenDialog({
			canSelectFiles: true,
			canSelectFolders: false,
			canSelectMany: false,
			filters,
			title: localize('horizonOpenMapJson', "Open Horizon map JSON"),
			defaultUri: folder ? URI.file(folder.path) : undefined,
		});
		if (!picked?.[0]) {
			return;
		}
		try {
			const raw = await this._fileService.readFile(picked[0]);
			const map = JSON.parse(raw.value.toString()) as unknown;
			this.post({ type: 'mapData', map, label: picked[0].fsPath || picked[0].path });
			this.post({
				type: 'analyseResult',
				status: 'done',
				path: picked[0].fsPath || picked[0].path,
				map,
			});
		} catch (err) {
			const message = err instanceof Error ? err.message : String(err);
			this.post({ type: 'mapData', map: null, error: message });
			this._notificationService.error(localize('horizonMapJsonError', "Horizon: {0}", message));
		}
	}
}
