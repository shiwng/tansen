import { invoke } from '@tauri-apps/api/core';

/**
 * The providers' one way out (LyricsHttp.kt): a GET that answers with the body
 * or null, never an exception. They're raced against each other, so the
 * deadline is short: a lyric that arrives after the second chorus is no use.
 *
 * On desktop every host goes through the Rust `proxy_get` command (CORS does
 * not bind it; the allowlist there mirrors the upstream web player's
 * extension origins plus the remaining provider hosts). `route: 'direct'`
 * still fetches from the page first, for hosts that allow it.
 */

const DIRECT_TIMEOUT_MS = 6000;
const PROXY_TIMEOUT_MS = 15_000;

export interface GetOptions {
  route?: 'direct' | 'extension';
  headers?: Record<string, string>;
  /** Longer deadline for authenticated routes that look things up upstream (PaxSenix). */
  patient?: boolean;
}

async function viaProxy(url: string, patient: boolean | undefined): Promise<string | null> {
  try {
    const response = await invoke<{ status: number; body: string } | null>('proxy_get', { url, patient: patient ?? false });
    if (!response || response.status < 200 || response.status >= 300) return null;
    return response.body;
  } catch {
    return null;
  }
}

export async function lyricsGet(url: string, { route = 'direct', headers, patient }: GetOptions = {}): Promise<string | null> {
  if (route === 'extension') return viaProxy(url, patient);
  try {
    const response = await fetch(url, {
      headers: { Accept: 'application/json, text/plain, */*', ...headers },
      signal: AbortSignal.timeout(patient ? PROXY_TIMEOUT_MS : DIRECT_TIMEOUT_MS),
      referrerPolicy: 'no-referrer',
    });
    return response.ok ? await response.text() : null;
  } catch (error) {
    // A CORS refusal looks like a network error from here; the Rust proxy isn't bound by it.
    if (error instanceof TypeError) return viaProxy(url, patient);
    return null;
  }
}

/** A URL with query parameters, skipping empty ones. */
export function urlWith(base: string, params: Record<string, string | number | null | undefined>): string {
  const url = new URL(base);
  for (const [key, value] of Object.entries(params)) {
    if (value !== null && value !== undefined && value !== '') url.searchParams.set(key, String(value));
  }
  return url.toString();
}

// The web player's permission machinery, as no-ops: on desktop the Rust proxy
// either can reach a host (allowlisted) or cannot, and no user grant changes
// that, so there is never an "ungranted origin" to report.
export const ungrantedOrigins = new Set<string>();
export function onUngrantedChange(_listener: () => void): () => void {
  return () => {};
}
export function onGranted(_listener: () => void): () => void {
  return () => {};
}
