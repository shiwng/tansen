/** The radio entry point explains the event-driven related-track action. */
export function RadioPage() {
  return (
    <div className="mx-(--body-gutter) pt-11">
      <h1 className="type-header-emphasized">Radio</h1>
      <p className="mt-6 max-w-md type-body text-secondary">
        Start a YouTube Music station from the current track's menu in the player bar. Related tracks are appended to
        the existing queue and play through the normal engine.
      </p>
    </div>
  );
}
