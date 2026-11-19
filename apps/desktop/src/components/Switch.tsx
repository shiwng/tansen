import { Switch as GlisseSwitch } from 'glisse';

/**
 * The Settings toggle: glisse's liquid-glass switch (spring-driven thumb with
 * a press refraction lens, overdamped so it never bounces). The adapter keeps
 * the Settings call sites unchanged; `.settings-switch` in global.css points
 * the on-fill at the app accent instead of glisse's iOS green.
 */
export function Switch({ checked, onChange, label }: { checked: boolean; onChange: (checked: boolean) => void; label: string }) {
  return <GlisseSwitch className="settings-switch" checked={checked} onCheckedChange={onChange} aria-label={label} />;
}
