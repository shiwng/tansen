import { useNavigate } from 'react-router';
import { LogoMark } from '@/components/icons/LogoMark';

/**
 * The local-library empty state: a centred mark, prominent heading, one line
 * of context and one compact action. "Browse Tansen" opens the Search page,
 * whose browse categories are Tansen's discovery surface.
 *
 * `standalone` fills the page below the toolbar; otherwise the caller places
 * it under its own section header.
 */
export function EmptyLibrary({ standalone = false }: { standalone?: boolean }) {
  const navigate = useNavigate();
  return (
    <div
      className={`flex flex-col items-center justify-center px-6 text-center ${
        standalone
          ? 'min-h-[calc(100vh-var(--toolbar-height)-var(--player-pill-height)-var(--player-bottom-offset)-40px)]'
          : 'min-h-[calc(100vh-var(--toolbar-height)-203px)] pb-10'
      }`}
    >
      <LogoMark size={96} className="text-secondary opacity-70" />
      <h2 className="mt-7 type-[700_28px/34px] text-secondary">Add Music to Your Library</h2>
      <p className="mt-2.5 type-body text-secondary">Explore music and add your favourites to your Tansen library.</p>
      <button
        onClick={() => navigate('/search')}
        className="mt-6 inline-flex h-[26px] items-center rounded-[6px] bg-quinary px-3.5 type-body-emphasized text-primary hover:bg-quaternary focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-key"
      >
        Browse Tansen
      </button>
    </div>
  );
}
