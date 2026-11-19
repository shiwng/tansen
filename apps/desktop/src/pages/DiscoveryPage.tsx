import { useQuery } from '@tanstack/react-query';
import { useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { SearchField } from '@/components/music/SearchField';
import { TrackList } from '@/components/music/TrackList';
import { sourceFor } from '@/lib/rows';
import type { Track } from '@/data/music/types';

/**
 * Discovery: typeahead rows from the sidecar (unauthenticated) plus the
 * browse cards it returns. Reuses the search page's remote-row mapping and
 * TrackList; no redesign. Remote album/artist/playlist pages do not exist —
 * the cards render as plain chips (their commands exist for later wiring).
 */

interface SuggestTrack {
  key: { source: string; id: string };
  title: string;
  artist: string;
  album: string | null;
  duration_sec: number | null;
  thumbnail_url: string | null;
}

interface SuggestCard {
  kind: string;
  browse_id: string;
  title: string;
  subtitle: string | null;
  thumbnail_url: string | null;
}

interface SuggestResponse {
  tracks: SuggestTrack[];
  cards: SuggestCard[];
}

function suggestToTracks(rows: SuggestTrack[]): Track[] {
  return rows.map((r) => ({
    videoId: r.key.id,
    title: r.title,
    artists: [{ name: r.artist || 'Unknown Artist', id: null }],
    album: r.album ? { id: null, name: r.album } : null,
    durationSec: r.duration_sec && r.duration_sec > 0 ? r.duration_sec : null,
    explicit: false,
    thumbnail: r.thumbnail_url,
    index: null,
    plays: null,
    sourceKind: r.key.source,
    artworkUrl: r.thumbnail_url,
  }));
}

export function DiscoveryPage() {
  const [query, setQuery] = useState('');
  const suggest = useQuery({
    queryKey: ['discover', 'suggest', query],
    queryFn: () => invoke<SuggestResponse>('discovery_suggest', { query }),
    enabled: query.trim().length > 0,
    staleTime: 5 * 60_000,
    retry: false,
  });

  const rows = suggestToTracks(suggest.data?.tracks ?? []);
  const source = sourceFor('search', query || 'discover', 'remote');

  return (
    <div>
      <div className="mx-(--body-gutter) pt-11 pb-5">
        <SearchField
          initial={query}
          onSearch={(v) => setQuery(v)}
        />
      </div>
      {suggest.isPending && <p className="mx-(--body-gutter) type-body text-secondary">Suggestions…</p>}
      {suggest.isError && (
        <p className="mx-(--body-gutter) type-body text-secondary">
          Discovery unavailable: {String(suggest.error)}. Enable catalogue sources in Settings → Sources and try again.
        </p>
      )}
      {suggest.data && rows.length === 0 && suggest.data.cards.length === 0 && (
        <p className="mx-(--body-gutter) type-body text-secondary">Nothing matches “{query}”.</p>
      )}
      {rows.length > 0 && <TrackList tracks={rows} variant="playlist" source={source} />}
      {suggest.data && suggest.data.cards.length > 0 && (
        <div className="mx-(--body-gutter) mt-6 flex flex-wrap gap-2 pb-10">
          {suggest.data.cards.map((card) => (
            <span
              key={card.browse_id}
              className="rounded-[10px] bg-quinary px-3 py-2 type-callout text-secondary"
            >
              {card.kind}: {card.title}
              {card.subtitle ? ` — ${card.subtitle}` : ''}
            </span>
          ))}
        </div>
      )}
    </div>
  );
}
