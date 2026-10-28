import { useEffect, useRef, useState } from "react";
import { ListMusic, Pause, Play, Repeat, Repeat1, SkipBack, SkipForward, Volume1, Volume2, VolumeX } from "lucide-react";
import { Art } from "./Art";
import { fmtTime } from "../types";
import { usePlayer } from "../stores/player";

export function PlayerBar() {
  const p = usePlayer();
  const [dragging, setDragging] = useState(false);
  const [dragValue, setDragValue] = useState(0);
  const [showVolume, setShowVolume] = useState(false);

  const pos = dragging ? dragValue : p.position;
  const dur = p.duration || p.current?.duration_sec || 0;
  const fill = dur > 0 ? (pos / dur) * 100 : 0;

  const RepeatIcon = p.repeat === "one" ? Repeat1 : Repeat;

  return (
    <footer className="flex h-20 shrink-0 items-center gap-4 border-t border-[var(--border)] bg-[var(--bg-raised)] px-4">
      {/* Now playing */}
      <div className="flex w-64 min-w-0 items-center gap-3">
        <Art path={p.current?.art_path} className="h-12 w-12 rounded-md" />
        <div className="min-w-0">
          <div className="truncate text-sm font-medium">{p.current?.title ?? "Nothing playing"}</div>
          <div className="truncate text-xs text-[var(--text-dim)]">
            {p.current ? p.current.artist || "Unknown artist" : "Import a folder to begin"}
          </div>
        </div>
      </div>

      {/* Transport */}
      <div className="flex flex-1 flex-col items-center gap-1">
        <div className="flex items-center gap-2">
          <button
            onClick={p.previous}
            disabled={!p.current}
            className="rounded p-1.5 text-[var(--text-dim)] hover:text-white disabled:opacity-40"
            title="Previous"
          >
            <SkipBack size={17} fill="currentColor" />
          </button>
          <button
            onClick={p.toggle}
            disabled={!p.current}
            className="flex h-9 w-9 items-center justify-center rounded-full bg-white text-black hover:scale-105 transition-transform disabled:opacity-40"
            title={p.playing ? "Pause" : "Play"}
          >
            {p.playing ? <Pause size={17} fill="currentColor" /> : <Play size={17} fill="currentColor" className="ml-0.5" />}
          </button>
          <button
            onClick={p.next}
            disabled={!p.current}
            className="rounded p-1.5 text-[var(--text-dim)] hover:text-white disabled:opacity-40"
            title="Next"
          >
            <SkipForward size={17} fill="currentColor" />
          </button>
          <button
            onClick={p.cycleRepeat}
            className={`rounded p-1.5 hover:text-white ${p.repeat === "off" ? "text-[var(--text-dim)]" : "text-[var(--accent)]"}`}
            title={`Repeat: ${p.repeat}`}
          >
            <RepeatIcon size={16} />
          </button>
        </div>
        <div className="flex w-full max-w-xl items-center gap-2 text-[11px] tabular-nums text-[var(--text-dim)]">
          <span>{fmtTime(pos)}</span>
          <input
            type="range"
            className="bc-slider flex-1"
            style={{ ["--fill" as string]: `${fill}%` }}
            min={0}
            max={dur || 1}
            step={0.1}
            value={pos}
            disabled={!p.current}
            onChange={(e) => {
              setDragging(true);
              setDragValue(Number(e.target.value));
            }}
            onPointerUp={() => {
              if (dragging) {
                p.seek(dragValue);
                setDragging(false);
              }
            }}
            onKeyUp={() => {
              if (dragging) {
                p.seek(dragValue);
                setDragging(false);
              }
            }}
          />
          <span>{fmtTime(dur)}</span>
        </div>
      </div>

      {/* Right: volume + queue */}
      <div className="flex w-64 items-center justify-end gap-2">
        <button
          onClick={() => p.setVolume(p.volume > 0 ? 0 : 1)}
          onMouseEnter={() => setShowVolume(true)}
          className="text-[var(--text-dim)] hover:text-white"
          title="Mute"
        >
          {p.volume === 0 ? <VolumeX size={16} /> : p.volume < 0.5 ? <Volume1 size={16} /> : <Volume2 size={16} />}
        </button>
        <input
          type="range"
          className={`bc-slider w-24 transition-all ${showVolume ? "" : "md:w-16"}`}
          style={{ ["--fill" as string]: `${p.volume * 100}%` }}
          min={0}
          max={1}
          step={0.01}
          value={p.volume}
          onChange={(e) => p.setVolume(Number(e.target.value))}
          onMouseLeave={() => setShowVolume(false)}
          title="Volume"
        />
        <button
          onClick={() => p.setQueueOpen(!p.queueOpen)}
          className={`rounded p-1.5 hover:text-white ${p.queueOpen ? "text-[var(--accent)]" : "text-[var(--text-dim)]"}`}
          title="Queue"
        >
          <ListMusic size={17} />
        </button>
      </div>
    </footer>
  );
}

/** Global keyboard shortcuts: Space = play/pause, arrows = seek/volume. */
export function useKeyboardShortcuts() {
  const p = usePlayer();
  const ref = useRef(p);
  ref.current = p;
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      const t = e.target as HTMLElement;
      if (t.tagName === "INPUT" || t.tagName === "TEXTAREA" || t.isContentEditable) return;
      const s = ref.current;
      switch (e.code) {
        case "Space":
          e.preventDefault();
          s.toggle();
          break;
        case "ArrowRight":
          if (s.current) s.seek(Math.min(s.position + 5, s.duration || s.position + 5));
          break;
        case "ArrowLeft":
          if (s.current) s.seek(Math.max(0, s.position - 5));
          break;
        case "ArrowUp":
          e.preventDefault();
          s.setVolume(Math.min(1, s.volume + 0.05));
          break;
        case "ArrowDown":
          e.preventDefault();
          s.setVolume(Math.max(0, s.volume - 0.05));
          break;
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, []);
}
