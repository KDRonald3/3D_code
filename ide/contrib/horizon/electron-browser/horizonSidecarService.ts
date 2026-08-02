/*---------------------------------------------------------------------------------------------
 *  Copyright (c) Horizon contributors. All rights reserved.
 *--------------------------------------------------------------------------------------------*/

/**
 * Desktop sidecar client — attach-only over loopback HTTP.
 *
 * Spawning `horizon-server` is owned by `./ide/scripts/run.sh` /
 * `Run.ps1` / `run-sidecar.sh` (writes `ide/.cache/horizon-sidecar.url`).
 * The workbench renderer cannot import Node `child_process` as an ESM
 * bare specifier, so process spawn stays outside the EditorPane.
 *
 * Resolution order for the base URL:
 *   1. `HORIZON_SIDECAR_URL` (process env from run.sh)
 *   2. `ide/.cache/horizon-sidecar.url` (and legacy `sidecar.url`)
 *   3. Fail with a clear message pointing at run.sh
 */

import { Disposable } from '../../../../base/common/lifecycle.js';
import { env } from '../../../../base/common/process.js';
import { URI } from '../../../../base/common/uri.js';
import { localize } from '../../../../nls.js';
import { IFileService } from '../../../../platform/files/common/files.js';
import { ILogService } from '../../../../platform/log/common/log.js';
import { IWorkspaceContextService } from '../../../../platform/workspace/common/workspace.js';
import { INativeWorkbenchEnvironmentService } from '../../../services/environment/electron-browser/environmentService.js';
import {
	assertLocalhostSidecarUrl,
	fetchSidecarJson,
	getSidecarSourceHttp,
	HorizonSidecarAnalyseProgress,
	HorizonSidecarAnalyseResult,
	HorizonSidecarSourceQuery,
	HorizonSidecarSourceResult,
	IHorizonSidecarService,
	joinSidecarUrl,
	runSidecarAnalyseHttp,
	sidecarHealthOk,
	stripTrailingSlash,
} from '../common/horizonSidecar.js';

const SIDECAR_URL_FILE_NAMES = ['horizon-sidecar.url', 'sidecar.url'] as const;

export class ElectronHorizonSidecarService extends Disposable implements IHorizonSidecarService {

	declare readonly _serviceBrand: undefined;

	private baseUrl: string | undefined;

	constructor(
		@ILogService private readonly logService: ILogService,
		@INativeWorkbenchEnvironmentService private readonly environmentService: INativeWorkbenchEnvironmentService,
		@IWorkspaceContextService private readonly workspaceService: IWorkspaceContextService,
		@IFileService private readonly fileService: IFileService,
	) {
		super();
	}

	async ensureRunning(): Promise<string> {
		if (this.baseUrl && await sidecarHealthOk(this.baseUrl)) {
			return this.baseUrl;
		}

		const configured = await this.resolveConfiguredUrl();
		if (!configured) {
			throw new Error(localize(
				'horizonSidecarNeedRunSh',
				"Horizon sidecar is not running. Start the IDE with ./ide/scripts/run.sh (or .\\ide\\scripts\\windows\\Run.ps1), which starts horizon-server automatically."
			));
		}

		assertLocalhostSidecarUrl(configured);
		if (!(await sidecarHealthOk(configured))) {
			throw new Error(localize(
				'horizonSidecarUnhealthyDesktop',
				"Sidecar URL {0} failed GET /api/health. Re-run ./ide/scripts/run.sh or ./ide/scripts/run-sidecar.sh.",
				configured
			));
		}

		this.baseUrl = stripTrailingSlash(configured);
		this.logService.info(`[Horizon sidecar] attached to ${this.baseUrl}`);
		return this.baseUrl;
	}

	async analyse(
		repoPath: string,
		onProgress?: (progress: HorizonSidecarAnalyseProgress) => void,
	): Promise<HorizonSidecarAnalyseResult> {
		const trimmed = (repoPath || '').trim();
		if (!trimmed) {
			throw new Error(localize('horizonSidecarEmptyPathDesktop', "analyse path is empty"));
		}
		const base = await this.ensureRunning();
		this.logService.info(`[Horizon sidecar] POST /api/analyse path=${trimmed}`);
		return runSidecarAnalyseHttp(base, trimmed, onProgress);
	}

	async getMap(): Promise<unknown | undefined> {
		const base = await this.ensureRunning();
		const res = await fetchSidecarJson(joinSidecarUrl(base, '/api/map'), {
			headers: { Accept: 'application/json' },
		});
		if (res.status === 404) {
			return undefined;
		}
		if (res.status !== 200) {
			throw new Error(
				typeof res.body?.error === 'string' ? res.body.error : 'failed to load map'
			);
		}
		return res.body;
	}

	async getSource(query: HorizonSidecarSourceQuery): Promise<HorizonSidecarSourceResult> {
		const base = await this.ensureRunning();
		return getSidecarSourceHttp(base, query);
	}

	private async resolveConfiguredUrl(): Promise<string | undefined> {
		const fromEnv = (env['HORIZON_SIDECAR_URL'] || '').trim();
		if (fromEnv) {
			try {
				return assertLocalhostSidecarUrl(fromEnv);
			} catch (err) {
				this.logService.warn(`[Horizon sidecar] ignoring bad HORIZON_SIDECAR_URL: ${err}`);
			}
		}

		for (const candidate of this.urlFileCandidates()) {
			try {
				const content = await this.fileService.readFile(candidate);
				const text = content.value.toString().trim();
				const line = text.split(/\r?\n/).map(s => s.trim()).find(Boolean);
				if (!line) {
					continue;
				}
				const url = assertLocalhostSidecarUrl(line);
				this.logService.info(`[Horizon sidecar] read URL file ${candidate.fsPath}`);
				return url;
			} catch {
				// try next
			}
		}

		return undefined;
	}

	private urlFileCandidates(): URI[] {
		const out: URI[] = [];
		const seen = new Set<string>();
		const push = (uri: URI) => {
			const key = uri.toString();
			if (!seen.has(key)) {
				seen.add(key);
				out.push(uri);
			}
		};

		// Workspace roots and their parents (repo root when IDE opens /workspace).
		for (const folder of this.workspaceService.getWorkspace().folders) {
			push(URI.joinPath(folder.uri, 'ide', '.cache', 'horizon-sidecar.url'));
			push(URI.joinPath(folder.uri, '.cache', 'horizon-sidecar.url'));
			// Parent of workspace folder (e.g. workspace=/workspace/crates/foo).
			push(URI.joinPath(folder.uri, '..', 'ide', '.cache', 'horizon-sidecar.url'));
			push(URI.joinPath(folder.uri, '..', '..', 'ide', '.cache', 'horizon-sidecar.url'));
		}

		// App root when running from a Code-OSS checkout under ide/code-oss.
		const appRoot = this.environmentService.appRoot;
		if (appRoot) {
			const appUri = URI.file(appRoot);
			push(URI.joinPath(appUri, '..', '.cache', 'horizon-sidecar.url'));
			push(URI.joinPath(appUri, '..', '..', 'ide', '.cache', 'horizon-sidecar.url'));
			for (const name of SIDECAR_URL_FILE_NAMES) {
				push(URI.joinPath(appUri, '..', '.cache', name));
			}
		}

		return out;
	}
}
