/*---------------------------------------------------------------------------------------------
 *  Copyright (c) Horizon contributors. All rights reserved.
 *--------------------------------------------------------------------------------------------*/

import { ChildProcess, spawn, spawnSync } from 'child_process';
import * as fs from 'fs';
import * as path from 'path';
import { Disposable } from '../../../../base/common/lifecycle.js';
import { env } from '../../../../base/common/process.js';
import { localize } from '../../../../nls.js';
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

const LISTEN_URL_RE = /https?:\/\/(?:127\.0\.0\.1|localhost|\[::1\]):\d+\/?/i;
const HEALTH_TIMEOUT_MS = 120_000;
const STOP_GRACE_MS = 3_000;

/** Prefer user-requested name; also accept older `sidecar.url` from early scripts. */
const SIDECAR_URL_FILE_NAMES = ['horizon-sidecar.url', 'sidecar.url'] as const;

/**
 * Electron renderer sidecar client: attach (env / URL file) or spawn
 * `horizon-server --no-open` (release → debug → PATH → cargo run).
 */
export class ElectronHorizonSidecarService extends Disposable implements IHorizonSidecarService {

	declare readonly _serviceBrand: undefined;

	private process: ChildProcess | undefined;
	private baseUrl: string | undefined;
	/** True when using an externally started server (do not kill on stop). */
	private attachedExternal = false;
	private starting: Promise<string> | undefined;
	private disposed = false;

	constructor(
		@ILogService private readonly logService: ILogService,
		@INativeWorkbenchEnvironmentService private readonly environmentService: INativeWorkbenchEnvironmentService,
		@IWorkspaceContextService private readonly workspaceService: IWorkspaceContextService,
	) {
		super();
	}

	override dispose(): void {
		this.disposed = true;
		void this.stop();
		super.dispose();
	}

	async ensureRunning(): Promise<string> {
		if (this.disposed) {
			throw new Error('Horizon sidecar is disposed');
		}

		const configured = this.resolveConfiguredUrl();
		if (configured) {
			assertLocalhostSidecarUrl(configured);
			if (await sidecarHealthOk(configured)) {
				this.baseUrl = stripTrailingSlash(configured);
				this.attachedExternal = true;
				this.log(`attached to ${this.baseUrl}`);
				return this.baseUrl;
			}
			throw new Error(
				`HORIZON_SIDECAR_URL / URL file is set (${configured}) but GET /api/health failed. ` +
				`Start the server (./ide/scripts/run-sidecar.sh) or clear the URL to let Horizon spawn one.`
			);
		}

		if (this.baseUrl) {
			if (await sidecarHealthOk(this.baseUrl)) {
				return this.baseUrl;
			}
			if (this.attachedExternal) {
				throw new Error(
					`Attached sidecar at ${this.baseUrl} is no longer healthy. ` +
					`Restart it or clear HORIZON_SIDECAR_URL.`
				);
			}
			this.log('health check failed; restarting sidecar');
			await this.stop();
		}

		if (!this.starting) {
			this.starting = this.spawnServer().finally(() => {
				this.starting = undefined;
			});
		}
		return this.starting;
	}

	async analyse(
		repoPath: string,
		onProgress?: (progress: HorizonSidecarAnalyseProgress) => void,
	): Promise<HorizonSidecarAnalyseResult> {
		const root = this.validateAnalysePath(repoPath);
		const base = await this.ensureRunning();
		this.log(`POST /api/analyse path=${root}`);
		return runSidecarAnalyseHttp(base, root, onProgress);
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

	async stop(): Promise<void> {
		const proc = this.process;
		const wasExternal = this.attachedExternal;
		this.process = undefined;
		this.baseUrl = undefined;
		this.attachedExternal = false;
		if (wasExternal || !proc || proc.killed) {
			return;
		}
		this.log('stopping sidecar process');
		await killSidecarProcess(proc);
	}

	/**
	 * Resolve attach URL from `HORIZON_SIDECAR_URL` or `ide/.cache/horizon-sidecar.url`.
	 */
	private resolveConfiguredUrl(): string | undefined {
		const fromEnv = (env['HORIZON_SIDECAR_URL'] || '').trim();
		const fromFile = this.readUrlFile();
		const raw = fromEnv || fromFile;
		if (!raw) {
			return undefined;
		}
		try {
			const u = new URL(raw);
			return `${u.protocol}//${u.host}`;
		} catch {
			throw new Error(`Invalid HORIZON_SIDECAR_URL / URL file value: ${raw}`);
		}
	}

	private readUrlFile(): string | undefined {
		for (const candidate of this.urlFileCandidates()) {
			try {
				if (!fs.existsSync(candidate)) {
					continue;
				}
				const text = fs.readFileSync(candidate, 'utf8').trim().split(/\r?\n/)[0]?.trim();
				if (text) {
					this.log(`read sidecar URL from ${candidate}`);
					return text;
				}
			} catch {
				/* ignore */
			}
		}
		return undefined;
	}

	private urlFileCandidates(): string[] {
		const out: string[] = [];
		const roots = this.candidateRoots();
		for (const root of roots) {
			for (const name of SIDECAR_URL_FILE_NAMES) {
				// ide/.cache/<name> when root is repo or ide/
				out.push(path.join(root, 'ide', '.cache', name));
				out.push(path.join(root, '.cache', name));
			}
		}
		return out;
	}

	private candidateRoots(): string[] {
		const roots: string[] = [];
		const appRoot = this.environmentService.appRoot;
		if (appRoot) {
			// appRoot ≈ ide/code-oss → parent is ide/, grandparent is repo
			roots.push(path.resolve(appRoot));
			roots.push(path.resolve(appRoot, '..'));
			roots.push(path.resolve(appRoot, '..', '..'));
		}
		for (const folder of this.workspaceService.getWorkspace().folders) {
			const p = folder.uri.fsPath || folder.uri.path;
			if (p) {
				roots.push(path.resolve(p));
			}
		}
		try {
			roots.push(path.resolve(process.cwd()));
		} catch {
			/* ignore */
		}
		return [...new Set(roots)];
	}

	private validateAnalysePath(repoPath: string): string {
		const trimmed = (repoPath || '').trim();
		if (!trimmed) {
			throw new Error(localize('horizonSidecarEmptyPath', 'analyse path is empty'));
		}
		const root = path.resolve(trimmed);
		if (!fs.existsSync(root) || !fs.statSync(root).isDirectory()) {
			throw new Error(`analyse path is not a directory: ${root}`);
		}
		return root;
	}

	private async spawnServer(): Promise<string> {
		const { command, args, cwd, label } = this.resolveLaunch();
		this.log(`starting (${label}): ${command} ${args.join(' ')} (cwd=${cwd ?? 'default'})`);
		this.attachedExternal = false;

		const child = spawn(command, args, {
			cwd,
			env: {
				...process.env,
				CARGO_TERM_PROGRESS_WHEN: 'never',
			},
			stdio: ['ignore', 'pipe', 'pipe'],
			detached: process.platform !== 'win32',
		});
		this.process = child;

		let combined = '';
		const onChunk = (chunk: Buffer) => {
			const text = chunk.toString('utf8');
			combined += text;
			this.logService.info(`[Horizon sidecar] ${text.trimEnd()}`);
		};
		child.stdout?.on('data', onChunk);
		child.stderr?.on('data', onChunk);

		child.on('error', (err) => {
			this.log(`spawn error: ${err.message}`);
		});

		child.on('exit', (code, signal) => {
			this.log(`sidecar exited code=${code} signal=${signal}`);
			if (this.process === child) {
				this.process = undefined;
				this.baseUrl = undefined;
			}
		});

		let url: string;
		try {
			url = await this.waitForUrl(() => combined, child);
		} catch (err) {
			await killSidecarProcess(child);
			if (this.process === child) {
				this.process = undefined;
			}
			const hint = formatSpawnFailure(command, args, cwd, combined);
			const message = err instanceof Error ? err.message : String(err);
			throw new Error(`${message}. ${hint}`);
		}

		assertLocalhostSidecarUrl(url);
		try {
			await this.waitForHealth(url);
		} catch (err) {
			await killSidecarProcess(child);
			if (this.process === child) {
				this.process = undefined;
			}
			throw err;
		}

		this.baseUrl = stripTrailingSlash(url);
		this.log(`ready at ${this.baseUrl}`);
		this.writeUrlFileBestEffort(this.baseUrl);
		return this.baseUrl;
	}

	private writeUrlFileBestEffort(url: string): void {
		const appRoot = this.environmentService.appRoot;
		if (!appRoot) {
			return;
		}
		// Prefer ide/.cache when appRoot is ide/code-oss
		const ideRoot = path.resolve(appRoot, '..');
		const file = path.join(ideRoot, '.cache', 'horizon-sidecar.url');
		try {
			fs.mkdirSync(path.dirname(file), { recursive: true });
			fs.writeFileSync(file, `${url}\n`, 'utf8');
		} catch (err) {
			this.log(`could not write URL file ${file}: ${err instanceof Error ? err.message : String(err)}`);
		}
	}

	private resolveLaunch(): {
		command: string;
		args: string[];
		cwd?: string;
		label: string;
	} {
		const envBinary = (env['HORIZON_SERVER_PATH'] || '').trim();
		if (envBinary && fs.existsSync(envBinary) && isExecutableFile(envBinary)) {
			return {
				command: envBinary,
				args: ['--no-open'],
				label: 'HORIZON_SERVER_PATH',
			};
		}

		const cargoWs = this.findHorizonCargoWorkspace();
		if (cargoWs) {
			const built = findBuiltServerBinary(cargoWs);
			if (built) {
				return {
					command: built,
					args: ['--no-open'],
					cwd: cargoWs,
					label: built.includes(`${path.sep}release${path.sep}`)
						? 'target/release'
						: 'target/debug',
				};
			}
		}

		const onPath = findOnPath('horizon-server');
		if (onPath) {
			return {
				command: onPath,
				args: ['--no-open'],
				label: 'PATH',
			};
		}

		if (cargoWs) {
			return {
				command: 'cargo',
				args: ['run', '-p', 'horizon-server', '--', '--no-open'],
				cwd: cargoWs,
				label: 'cargo run',
			};
		}

		throw new Error(
			'Could not locate horizon-server. Build it (`cargo build -p horizon-server --release`), ' +
			'put it on PATH, set HORIZON_SERVER_PATH, or start via ./ide/scripts/run.sh / run-sidecar.sh.'
		);
	}

	private findHorizonCargoWorkspace(): string | undefined {
		for (const start of this.candidateRoots()) {
			let dir = path.resolve(start);
			for (let i = 0; i < 10; i++) {
				if (isHorizonWorkspace(dir)) {
					return dir;
				}
				const parent = path.dirname(dir);
				if (parent === dir) {
					break;
				}
				dir = parent;
			}
		}
		return undefined;
	}

	private waitForUrl(
		getCombined: () => string,
		child: ChildProcess,
	): Promise<string> {
		return new Promise((resolve, reject) => {
			const started = Date.now();
			const timer = setInterval(() => {
				if (this.disposed) {
					clearInterval(timer);
					reject(new Error('sidecar disposed while starting'));
					return;
				}
				if (child.exitCode !== null) {
					clearInterval(timer);
					reject(
						new Error(
							`horizon-server exited before printing a listen URL (code ${child.exitCode})`
						)
					);
					return;
				}
				const match = getCombined().match(LISTEN_URL_RE);
				if (match) {
					clearInterval(timer);
					resolve(match[0].trim());
					return;
				}
				if (Date.now() - started > HEALTH_TIMEOUT_MS) {
					clearInterval(timer);
					reject(
						new Error(
							'timed out waiting for horizon-server listen URL on stdout/stderr'
						)
					);
				}
			}, 100);
		});
	}

	private async waitForHealth(baseUrl: string): Promise<void> {
		const started = Date.now();
		while (Date.now() - started < HEALTH_TIMEOUT_MS) {
			if (this.disposed) {
				throw new Error('sidecar disposed while waiting for /api/health');
			}
			if (await sidecarHealthOk(baseUrl)) {
				return;
			}
			await delay(200);
		}
		throw new Error(
			`timed out waiting for GET /api/health at ${stripTrailingSlash(baseUrl)}`
		);
	}

	private log(message: string): void {
		this.logService.info(`[Horizon sidecar] ${message}`);
	}
}

function killSidecarProcess(proc: ChildProcess): Promise<void> {
	return new Promise((resolve) => {
		if (proc.exitCode !== null || proc.killed) {
			resolve();
			return;
		}
		const done = () => resolve();
		proc.once('exit', done);

		const pid = proc.pid;
		try {
			if (pid && process.platform !== 'win32') {
				try {
					process.kill(-pid, 'SIGTERM');
				} catch {
					proc.kill('SIGTERM');
				}
			} else {
				proc.kill('SIGTERM');
			}
		} catch {
			done();
			return;
		}

		setTimeout(() => {
			if (proc.exitCode !== null) {
				done();
				return;
			}
			try {
				if (pid && process.platform !== 'win32') {
					try {
						process.kill(-pid, 'SIGKILL');
					} catch {
						proc.kill('SIGKILL');
					}
				} else if (!proc.killed) {
					proc.kill('SIGKILL');
				}
			} catch {
				/* ignore */
			}
			done();
		}, STOP_GRACE_MS);
	});
}

function formatSpawnFailure(
	command: string,
	args: string[],
	cwd: string | undefined,
	combined: string,
): string {
	const tail = combined.trim().split(/\r?\n/).slice(-8).join(' | ');
	const parts = [
		`Launch was: ${command} ${args.join(' ')}`,
		cwd ? `cwd=${cwd}` : undefined,
		tail ? `last output: ${tail}` : 'no output captured',
	];
	return parts.filter(Boolean).join('; ');
}

function isExecutableFile(filePath: string): boolean {
	try {
		const st = fs.statSync(filePath);
		if (!st.isFile()) {
			return false;
		}
		if (process.platform === 'win32') {
			return true;
		}
		try {
			fs.accessSync(filePath, fs.constants.X_OK);
			return true;
		} catch {
			const probe = spawnSync(filePath, ['--help'], {
				encoding: 'utf8',
				timeout: 3_000,
			});
			return probe.status === 0 || probe.status === null;
		}
	} catch {
		return false;
	}
}

function isHorizonWorkspace(dir: string): boolean {
	const cargo = path.join(dir, 'Cargo.toml');
	if (!fs.existsSync(cargo)) {
		return false;
	}
	try {
		const text = fs.readFileSync(cargo, 'utf8');
		return (
			text.includes('horizon-server') ||
			fs.existsSync(path.join(dir, 'crates', 'horizon-server', 'Cargo.toml'))
		);
	} catch {
		return false;
	}
}

function findBuiltServerBinary(cargoWorkspace: string): string | undefined {
	const candidates = [
		path.join(cargoWorkspace, 'target', 'release', 'horizon-server'),
		path.join(cargoWorkspace, 'target', 'debug', 'horizon-server'),
	];
	if (process.platform === 'win32') {
		candidates.unshift(
			path.join(cargoWorkspace, 'target', 'release', 'horizon-server.exe'),
			path.join(cargoWorkspace, 'target', 'debug', 'horizon-server.exe'),
		);
	}
	for (const c of candidates) {
		try {
			if (fs.existsSync(c) && fs.statSync(c).isFile()) {
				return c;
			}
		} catch {
			/* ignore */
		}
	}
	return undefined;
}

function findOnPath(binary: string): string | undefined {
	if (!binary || binary.includes('/') || binary.includes('\\')) {
		return undefined;
	}
	const pathEnv = process.env.PATH || '';
	const sep = process.platform === 'win32' ? ';' : ':';
	const exts =
		process.platform === 'win32'
			? (process.env.PATHEXT || '.EXE;.CMD;.BAT').split(';').filter(Boolean)
			: [''];

	for (const dir of pathEnv.split(sep)) {
		if (!dir) {
			continue;
		}
		for (const ext of exts) {
			const candidate = path.join(dir, binary + ext);
			try {
				if (fs.existsSync(candidate) && fs.statSync(candidate).isFile()) {
					if (process.platform === 'win32') {
						return candidate;
					}
					try {
						fs.accessSync(candidate, fs.constants.X_OK);
						return candidate;
					} catch {
						/* not executable */
					}
				}
			} catch {
				/* ignore */
			}
		}
	}
	return undefined;
}

function delay(ms: number): Promise<void> {
	return new Promise(resolve => setTimeout(resolve, ms));
}
