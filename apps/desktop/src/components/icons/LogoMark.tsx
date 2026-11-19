/**
 * The Tansen mark: three rounded sound bars, drawn with `currentColor` so it
 * follows the label like the wordmark does. Original artwork for this
 * project.
 */
const MARK = `<svg width="100%" height="100%" viewBox="0 0 24 24" fill="none" xmlns="http://www.w3.org/2000/svg">
  <rect x="3.5" y="11" width="4.4" height="9" rx="2.2" fill="currentColor"/>
  <rect x="9.8" y="4" width="4.4" height="16" rx="2.2" fill="currentColor"/>
  <rect x="16.1" y="8" width="4.4" height="12" rx="2.2" fill="currentColor"/>
</svg>
`;

export function LogoMark({ size = 20, className }: { size?: number; className?: string }) {
  return (
    <span
      aria-hidden="true"
      className={className}
      style={{ display: 'inline-flex', width: size, height: size }}
      dangerouslySetInnerHTML={{ __html: MARK }}
    />
  );
}
