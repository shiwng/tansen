import { useEffect, useState } from "react";
import { Sidebar, type Page } from "./components/Sidebar";
import { PlayerBar, useKeyboardShortcuts } from "./components/PlayerBar";
import { QueuePanel } from "./components/QueuePanel";
import { HomePage } from "./pages/HomePage";
import { LibraryPage } from "./pages/LibraryPage";
import { SearchPage } from "./pages/SearchPage";
import { SettingsPage } from "./pages/SettingsPage";
import { usePlayer } from "./stores/player";
import { useLibrary } from "./stores/library";

export default function App() {
  const [page, setPage] = useState<Page>("home");
  const playerReady = usePlayer((s) => s.ready);
  const initPlayer = usePlayer((s) => s.init);
  const initLibrary = useLibrary((s) => s.init);
  const error = usePlayer((s) => s.error);
  const setError = usePlayer((s) => s.setError);

  useEffect(() => {
    initPlayer();
    initLibrary();
  }, [initPlayer, initLibrary]);

  useKeyboardShortcuts();

  return (
    <div className="flex h-full flex-col">
      <div className="flex min-h-0 flex-1">
        <Sidebar page={page} onNavigate={setPage} />
        <main className="min-w-0 flex-1 overflow-hidden">
          {page === "home" && <HomePage />}
          {page === "search" && <SearchPage />}
          {page === "library" && <LibraryPage />}
          {page === "settings" && <SettingsPage />}
        </main>
        <QueuePanel />
      </div>

      {error && (
        <div className="flex items-center justify-between border-t border-red-900/50 bg-red-950/60 px-4 py-2 text-sm text-red-300">
          <span>{error}</span>
          <button onClick={() => setError(null)} className="text-red-400 hover:text-white">
            ✕
          </button>
        </div>
      )}

      <PlayerBar />
      {!playerReady && (
        <div className="pointer-events-none absolute inset-x-0 top-0 h-0.5 animate-pulse bg-[var(--accent)]" />
      )}
    </div>
  );
}
