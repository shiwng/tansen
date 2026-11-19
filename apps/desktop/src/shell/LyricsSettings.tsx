import { ArrowDown, ArrowUp, Check, RotateCcw } from 'lucide-react';
import { useState } from 'react';
import { Menu, MenuSeparator } from '@/components/Menu';
import { useLyricsSettings } from '@/lyrics/settings';
import { sourceById } from '@/lyrics/sources';

const row =
  'flex min-h-6 w-full items-center gap-2 rounded-[5px] px-2 py-1 text-start outline-none hover:bg-quinary focus-visible:bg-quinary';
const moveButton =
  'grid size-[18px] flex-none place-items-center rounded-[4px] text-tertiary enabled:hover:bg-quaternary enabled:hover:text-primary focus-visible:bg-quaternary disabled:opacity-30';
const checkSlot = 'inline-grid w-3.5 flex-none place-items-center text-primary';

/**
 * Lyric providers in the same glass popover as Apple's contextual menus:
 * switch each source on or off, put them in order, prefer syllable timing,
 * and the PaxSenix key. Everything persists on the first change
 * (lyrics/settings.ts).
 */
export function LyricsSettings({ anchor, onClose }: { anchor: HTMLElement; onClose: () => void }) {
  const { order, disabled, preferSyllables, paxsenixKey, setEnabled, move, setPreferSyllables, setPaxsenixKey, reset } =
    useLyricsSettings();
  const [key, setKey] = useState(paxsenixKey);

  return (
    <Menu anchor={anchor} placement="below-end" width={280} maxHeight={420} onClose={onClose} label="Lyric settings">
      <div role="group" aria-label="Lyric providers">
        {order.map((id, index) => {
          const source = sourceById(id)!;
          const on = !disabled.includes(id);
          return (
            <div
              key={id}
              role="menuitemcheckbox"
              aria-checked={on}
              aria-label={`${source.label}: ${on ? 'enabled' : 'disabled'}`}
              title={source.detail}
              tabIndex={0}
              className={row}
              onClick={(event) => {
                if ((event.target as HTMLElement).closest('button')) return;
                setEnabled(id, !on);
              }}
              onKeyDown={(event) => {
                if (event.target !== event.currentTarget) return;
                if (event.key === ' ' || event.key === 'Enter') {
                  event.preventDefault();
                  setEnabled(id, !on);
                }
              }}
            >
              <span className={checkSlot}>{on && <Check size={13} strokeWidth={2.8} />}</span>
              <span className="min-w-0 flex-1 truncate type-body text-primary">{source.label}</span>
              <span className="flex flex-none gap-0.5">
                <button
                  type="button"
                  className={moveButton}
                  aria-label={`Move ${source.label} up`}
                  disabled={index === 0}
                  onClick={() => move(id, -1)}
                >
                  <ArrowUp size={11} />
                </button>
                <button
                  type="button"
                  className={moveButton}
                  aria-label={`Move ${source.label} down`}
                  disabled={index === order.length - 1}
                  onClick={() => move(id, 1)}
                >
                  <ArrowDown size={11} />
                </button>
              </span>
            </div>
          );
        })}
      </div>

      <MenuSeparator />
      <div
        role="menuitemcheckbox"
        aria-checked={preferSyllables}
        title="Keep looking past a line-timed answer for a word-timed one."
        tabIndex={0}
        className={row}
        onClick={(event) => {
          if ((event.target as HTMLElement).closest('button, input')) return;
          setPreferSyllables(!preferSyllables);
        }}
        onKeyDown={(event) => {
          if (event.target !== event.currentTarget) return;
          if (event.key === ' ' || event.key === 'Enter') {
            event.preventDefault();
            setPreferSyllables(!preferSyllables);
          }
        }}
      >
        <span className={checkSlot}>{preferSyllables && <Check size={13} strokeWidth={2.8} />}</span>
        <span className="min-w-0 flex-1 truncate type-body text-primary">Prefer syllable timing</span>
      </div>

      <MenuSeparator />
      <label className="flex items-center gap-2 px-2 pt-0.5 pb-1 type-body text-primary">
        <span className="flex-1 truncate">PaxSenix key</span>
        <input
          className="w-[130px] rounded-[5px] border-0 bg-quinary px-2 py-0.5 type-callout text-primary outline-none focus-visible:ring-2 focus-visible:ring-key"
          type="password"
          placeholder="Spotify / Musixmatch"
          value={key}
          onChange={(event) => setKey(event.currentTarget.value)}
          onBlur={() => setPaxsenixKey(key)}
          autoComplete="off"
          aria-label="PaxSenix key"
        />
      </label>

      <MenuSeparator />
      <button
        type="button"
        role="menuitem"
        className={`${row} text-secondary`}
        onClick={reset}
      >
        <span className="inline-grid w-3.5 flex-none place-items-center">
          <RotateCcw size={12} />
        </span>
        Restore Defaults
      </button>
    </Menu>
  );
}
