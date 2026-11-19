/**
 * Radio Browser discovery through the Rust side (no webview network: the
 * app's CSP allows only ipc, and the directory needs a User-Agent). All
 * queries are cached and paged by the backend; this module only shapes them.
 */
import { invoke } from '@tauri-apps/api/core';
import { convertFileSrc } from '@tauri-apps/api/core';

export interface RadioStation {
  id: string;
  name: string;
  url: string;
  homepage: string;
  favicon: string;
  tags: string;
  country: string;
  countryCode: string;
  language: string;
  codec: string;
  bitrate: number;
  votes: number;
  clickCount: number;
  lastCheckOk: number;
  hls: number;
}

export interface RadioFacet {
  name: string;
  code: string;
  count: number;
}

export interface RadioFacets {
  countries: RadioFacet[];
  languages: RadioFacet[];
  genres: RadioFacet[];
}

export interface RadioQuery {
  search?: string;
  countryCode?: string;
  language?: string;
  tag?: string;
  order?: 'clickcount' | 'votes' | 'name';
  offset?: number;
}

export const RADIO_PAGE_SIZE = 60;

export function radioStations(query: RadioQuery): Promise<RadioStation[]> {
  return invoke<RadioStation[]>('radio_stations', { query });
}

export function radioFacets(): Promise<RadioFacets> {
  return invoke<RadioFacets>('radio_facets');
}

/** Best-effort popularity signal; never awaited by playback. */
export function radioClick(stationId: string): void {
  void invoke('radio_click', { stationId }).catch(() => {});
}

/** Caches a station favicon (arbitrary https host) and returns an art:// URL. */
export async function radioArtwork(url: string | null): Promise<string | null> {
  if (!url?.startsWith('https://')) return null;
  try {
    return convertFileSrc(await invoke<string>('radio_artwork', { url }), 'art');
  } catch {
    return null;
  }
}

/** "Country • tag • tag", exactly what the station record says. */
export function stationDetail(station: RadioStation): string {
  const tags = station.tags
    .split(',')
    .map((t) => t.trim())
    .filter(Boolean)
    .slice(0, 3);
  return [station.country, ...tags].filter(Boolean).join(' • ');
}

/** The bitrate/codec line, honest about missing values. */
export function stationQuality(station: RadioStation): string {
  const codec = station.codec || 'unknown codec';
  return station.bitrate > 0 ? `${codec} · ${station.bitrate} kbps` : codec;
}

/** A custom stream URL the user typed: public http(s), no playlist files. */
export function validateStreamUrl(raw: string): string | null {
  let url: URL;
  try {
    url = new URL(raw.trim());
  } catch {
    return 'Enter a full http:// or https:// stream URL.';
  }
  if (url.protocol !== 'http:' && url.protocol !== 'https:') return 'Only http and https streams are supported.';
  if (/\.m3u8($|\?)/i.test(url.pathname + url.search)) return 'HLS playlists are not supported by the decoder.';
  return null;
}
