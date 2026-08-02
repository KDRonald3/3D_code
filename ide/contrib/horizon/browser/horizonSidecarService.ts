/*---------------------------------------------------------------------------------------------
 *  Copyright (c) Horizon contributors. All rights reserved.
 *--------------------------------------------------------------------------------------------*/

import { Disposable } from '../../../../base/common/lifecycle.js';
import { env } from '../../../../base/common/process.js';
import { IConfigurationService } from '../../../../platform/configuration/common/configuration.js';
import { localize } from '../../../../nls.js';
import { InstantiationType, registerSingleton } from '../../../../platform/instantiation/common/extensions.js';
import { ILogService } from '../../../../platform/log/common/log.js';
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

/**
 * Attach-only sidecar client for the shared browser layer.
 *
 * Prefers `HORIZON_SIDECAR_URL` (set by `./ide/scripts/run.sh`). Does not spawn
 * processes — desktop spawn lives in `electron-browser/horizonSidecarService.ts`.
 */
export class BrowserHorizonSidecarService extends Disposable implements IHorizonSidecarService {

	declare readonly _serviceBrand: undefined;

	private _baseUrl: string | undefined;

	constructor(
		@ILogService private readonly logService: ILogService,
		@IConfigurationService private readonly configurationService: IConfigurationService,
	) {
		super();
	}

	async ensureRunning(): Promise<string> {
		if (this._baseUrl && await sidecarHealthOk(this._baseUrl)) {
			return this._baseUrl;
		}

		const configured = this.resolveConfiguredUrl();
		if (!configured) {
			throw new Error(localize(
				'horizonSidecarNoUrl',
				"Horizon sidecar is not running. Launch the IDE with ./ide/scripts/run.sh (starts the sidecar), or start ./ide/scripts/run-sidecar.sh and point the IDE at it — set the `horizon.sidecarUrl` setting to http://127.0.0.1:PORT (required in the browser), or export HORIZON_SIDECAR_URL on desktop."
			));
		}

		assertLocalhostSidecarUrl(configured);
		if (!(await sidecarHealthOk(configured))) {
			throw new Error(localize(
				'horizonSidecarUnhealthy',
				"HORIZON_SIDECAR_URL is set ({0}) but GET /api/health failed. Start ./ide/scripts/run-sidecar.sh or re-run ./ide/scripts/run.sh.",
				configured
			));
		}

		this._baseUrl = stripTrailingSlash(configured);
		this.logService.info(`[Horizon sidecar] attached to ${this._baseUrl}`);
		return this._baseUrl;
	}

	async analyse(
		repoPath: string,
		onProgress?: (progress: HorizonSidecarAnalyseProgress) => void,
	): Promise<HorizonSidecarAnalyseResult> {
		const trimmed = (repoPath || '').trim();
		if (!trimmed) {
			throw new Error(localize('horizonSidecarEmptyPath', "analyse path is empty"));
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

	private resolveConfiguredUrl(): string | undefined {
		// `horizon.sidecarUrl` first: in web, `env` is always {} so the setting is
		// the only channel; on desktop an explicit setting should still win.
		const setting = (this.configurationService.getValue<string>('horizon.sidecarUrl') || '').trim();
		const raw = setting || (env['HORIZON_SIDECAR_URL'] || '').trim();
		if (!raw) {
			return undefined;
		}
		const source = setting ? 'horizon.sidecarUrl' : 'HORIZON_SIDECAR_URL';
		try {
			const u = new URL(raw);
			return `${u.protocol}//${u.host}`;
		} catch {
			throw new Error(`Invalid ${source}: ${raw}`);
		}
	}
}

registerSingleton(IHorizonSidecarService, BrowserHorizonSidecarService, InstantiationType.Delayed);
