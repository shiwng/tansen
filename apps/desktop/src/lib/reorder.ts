/** Move `from` to `to` in one array copy — the playlist reorder's one rule. */
export function reorderMove<T>(list: readonly T[], from: number, to: number): T[] {
  if (from === to || from < 0 || to < 0 || from >= list.length || to >= list.length) return [...list];
  const copy = [...list];
  const [moved] = copy.splice(from, 1);
  copy.splice(to, 0, moved!);
  return copy;
}
