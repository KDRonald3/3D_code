/*---------------------------------------------------------------------------------------------
 *  Copyright (c) Horizon contributors. All rights reserved.
 *--------------------------------------------------------------------------------------------*/

import * as DOM from '../../../../base/browser/dom.js';
import { CancellationToken } from '../../../../base/common/cancellation.js';
import { DisposableStore, MutableDisposable } from '../../../../base/common/lifecycle.js';
import { basename } from '../../../../base/common/resources.js';
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
import { IWorkspaceContextService } from '../../../../platform/workspace/common/workspace.js';
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
import { buildHorizonMapFallbackHtml, buildHorizonMapHtml, horizonMediaFileRoot } from './horizonHtml.js';
import { HorizonMapInput } from './horizonInput.js';

import './media/horizonEditor.css';

export const HorizonMapVisibleContext = new RawContextKey<boolean>(HORIZON_MAP_VISIBLE_CONTEXT, false);

/**
 * Built-in EditorPane hosting the Horizon Map webview (first-class workbench contrib).
 *
 * Sidecar analyse / inspection (W3–W4) attach here; MVP loads map media and
 * handles the webview bridge protocol.
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
		@IWorkspaceContextService private readonly _workspaceService: IWorkspaceContextService,
		@INotificationService private readonly _notificationService: INotificationService,
		@IFileDialogService private readonly _fileDialogService: IFileDialogService,
		@IContextKeyService contextKeyService: IContextKeyService,
	) {
		super(HorizonMapEditorPane.ID, group, telemetryService, themeService, storageService);
		this._mapVisible = HorizonMapVisibleContext.bindTo(contextKeyService);
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

	/** Re-run analyse (command palette). Sidecar wiring lands in W4. */
	async analyseWorkspace(pathOverride?: string): Promise<void> {
		await this.runAnalyse(pathOverride);
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
				await this.runAnalyse(msg.path);
				break;
			case 'selectFunction':
				// W3: open read-only inspection on the real file/range.
				this._notificationService.notify({
					severity: Severity.Info,
					message: localize(
						'horizonSelectFunctionStub',
						"Horizon: selected {0} (inspection canvas lands in W3).",
						msg.functionName || msg.functionId || 'function'
					),
				});
				break;
			case 'selectFile':
				if (msg.filePath) {
					this._notificationService.notify({
						severity: Severity.Info,
						message: localize('horizonSelectFileStub', "Horizon: file {0} (read-only inspection in W3).", msg.filePath),
					});
				}
				break;
			case 'openMapJson':
				await this.openMapJson();
				break;
			case 'sourceRequest':
				this.post({
					type: 'sourceResult',
					requestId: msg.requestId,
					error: 'unavailable',
					message: 'Source slices require the Horizon sidecar (W4).',
				});
				break;
		}
	}

	private async onReady(): Promise<void> {
		const folder = this._workspaceService.getWorkspace().folders[0];
		if (folder) {
			this.post({
				type: 'workspaceInfo',
				root: folder.uri.fsPath || folder.uri.path,
				name: basename(folder.uri) || folder.name,
			});
		}
		this.postTheme();
	}

	private postTheme(): void {
		const scheme = this.themeService.getColorTheme().type;
		const theme =
			scheme === ColorScheme.DARK || scheme === ColorScheme.HIGH_CONTRAST_DARK
				? 'dark'
				: 'light';
		this.post({ type: 'theme', theme });
	}

	private async runAnalyse(pathOverride?: string): Promise<void> {
		const folder = this._workspaceService.getWorkspace().folders[0];
		const target = (pathOverride || '').trim() || (folder ? (folder.uri.fsPath || folder.uri.path) : '');
		if (!target) {
			this.post({
				type: 'analyseResult',
				status: 'error',
				error: 'No workspace folder open.',
			});
			this._notificationService.error(localize('horizonNoWorkspace', "Horizon: open a Rust workspace folder to analyse."));
			return;
		}

		// W4 will spawn/attach horizon-server. Surface a clear interim status.
		this.post({
			type: 'analyseResult',
			status: 'running',
			path: target,
		});
		this.post({
			type: 'analyseResult',
			status: 'failed',
			path: target,
			error: 'Sidecar analyse is not wired in the workbench contrib yet (W4). Use ./ide/scripts/run-sidecar.sh meanwhile.',
		});
		this._notificationService.notify({
			severity: Severity.Warning,
			message: localize(
				'horizonAnalyseStub',
				"Horizon analyse: sidecar not yet attached to the Map EditorPane (W4)."
			),
		});
	}

	private async openMapJson(): Promise<void> {
		const filters: FileFilter[] = [{ name: 'JSON', extensions: ['json'] }];
		const defaultUri = this._workspaceService.getWorkspace().folders[0]?.uri;
		const picked = await this._fileDialogService.showOpenDialog({
			canSelectFiles: true,
			canSelectFolders: false,
			canSelectMany: false,
			filters,
			title: localize('horizonOpenMapJson', "Open Horizon map JSON"),
			defaultUri,
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
