import { Menu } from 'lucide-react';
import { LogoMark } from '@/components/icons/LogoMark';
import { useUi } from '@/state/ui';

/** The <484px top bar: drawer toggle, wordmark. (Upstream's, minus the extension chip.) */
export function MobileTopBar() {
  const setDrawerOpen = useUi((s) => s.setDrawerOpen);
  return (
    <header className="absolute inset-x-0 top-0 z-(--z-web-chrome) flex h-(--mobile-top-bar-height) items-center gap-1 glass-material max-md:px-2 max-sm:flex sm:hidden">
      <button className="grid size-9 place-items-center rounded-nav text-primary hover:bg-quinary" aria-label="Open menu" onClick={() => setDrawerOpen(true)}>
        <Menu size={20} />
      </button>
      <span className="flex items-center gap-2">
        <LogoMark size={18} className="text-primary" />
        <span className="type-[700_16px/20px]">Tansen</span>
      </span>
    </header>
  );
}
