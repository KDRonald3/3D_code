/*---------------------------------------------------------------------------------------------
 *  Copyright (c) Horizon contributors. All rights reserved.
 *--------------------------------------------------------------------------------------------*/

import { Emitter, Event } from '../../../../base/common/event.js';
import { Disposable } from '../../../../base/common/lifecycle.js';
import { basename } from '../../../../base/common/resources.js';
import { URI } from '../../../../base/common/uri.js';
import { localize } from '../../../../nls.js';
import { IFileDialogService } from '../../../../platform/dialogs/common/dialogs.js';
import { createDecorator } from '../../../../platform/instantiation/common/instantiation.js';
import { IQuickInputService, IQuickPickItem } from '../../../../platform/quickinput/common/quickInput.js';
import { IStorageService, StorageScope, StorageTarget } from '../../../../platform/storage/common/storage.js';
import { IWorkspaceContextService } from '../../../../platform/workspace/common/workspace.js';
import { HORIZON_FOLDER_STORAGE_KEY } from '../common/horizon.js';
import { IHorizonSidecarService } from '../common/horizonSidecar.js';

/**
 * Analyse progress aligned with Host→webview `analyseResult`.
 * Sidecar HTTP/spawn is delegated to {@link IHorizonSidecarService}.
 */
export type HorizonAnalyseStatus = 'idle' | 'running' | 'done' | 'failed' | 'error';

export interface HorizonAnalyseProgress {
	readonly status: HorizonAnalyseStatus;
	readonly path?: string;
	readonly label?: string;
	readonly error?: string;
	readonly elapsed_ms?: number;
	readonly map?: unknown;
}

export interface HorizonFolderInfo {
	readonly path: string;
	readonly name: string;
}

export const IHorizonAnalysisService = createDecorator<IHorizonAnalysisService>('horizonAnalysisService');

/**
 * Host-owned analyse root + sidecar analyse API.
 *
 * Folder persistence and QuickPick live here so sidebar / status bar / Map /
 * auto-start share one source of truth. Sidecar HTTP is via {@link IHorizonSidecarService}.
 */
export interface IHorizonAnalysisService {
	readonly _serviceBrand: undefined;

	readonly onDidChangeFolder: Event<HorizonFolderInfo | undefined>;
	readonly onDidChangeProgress: Event<HorizonAnalyseProgress>;

	readonly progress: HorizonAnalyseProgress;
	/** Last successful map payload (preload when Map opens). */
	readonly lastMap: unknown | undefined;

	getFolder(): HorizonFolderInfo | undefined;
	setFolderPath(path: string): HorizonFolderInfo;

	/**
	 * QuickPick of workspace folders + "Browse…" (native folder dialog).
	 * Persists the choice; does not analyse by itself.
	 */
	chooseFolder(): Promise<HorizonFolderInfo | undefined>;

	/** Attach or spawn horizon-server (loopback). */
	ensureSidecar(): Promise<void>;

	/**
	 * Ensure sidecar then analyse `pathOverride` or the current Horizon folder.
	 * Persists pathOverride when provided. Does not block the UI forever.
	 */
	analyse(pathOverride?: string): Promise<HorizonAnalyseProgress>;
}

interface FolderPickItem extends IQuickPickItem {
	readonly folderPath?: string;
	readonly browse?: boolean;
}

export class HorizonAnalysisService extends Disposable implements IHorizonAnalysisService {

	declare readonly _serviceBrand: undefined;

	private readonly _onDidChangeFolder = this._register(new Emitter<HorizonFolderInfo | undefined>());
	readonly onDidChangeFolder = this._onDidChangeFolder.event;

	private readonly _onDidChangeProgress = this._register(new Emitter<HorizonAnalyseProgress>());
	readonly onDidChangeProgress = this._onDidChangeProgress.event;

	private _progress: HorizonAnalyseProgress = { status: 'idle' };
	private _lastMap: unknown | undefined;
	private _folder: HorizonFolderInfo | undefined;
	private _analyseGeneration = 0;

	constructor(
		@IStorageService private readonly storageService: IStorageService,
		@IWorkspaceContextService private readonly workspaceService: IWorkspaceContextService,
		@IQuickInputService private readonly quickInputService: IQuickInputService,
		@IFileDialogService private readonly fileDialogService: IFileDialogService,
		@IHorizonSidecarService private readonly sidecarService: IHorizonSidecarService,
	) {
		super();
		this._folder = this.resolveInitialFolder();
		this._register(this.workspaceService.onDidChangeWorkspaceFolders(() => {
			if (!this._folder) {
				this._folder = this.defaultWorkspaceFolder();
				this._onDidChangeFolder.fire(this._folder);
			}
		}));
	}

	get progress(): HorizonAnalyseProgress {
		return this._progress;
	}

	get lastMap(): unknown | undefined {
		return this._lastMap;
	}

	getFolder(): HorizonFolderInfo | undefined {
		return this._folder ?? this.defaultWorkspaceFolder();
	}

	setFolderPath(path: string): HorizonFolderInfo {
		const trimmed = path.trim();
		const info: HorizonFolderInfo = {
			path: trimmed,
			name: this.nameForPath(trimmed),
		};
		this._folder = info;
		this.storageService.store(HORIZON_FOLDER_STORAGE_KEY, trimmed, StorageScope.WORKSPACE, StorageTarget.USER);
		this._onDidChangeFolder.fire(info);
		return info;
	}

	async chooseFolder(): Promise<HorizonFolderInfo | undefined> {
		const current = this.getFolder();
		const items: FolderPickItem[] = [];

		for (const folder of this.workspaceService.getWorkspace().folders) {
			const folderPath = folder.uri.fsPath || folder.uri.path;
			items.push({
				label: folder.name || basename(folder.uri) || folderPath,
				description: folderPath,
				folderPath,
				picked: current?.path === folderPath,
			});
		}

		items.push({
			label: localize('horizonBrowseFolder', 'Browse…'),
			description: localize('horizonBrowseFolderDesc', 'Open a folder with the system dialog'),
			browse: true,
		});

		const picked = await this.quickInputService.pick(items, {
			placeHolder: localize('horizonChooseFolderPlaceholder', 'Choose Horizon analyse folder'),
			matchOnDescription: true,
			activeItem: items.find(i => i.picked) ?? items[0],
		});

		if (!picked) {
			return undefined;
		}

		if (picked.browse) {
			const defaultUri = current
				? URI.file(current.path)
				: this.workspaceService.getWorkspace().folders[0]?.uri;
			const dialogPicked = await this.fileDialogService.showOpenDialog({
				canSelectFiles: false,
				canSelectFolders: true,
				canSelectMany: false,
				title: localize('horizonChooseFolderDialog', 'Choose folder to analyse'),
				defaultUri,
			});
			if (!dialogPicked?.[0]) {
				return undefined;
			}
			const root = dialogPicked[0].fsPath || dialogPicked[0].path;
			return this.setFolderPath(root);
		}

		if (picked.folderPath) {
			return this.setFolderPath(picked.folderPath);
		}

		return undefined;
	}

	/** Attach or spawn `horizon-server`; health-check `/api/health`. */
	async ensureSidecar(): Promise<void> {
		await this.sidecarService.ensureRunning();
	}

	async analyse(pathOverride?: string): Promise<HorizonAnalyseProgress> {
		const override = (pathOverride || '').trim();
		const folder = override ? this.setFolderPath(override) : this.getFolder();
		if (!folder) {
			const progress: HorizonAnalyseProgress = {
				status: 'error',
				error: localize('horizonNoFolder', 'No workspace folder open.'),
			};
			this.setProgress(progress);
			return progress;
		}

		const generation = ++this._analyseGeneration;
		const started = Date.now();
		this.setProgress({ status: 'running', path: folder.path, label: folder.name });

		try {
			await this.ensureSidecar();
			if (generation !== this._analyseGeneration) {
				return this._progress;
			}

			const result = await this.runSidecarAnalyse(folder.path);
			if (generation !== this._analyseGeneration) {
				return this._progress;
			}

			const progress: HorizonAnalyseProgress = {
				...result,
				path: folder.path,
				label: folder.name,
				elapsed_ms: result.elapsed_ms ?? (Date.now() - started),
			};
			if (progress.status === 'done' && progress.map !== undefined) {
				this._lastMap = progress.map;
			}
			this.setProgress(progress);
			return progress;
		} catch (err) {
			if (generation !== this._analyseGeneration) {
				return this._progress;
			}
			const message = err instanceof Error ? err.message : String(err);
			const progress: HorizonAnalyseProgress = {
				status: 'failed',
				path: folder.path,
				label: folder.name,
				error: message,
				elapsed_ms: Date.now() - started,
			};
			this.setProgress(progress);
			return progress;
		}
	}

	/**
	 * POST `/api/analyse`, poll until done, return map JSON via sidecar service.
	 */
	protected async runSidecarAnalyse(analysePath: string): Promise<HorizonAnalyseProgress> {
		const result = await this.sidecarService.analyse(analysePath, progress => {
			this.setProgress({
				status: progress.status,
				path: progress.path ?? analysePath,
				label: this._folder?.name,
				error: progress.error,
				elapsed_ms: progress.elapsed_ms,
			});
		});
		return {
			status: 'done',
			path: result.path,
			map: result.map,
			elapsed_ms: result.elapsed_ms,
		};
	}

	private setProgress(progress: HorizonAnalyseProgress): void {
		this._progress = progress;
		this._onDidChangeProgress.fire(progress);
	}

	private resolveInitialFolder(): HorizonFolderInfo | undefined {
		const stored = this.storageService.get(HORIZON_FOLDER_STORAGE_KEY, StorageScope.WORKSPACE);
		if (stored && stored.trim()) {
			return { path: stored.trim(), name: this.nameForPath(stored.trim()) };
		}
		return this.defaultWorkspaceFolder();
	}

	private defaultWorkspaceFolder(): HorizonFolderInfo | undefined {
		const folder = this.workspaceService.getWorkspace().folders[0];
		if (!folder) {
			return undefined;
		}
		const path = folder.uri.fsPath || folder.uri.path;
		return {
			path,
			name: folder.name || basename(folder.uri) || path,
		};
	}

	private nameForPath(path: string): string {
		try {
			return basename(URI.file(path)) || path;
		} catch {
			const parts = path.replace(/[\\/]+$/, '').split(/[\\/]/);
			return parts[parts.length - 1] || path;
		}
	}
}
