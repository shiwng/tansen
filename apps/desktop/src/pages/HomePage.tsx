import { useMemo } from "react";
import { Clock3 } from "lucide-react";
import { TrackTable } from "../components/TrackTable";
import { useLibrary } from "../stores/library";

/** Home: recently added tracks. Catalogue discovery arrives in phase 4. */
export function HomePage() {
  const { tracks } = useLibrary();
  const recent = useMemo(
    () => [...tracks].sort((a, b) => b.id - a.id).slice(0, 15),
    [tracks],
  );

  return (
    <div className="flex h-full flex-col">
      <header className="px-6 pt-6 pb-4">
        <h1 className="text-2xl font-bold tracking-tight">Home</h1>
        <p className="mt-1 text-sm text-[var(--text-dim)]">
          Local playback is live. Streaming catalogue discovery is planned for phase 4.
        </p>
      </header>
      <div className="flex-1 overflow-y-auto px-4 pb-6">
        {tracks.length === 0 ? (
          <p className="px-3 py-8 text-center text-sm text-[var(--text-dim)]">
            Import a folder from the Library page to get started.
          </p>
        ) : (
          <>
            <h2 className="flex items-center gap-2 px-3 pb-2 text-sm font-semibold text-[var(--text-dim)]">
              <Clock3 size={14} /> Recently added
            </h2>
            <TrackTable tracks={recent} />
          </>
        )}
      </div>
    </div>
  );
}
