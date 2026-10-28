import { Trash2, X } from "lucide-react";
import { Art } from "./Art";
import { fmtTime } from "../types";
import { usePlayer } from "../stores/player";

export function QueuePanel() {
  const { queue, queueIndex, queueOpen, setQueueOpen, jumpTo, removeFromQueue, clearQueue } = usePlayer();
  if (!queueOpen) return null;

  return (
    <aside className="flex w-80 shrink-0 flex-col border-l border-[var(--border)] bg-[var(--bg-raised)]">
      <div className="flex items-center justify-between border-b border-[var(--border)] px-4 py-3">
        <h2 className="text-sm font-semibold">Queue</h2>
        <div className="flex items-center gap-1">
          {queue.length > 0 && (
            <button onClick={clearQueue} className="rounded p-1.5 text-[var(--text-dim)] hover:text-white" title="Clear queue">
              <Trash2 size={15} />
            </button>
          )}
          <button onClick={() => setQueueOpen(false)} className="rounded p-1.5 text-[var(--text-dim)] hover:text-white" title="Close">
            <X size={16} />
          </button>
        </div>
      </div>
      <div className="flex-1 overflow-y-auto p-2">
        {queue.length === 0 ? (
          <p className="px-3 py-8 text-center text-sm text-[var(--text-dim)]">Queue is empty.</p>
        ) : (
          queue.map((t, i) => (
            <div
              key={`${t.id}-${i}`}
              onDoubleClick={() => jumpTo(i)}
              className={`group flex items-center gap-2 rounded-lg px-2 py-1.5 ${
                i === queueIndex ? "bg-[var(--accent-soft)]" : "hover:bg-[var(--bg-hover)]"
              }`}
            >
              <Art path={t.art_path} className="h-8 w-8 rounded" />
              <div className="min-w-0 flex-1">
                <div className="truncate text-xs font-medium">{t.title}</div>
                <div className="truncate text-[11px] text-[var(--text-dim)]">{t.artist || "Unknown artist"}</div>
              </div>
              <span className="text-[11px] tabular-nums text-[var(--text-dim)]">{fmtTime(t.duration_sec)}</span>
              <button
                onClick={() => removeFromQueue(i)}
                className="hidden text-[var(--text-dim)] hover:text-white group-hover:block"
                title="Remove"
              >
                <X size={13} />
              </button>
            </div>
          ))
        )}
      </div>
    </aside>
  );
}
