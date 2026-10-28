import { useMemo, useState } from "react";
import { Search } from "lucide-react";
import { TrackTable } from "../components/TrackTable";
import { useLibrary } from "../stores/library";

/** Search across the local library. Source-catalogue search is phase 4. */
export function SearchPage() {
  const { tracks } = useLibrary();
  const [query, setQuery] = useState("");

  const results = useMemo(() => {
    const q = query.trim().toLowerCase();
    if (!q) return [];
    return tracks.filter((t) =>
      [t.title, t.artist, t.album, t.album_artist, t.genre].some((f) => f.toLowerCase().includes(q)),
    );
  }, [tracks, query]);

  return (
    <div className="flex h-full flex-col">
      <header className="px-6 pt-6 pb-4">
        <div className="flex items-center gap-3 rounded-xl border border-[var(--border)] bg-[var(--bg-hover)] px-4 py-2.5">
          <Search size={16} className="text-[var(--text-dim)]" />
          <input
            autoFocus
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            placeholder="Search your library — title, artist, album…"
            className="flex-1 bg-transparent text-sm outline-none placeholder:text-[var(--text-dim)]"
          />
        </div>
        <p className="mt-2 text-xs text-[var(--text-dim)]">
          Searching {tracks.length} local tracks. Online catalogue search arrives with source adapters (phase 4).
        </p>
      </header>
      <div className="flex-1 overflow-y-auto px-4 pb-6">
        {query.trim() ? (
          <TrackTable tracks={results} emptyHint={`Nothing matches “${query}”.`} />
        ) : (
          <p className="px-3 py-8 text-center text-sm text-[var(--text-dim)]">Type to search.</p>
        )}
      </div>
    </div>
  );
}
