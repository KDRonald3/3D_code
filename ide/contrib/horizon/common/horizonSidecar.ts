/*---------------------------------------------------------------------------------------------
 *  Copyright (c) Horizon contributors. All rights reserved.
 *--------------------------------------------------------------------------------------------*/

import { createDecorator } from '../../../../platform/instantiation/common/instantiation.js';

/**
 * Sidecar client types + service contract for `horizon-server` (loopback HTTP).
 *
 * Desktop registers a spawn/attach implementation from `electron-browser/`.
 * Browser registers an attach-only fallback (env / URL file via HTTP).
 */

export type HorizonSidecarAnalyseStatus = 'idle' | 'running' | 'done' | 'failed' | 'error';

export interface HorizonSidecarAnalyseProgress {
	readonly status: HorizonSidecarAnalyseStatus;
	readonly path?: string;
	readonly elapsed_ms?: number;
	readonly error?: string;
}

export interface HorizonSidecarAnalyseResult {
	readonly map: unknown;
	readonly path: string;
	readonly elapsed_ms?: number;
}

export interface HorizonSidecarSourceQuery {
	readonly path: string;
	readonly byteStart: number;
	readonly byteEnd: number;
	readonly expectedHash: string;
}

export type HorizonSidecarSourceResult =
	| { readonly ok: true; readonly tokens: unknown }
	| { readonly ok: false; readonly error: string; readonly errorKind: string };

export const IHorizonSidecarService = createDecorator<IHorizonSidecarService>('horizonSidecarService');

/**
 * Loopback `horizon-server` client: health, analyse+poll+map, optional source slices.
 * Implementations must refuse non-localhost base URLs.
 */
export interface IHorizonSidecarService {
	readonly _serviceBrand: undefined;

	/** Attach or spawn; returns loopback base URL (`http://127.0.0.1:PORT`). */
	ensureRunning(): Promise<string>;

	/**
	 * POST `/api/analyse`, poll until terminal, then GET `/api/map`.
	 * `onProgress` fires on each poll while status is `running`.
	 */
	analyse(
		repoPath: string,
		onProgress?: (progress: HorizonSidecarAnalyseProgress) => void,
	): Promise<HorizonSidecarAnalyseResult>;

	/** GET `/api/map` when a map is already loaded; `undefined` on 404. */
	getMap(): Promise<unknown | undefined>;

	/** GET `/api/source` for webview inspector token preview. */
	getSource(query: HorizonSidecarSourceQuery): Promise<HorizonSidecarSourceResult>;
}

const LOCAL_HOST_RE = /^https?:\/\/(127\.0\.0\.1|localhost|\[::1\])(:\d+)?\/?$/i;

export function assertLocalhostSidecarUrl(url: string): string {
	let origin: string;
	try {
		const u = new URL(url);
		origin = `${u.protocol}//${u.host}`;
	} catch {
		throw new Error(`invalid sidecar URL: ${url}`);
	}
	if (!LOCAL_HOST_RE.test(origin) && !LOCAL_HOST_RE.test(`${origin}/`)) {
		throw new Error(
			`refusing non-localhost sidecar URL: ${origin} (Horizon only talks to 127.0.0.1 / localhost / ::1)`
		);
	}
	if (!origin.startsWith('http://') && !origin.startsWith('https://')) {
		throw new Error(`refusing non-http sidecar URL: ${origin}`);
	}
	return stripTrailingSlash(origin);
}

export function stripTrailingSlash(url: string): string {
	return url.replace(/\/?$/, '');
}

export function joinSidecarUrl(base: string, route: string): string {
	const b = stripTrailingSlash(base);
	return `${b}${route.startsWith('/') ? route : `/${route}`}`;
}

export async function fetchSidecarJson(
	url: string,
	init?: RequestInit,
): Promise<{ status: number; body: any }> {
	assertLocalhostSidecarUrl(url);
	const res = await fetch(url, init);
	let body: any = undefined;
	const text = await res.text();
	if (text) {
		try {
			body = JSON.parse(text);
		} catch {
			body = { error: text };
		}
	}
	return { status: res.status, body };
}

export async function sidecarHealthOk(baseUrl: string, timeoutMs = 2_000): Promise<boolean> {
	try {
		const origin = assertLocalhostSidecarUrl(baseUrl);
		const controller = new AbortController();
		const timer = setTimeout(() => controller.abort(), timeoutMs);
		try {
			const res = await fetch(joinSidecarUrl(origin, '/api/health'), {
				headers: { Accept: 'application/json' },
				signal: controller.signal,
			});
			if (!res.ok) {
				return false;
			}
			const body = (await res.json()) as { ok?: boolean };
			return body.ok === true;
		} finally {
			clearTimeout(timer);
		}
	} catch {
		return false;
	}
}

export async function pollSidecarAnalyse(
	base: string,
	onProgress?: (progress: HorizonSidecarAnalyseProgress) => void,
	pollMs = 400,
): Promise<HorizonSidecarAnalyseProgress> {
	const origin = assertLocalhostSidecarUrl(base);
	for (; ;) {
		const res = await fetchSidecarJson(joinSidecarUrl(origin, '/api/analyse'), {
			headers: { Accept: 'application/json' },
		});
		const body = res.body || {};
		const statusRaw = String(body.status || 'error');
		const status: HorizonSidecarAnalyseStatus =
			statusRaw === 'idle' || statusRaw === 'running' || statusRaw === 'done'
				|| statusRaw === 'failed' || statusRaw === 'error'
				? statusRaw
				: 'error';
		const progress: HorizonSidecarAnalyseProgress = {
			status,
			path: typeof body.path === 'string' ? body.path : undefined,
			elapsed_ms: typeof body.elapsed_ms === 'number' ? body.elapsed_ms : undefined,
			error: typeof body.error === 'string' ? body.error : undefined,
		};
		onProgress?.(progress);
		if (progress.status !== 'running') {
			return progress;
		}
		await new Promise(resolve => setTimeout(resolve, pollMs));
	}
}

/**
 * POST analyse + poll + GET map. Shared by browser attach and electron spawn clients.
 */
export async function runSidecarAnalyseHttp(
	baseUrl: string,
	repoPath: string,
	onProgress?: (progress: HorizonSidecarAnalyseProgress) => void,
): Promise<HorizonSidecarAnalyseResult> {
	const base = assertLocalhostSidecarUrl(baseUrl);
	const post = await fetchSidecarJson(joinSidecarUrl(base, '/api/analyse'), {
		method: 'POST',
		headers: { 'Content-Type': 'application/json', Accept: 'application/json' },
		body: JSON.stringify({ path: repoPath }),
	});

	if (post.status === 409) {
		// Attach to in-flight job.
	} else if (post.status !== 202 && post.status !== 200) {
		const err =
			typeof post.body?.error === 'string'
				? post.body.error
				: `analyse rejected (HTTP ${post.status})`;
		throw new Error(err);
	}

	const terminal = await pollSidecarAnalyse(base, onProgress);
	if (terminal.status === 'failed' || terminal.status === 'error') {
		throw new Error(terminal.error || 'analysis failed');
	}
	if (terminal.status !== 'done') {
		throw new Error(`unexpected analyse status: ${terminal.status}`);
	}

	const mapRes = await fetchSidecarJson(joinSidecarUrl(base, '/api/map'), {
		headers: { Accept: 'application/json' },
	});
	if (mapRes.status !== 200) {
		throw new Error(
			typeof mapRes.body?.error === 'string'
				? mapRes.body.error
				: 'map not available after analyse'
		);
	}
	return {
		map: mapRes.body,
		path: terminal.path || repoPath,
		elapsed_ms: terminal.elapsed_ms,
	};
}

export async function getSidecarSourceHttp(
	baseUrl: string,
	query: HorizonSidecarSourceQuery,
): Promise<HorizonSidecarSourceResult> {
	const base = assertLocalhostSidecarUrl(baseUrl);
	const params = new URLSearchParams({
		path: query.path,
		byte_start: String(query.byteStart),
		byte_end: String(query.byteEnd),
		expected_hash: query.expectedHash || '',
	});
	const res = await fetchSidecarJson(joinSidecarUrl(base, `/api/source?${params}`), {
		headers: { Accept: 'application/json' },
	});
	if (res.status === 200 && res.body && Array.isArray(res.body.tokens)) {
		return { ok: true, tokens: res.body.tokens };
	}
	const errorKind =
		typeof res.body?.error === 'string' ? res.body.error : 'error';
	const message =
		typeof res.body?.message === 'string'
			? res.body.message
			: typeof res.body?.error === 'string'
				? res.body.error
				: `source request failed (${res.status})`;
	return { ok: false, error: message, errorKind };
}
