import { useMemo, useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { FolderPlus, Loader2, RefreshCw, X } from "lucide-react";
import { TrackTable } from "../components/TrackTable";
import { useLibrary } from "../stores/library";

export function LibraryPage() {
  const { tracks, folders, loading, scanning, scanProgress, lastScan, error, addFolder, removeFolder, rescan } =
    useLibrary();
  const [query, setQuery] = useState("");

  const filtered = useMemo(() => {
    const q = query.trim().toLowerCase();
    if (!q) return tracks;
    return tracks.filter((t) =>
      [t.title, t.artist, t.album, t.album_artist].some((f) => f.toLowerCase().includes(q)),
    );
  }, [tracks, query]);

  return (
    <div className="flex h-full flex-col">
      <header className="flex items-center gap-3 px-6 pt-6 pb-4">
        <h1 className="text-2xl font-bold tracking-tight">Library</h1>
        <span className="text-sm text-[var(--text-dim)]">{tracks.length} tracks</span>
        <div className="ml-auto flex items-center gap-2">
          <input
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            placeholder="Filter…"
            className="w-56 rounded-lg border border-[var(--border)] bg-[var(--bg-hover)] px-3 py-1.5 text-sm outline-none focus:border-[var(--accent)]"
          />
          <button
            onClick={rescan}
            disabled={scanning}
            className="flex items-center gap-1.5 rounded-lg border border-[var(--border)] px-3 py-1.5 text-sm text-[var(--text-dim)] hover:text-white disabled:opacity-50"
          >
            {scanning ? <Loader2 size={14} className="animate-spin" /> : <RefreshCw size={14} />}
            Rescan
          </button>
          <button
            onClick={async () => {
              const sel = await open({ directory: true, multiple: false, title: "Choose a music folder" });
              if (typeof sel === "string") addFolder(sel);
            }}
            disabled={scanning}
            className="flex items-center gap-1.5 rounded-lg bg-[var(--accent)] px-3 py-1.5 text-sm font-medium text-white hover:opacity-90 disabled:opacity-50"
          >
            <FolderPlus size={14} />
            Import folder
          </button>
        </div>
      </header>

      {(scanning || lastScan || folders.length > 0) && (
        <div className="flex flex-wrap items-center gap-2 px-6 pb-3 text-xs">
          {folders.map((f) => (
            <span key={f.id} className="flex items-center gap-1.5 rounded-full bg-[var(--bg-hover)] px-2.5 py-1 text-[var(--text-dim)]">
              {f.path}
              <button onClick={() => removeFolder(f.id)} className="hover:text-white" title="Remove folder">
                <X size={11} />
              </button>
            </span>
          ))}
          {scanning && scanProgress && (
            <span className="flex items-center gap-1.5 text-[var(--accent)]">
              <Loader2 size={11} className="animate-spin" />
              Scanning {scanProgress.scanned}/{scanProgress.total || "…"}
            </span>
          )}
          {!scanning && lastScan && (
            <span className="text-[var(--text-dim)]">
              Last scan: +{lastScan.added} new{lastScan.errors > 0 ? `, ${lastScan.errors} failed` : ""}
            </span>
          )}
        </div>
      )}

      {error && <p className="px-6 pb-3 text-sm text-red-400">{error}</p>}

      <div className="flex-1 overflow-y-auto px-4 pb-6">
        {loading ? (
          <p className="px-3 py-8 text-center text-sm text-[var(--text-dim)]">Loading…</p>
        ) : tracks.length === 0 ? (
          <div className="flex flex-col items-center gap-3 py-24 text-center">
            <FolderPlus size={36} className="text-[var(--text-dim)]" />
            <p className="text-sm text-[var(--text-dim)]">
              No music yet. Import a folder to index your local files.
            </p>
          </div>
        ) : (
          <TrackTable tracks={filtered} emptyHint="No tracks match this filter." />
        )}
      </div>
    </div>
  );
}
