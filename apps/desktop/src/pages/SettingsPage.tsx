import { useEffect, useState } from "react";
import { Repeat } from "lucide-react";
import { backend } from "../backend";
import { usePlayer } from "../stores/player";
import { useLibrary } from "../stores/library";

function Section({ title, children }: { title: string; children: React.ReactNode }) {
  return (
    <section className="rounded-xl border border-[var(--border)] bg-[var(--bg-raised)] p-4">
      <h2 className="mb-3 text-sm font-semibold">{title}</h2>
      <div className="flex flex-col gap-2.5">{children}</div>
    </section>
  );
}

function Row({ label, hint, children }: { label: string; hint?: string; children: React.ReactNode }) {
  return (
    <div className="flex items-center justify-between gap-4">
      <div>
        <div className="text-sm">{label}</div>
        {hint && <div className="text-xs text-[var(--text-dim)]">{hint}</div>}
      </div>
      {children}
    </div>
  );
}

function NotYet({ children }: { children: React.ReactNode }) {
  return (
    <span className="rounded-full border border-[var(--border)] px-2 py-0.5 text-[11px] text-[var(--text-dim)]">
      {children}
    </span>
  );
}

export function SettingsPage() {
  const repeat = usePlayer((s) => s.repeat);
  const cycleRepeat = usePlayer((s) => s.cycleRepeat);
  const rescan = useLibrary((s) => s.rescan);
  const scanning = useLibrary((s) => s.scanning);
  const [settings, setSettings] = useState<[string, string][]>([]);

  useEffect(() => {
    backend.settingsGet().then(setSettings).catch(() => {});
  }, []);

  const get = (k: string) => settings.find(([key]) => key === k)?.[1];
  const put = async (k: string, v: string) => {
    setSettings((s) => [...s.filter(([key]) => key !== k), [k, v]]);
    await backend.settingsSet(k, v);
  };

  return (
    <div className="h-full overflow-y-auto px-6 py-6">
      <h1 className="mb-4 text-2xl font-bold tracking-tight">Settings</h1>
      <div className="flex max-w-2xl flex-col gap-4">
        <Section title="Playback">
          <Row label="Repeat mode" hint="Cycles off → all → one">
            <button
              onClick={cycleRepeat}
              className="flex items-center gap-1.5 rounded-lg border border-[var(--border)] px-3 py-1.5 text-sm capitalize"
            >
              <Repeat size={13} /> {repeat}
            </button>
          </Row>
          <Row label="Gapless playback" hint="Always on for local files">
            <NotYet>always on</NotYet>
          </Row>
          <Row label="Crossfade" hint="Adjustable 0–12 s (phase 6)">
            <NotYet>not yet implemented</NotYet>
          </Row>
          <Row label="Equalizer" hint="10-band DSP chain (phase 6)">
            <NotYet>not yet implemented</NotYet>
          </Row>
        </Section>

        <Section title="Library">
          <Row label="Rescan folders" hint="Re-index changed files, mark missing ones">
            <button
              onClick={rescan}
              disabled={scanning}
              className="rounded-lg border border-[var(--border)] px-3 py-1.5 text-sm disabled:opacity-50"
            >
              {scanning ? "Scanning…" : "Rescan"}
            </button>
          </Row>
        </Section>

        <Section title="Sources">
          <Row label="Streaming sources (YouTube Music, addon servers)" hint="Source adapters land in phase 4">
            <NotYet>not yet implemented</NotYet>
          </Row>
        </Section>

        <Section title="Integrations">
          <Row label="Last.fm / ListenBrainz scrobbling" hint="Phase 7">
            <NotYet>not yet implemented</NotYet>
          </Row>
          <Row label="Discord Rich Presence" hint="Phase 7">
            <NotYet>not yet implemented</NotYet>
          </Row>
          <Row label="Lyrics" hint="Provider system lands in phase 5">
            <NotYet>not yet implemented</NotYet>
          </Row>
        </Section>

        <Section title="Appearance">
          <Row label="Theme" hint="Dark is the only theme in this slice; the choice is persisted">
            <select
              value={get("theme") ?? "dark"}
              onChange={(e) => put("theme", e.target.value)}
              className="rounded-lg border border-[var(--border)] bg-[var(--bg-hover)] px-3 py-1.5 text-sm"
            >
              <option value="dark">Dark</option>
              <option value="dark">System (dark)</option>
            </select>
          </Row>
        </Section>

        <Section title="About">
          <Row label="Version" hint="Phase 3 vertical slice">
            <span className="text-sm text-[var(--text-dim)]">0.1.0</span>
          </Row>
          <Row label="Stored settings" hint="SQLite, application data directory">
            <span className="text-sm text-[var(--text-dim)]">{settings.length} keys</span>
          </Row>
        </Section>
      </div>
    </div>
  );
}
