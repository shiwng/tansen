import { Home, Library, Search, Settings } from "lucide-react";

export type Page = "home" | "search" | "library" | "settings";

const NAV: { id: Page; label: string; icon: typeof Home }[] = [
  { id: "home", label: "Home", icon: Home },
  { id: "search", label: "Search", icon: Search },
  { id: "library", label: "Library", icon: Library },
  { id: "settings", label: "Settings", icon: Settings },
];

export function Sidebar({ page, onNavigate }: { page: Page; onNavigate: (p: Page) => void }) {
  return (
    <nav className="flex w-52 shrink-0 flex-col gap-1 border-r border-[var(--border)] bg-[var(--bg-raised)] p-3">
      <div className="mb-4 flex items-center gap-2 px-2 pt-1">
        <div className="flex h-7 w-7 items-center justify-center rounded-lg bg-[var(--accent)] text-sm font-bold text-white">
          B
        </div>
        <span className="text-sm font-semibold tracking-tight">BitChord</span>
        <span className="text-[10px] text-[var(--text-dim)]">desktop</span>
      </div>
      {NAV.map(({ id, label, icon: Icon }) => (
        <button
          key={id}
          onClick={() => onNavigate(id)}
          className={`flex items-center gap-3 rounded-lg px-3 py-2 text-sm transition-colors ${
            page === id
              ? "bg-[var(--accent-soft)] text-white"
              : "text-[var(--text-dim)] hover:bg-[var(--bg-hover)] hover:text-[var(--text)]"
          }`}
        >
          <Icon size={17} strokeWidth={1.8} />
          {label}
        </button>
      ))}
    </nav>
  );
}
