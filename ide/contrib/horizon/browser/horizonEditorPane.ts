/*---------------------------------------------------------------------------------------------
 *  Copyright (c) Horizon contributors. All rights reserved.
 *--------------------------------------------------------------------------------------------*/

import * as DOM from '../../../../base/browser/dom.js';
import { CancellationToken } from '../../../../base/common/cancellation.js';
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
import { IEditorGroup } from '../../../services/editor/common/editorGroupsService.js';
import { IWebviewElement, IWebviewService } from '../../webview/browser/webview.js';
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
	private readonly _webview = this._register(new MutableDisposable<IWebviewElement>());
	private readonly _webviewEvents = this._register(new DisposableStore());
	private readonly _mapVisible: IContextKey<boolean>;
	private _htmlLoaded = false;

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
	) {
		super(HorizonMapEditorPane.ID, group, telemetryService, themeService, storageService);
		this._mapVisible = HorizonMapVisibleContext.bindTo(contextKeyService);

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
		if (this._dimension) {
			this.layout(this._dimension);
		}
	}

	override clearInput(): void {
		this._mapVisible.set(false);
		this._webviewEvents.clear();
		this._webview.clear();
		this._htmlLoaded = false;
		super.clearInput();
	}

	protected override setEditorVisible(visible: boolean): void {
		this._mapVisible.set(visible);
		super.setEditorVisible(visible);
	}

	override layout(dimension: DOM.Dimension): void {
		this._dimension = dimension;
		if (this._element) {
			this._element.style.width = `${dimension.width}px`;
			this._element.style.height = `${dimension.height}px`;
		}
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
			const webview = this._webviewService.createWebviewElement({
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
			webview.mountTo(this._element, DOM.getWindow(this._element));

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
				await this.runAnalyse(msg.path, true);
				break;
			case 'selectFunction':
				await this._inspectionService.openInspection({
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
				if (msg.filePath) {
					await this._inspectionService.openFileReadonly(msg.filePath);
				}
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
