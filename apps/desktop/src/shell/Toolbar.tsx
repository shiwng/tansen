import { useQuery } from '@tanstack/react-query';
import { invoke } from '@tauri-apps/api/core';
import { useNavigate, useSearchParams } from 'react-router';
import { SearchField } from '@/components/music/SearchField';

interface SourceConfig {
  id: string;
  name: string;
  kind: string;
  enabled: boolean;
}

/**
 * The page toolbar: a centred search field and the scope control, as Music's
 * window has. The scopes are the real enabled catalogue sources; the local
 * library is its own scope.
 */
export function Toolbar() {
  const [params] = useSearchParams();
  const navigate = useNavigate();
  const query = params.get('q') ?? '';
  const scopeParam = params.get('scope') ?? '';
  const sources = useQuery({
    // Not `['sources']`: Settings caches `sources_get_with_health` under that
    // key, and a shared key would hand it rows without `health`.
    queryKey: ['sources', 'config'],
    queryFn: () => invoke<SourceConfig[]>('sources_get'),
    staleTime: 60_000,
    retry: false,
  });

  const scopes = [
    { value: 'library', label: 'Library' },
    ...(sources.data ?? [])
      .filter((source) => source.enabled && source.kind !== 'local')
      .map((source) => ({ value: source.kind, label: source.name })),
  ];
  const selected = scopes.some((scope) => scope.value === scopeParam) ? scopeParam : '';
  const placeholder = scopes.find((scope) => scope.value === selected)?.label ?? 'Search';

  const search = (value: string) => {
    const trimmed = value.trim();
    if (!trimmed) return navigate('/search');
    navigate(`/search?q=${encodeURIComponent(trimmed)}${selected ? `&scope=${selected}` : ''}`);
  };

  return (
    <div data-tauri-drag-region className="chrome-dim relative z-10 flex h-(--toolbar-height) flex-none select-none items-center justify-center px-6">
      <SearchField initial={query} placeholder={placeholder} onSearch={search} />
      <div className="absolute right-5 flex items-center gap-px rounded-pill bg-segmented p-[3px]" role="tablist" aria-label="Search scope">
        {scopes.map((scope) => (
          <button
            key={scope.value}
            role="tab"
            aria-selected={scope.value === selected}
            onClick={() => navigate(scope.value === selected ? '/search' : `/search?scope=${scope.value}`)}
            className={`h-[24px] rounded-pill px-3 type-callout-emphasized transition-colors ${
              scope.value === selected ? 'bg-segmented-selected text-primary' : 'text-secondary hover:text-primary'
            }`}
          >
            {scope.label}
          </button>
        ))}
      </div>
    </div>
  );
}
