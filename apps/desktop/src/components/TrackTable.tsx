import { useMemo, useState } from "react";
import { ListPlus, Play } from "lucide-react";
import { Art } from "./Art";
import { fmtTime, type Track } from "../types";
import { usePlayer } from "../stores/player";

type SortKey = "title" | "artist" | "album" | "duration_sec" | "codec";

/** Sortable, searchable track table. Double-click plays the visible list. */
export function TrackTable({ tracks, emptyHint }: { tracks: Track[]; emptyHint?: string }) {
  const [sort, setSort] = useState<{ key: SortKey; dir: 1 | -1 }>({ key: "title", dir: 1 });
  const playTracks = usePlayer((s) => s.playTracks);
  const addToQueue = usePlayer((s) => s.addToQueue);

  const sorted = useMemo(() => {
    const copy = [...tracks];
    copy.sort((a, b) => {
      const av = a[sort.key] ?? "";
      const bv = b[sort.key] ?? "";
      if (typeof av === "number" && typeof bv === "number") return (av - bv) * sort.dir;
      return String(av).localeCompare(String(bv), undefined, { sensitivity: "base" }) * sort.dir;
    });
    return copy;
  }, [tracks, sort]);

  const header = (key: SortKey, label: string, className?: string) => (
    <th
      className={`cursor-pointer px-3 py-2 text-left text-xs font-medium text-[var(--text-dim)] hover:text-[var(--text)] ${className ?? ""}`}
      onClick={() => setSort((s) => (s.key === key ? { key, dir: (s.dir * -1) as 1 | -1 } : { key, dir: 1 }))}
    >
      {label}
      {sort.key === key ? (sort.dir === 1 ? " ↑" : " ↓") : ""}
    </th>
  );

  if (tracks.length === 0) {
    return <p className="px-3 py-8 text-center text-sm text-[var(--text-dim)]">{emptyHint ?? "No tracks."}</p>;
  }

  return (
    <table className="w-full border-collapse text-sm">
      <thead>
        <tr className="border-b border-[var(--border)]">
          <th className="w-8" />
          {header("title", "Title")}
          {header("artist", "Artist")}
          {header("album", "Album")}
          {header("duration_sec", "Time", "w-16")}
          {header("codec", "Format", "w-20")}
          <th className="w-10" />
        </tr>
      </thead>
      <tbody>
        {sorted.map((t) => (
          <tr
            key={t.id}
            onDoubleClick={() => playTracks(sorted.map((x) => x.id), sorted.indexOf(t))}
            className="group cursor-default border-b border-[var(--border)]/40 hover:bg-[var(--bg-hover)]"
          >
            <td className="px-2 py-1.5">
              <div className="relative">
                <Art path={t.art_path} className="h-9 w-9 rounded" />
                <button
                  title="Play"
                  onClick={() => playTracks(sorted.map((x) => x.id), sorted.indexOf(t))}
                  className="absolute inset-0 hidden items-center justify-center rounded bg-black/60 text-white group-hover:flex"
                >
                  <Play size={14} fill="currentColor" />
                </button>
              </div>
            </td>
            <td className="max-w-64 truncate px-3 py-1.5 font-medium">{t.title}</td>
            <td className="max-w-48 truncate px-3 py-1.5 text-[var(--text-dim)]">{t.artist || "—"}</td>
            <td className="max-w-48 truncate px-3 py-1.5 text-[var(--text-dim)]">{t.album || "—"}</td>
            <td className="px-3 py-1.5 tabular-nums text-[var(--text-dim)]">{fmtTime(t.duration_sec)}</td>
            <td className="px-3 py-1.5 text-[var(--text-dim)]">{t.codec || "—"}</td>
            <td className="px-2 py-1.5">
              <button
                title="Add to queue"
                onClick={() => addToQueue([t.id])}
                className="hidden text-[var(--text-dim)] hover:text-white group-hover:block"
              >
                <ListPlus size={15} />
              </button>
            </td>
          </tr>
        ))}
      </tbody>
    </table>
  );
}
