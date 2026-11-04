/** F5: the `stream_info_get` DTO (camelCase wire shape) and its mapping. */
export interface StreamInfoDto {
  source: string;
  codec: string | null;
  kbps: number | null;
  isLossless: boolean | null;
  belowRequest: boolean;
  isFallback: boolean;
}

/** The store-facing shape: one display line precomputed. */
export interface StreamInfoView {
  source: string;
  /** e.g. `FLAC · lossless · 900 kbps` or `AAC`. */
  facts: string;
  belowRequest: boolean;
  isFallback: boolean;
}

export function sourceLabel(source: string): string {
  switch (source) {
    case 'youtube':
      return 'YouTube Music';
    case 'addon':
      return 'Addon';
    case 'subsonic':
      return 'Subsonic';
    case 'local':
      return 'Local Library';
    default:
      return source;
  }
}

export function mapStreamInfo(dto: StreamInfoDto | null): StreamInfoView | null {
  if (!dto) return null;
  const facts: string[] = [];
  if (dto.codec) facts.push(dto.codec);
  if (dto.isLossless === true) facts.push('lossless');
  if (dto.isLossless === false) facts.push('lossy');
  if (dto.kbps && dto.kbps > 0) facts.push(`${dto.kbps} kbps`);
  return {
    source: sourceLabel(dto.source),
    facts: facts.join(' · '),
    belowRequest: dto.belowRequest,
    isFallback: dto.isFallback,
  };
}
