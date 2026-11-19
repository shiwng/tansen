import { describe, expect, it } from 'vitest';
import { pickCategoryArt } from './categoryArt';
import type { DiscoveryCard } from './discovery';

const card = (overrides: Partial<DiscoveryCard>): DiscoveryCard => ({
  kind: 'artist',
  browse_id: 'VLx',
  title: 'Playlist',
  subtitle: null,
  thumbnail_url: 'https://yt3.googleusercontent.com/a=w544-h544',
  ...overrides,
});

describe('pickCategoryArt', () => {
  it('prefers an artist whose title names the category', () => {
    const cards = [
      card({ browse_id: 'VL1', title: 'Top 100 Songs' }),
      card({ browse_id: 'VL2', title: 'Bollywood Classics', thumbnail_url: 'https://yt3.googleusercontent.com/b=w544-h544' }),
      card({ browse_id: 'VL3', title: 'Bollywood Party', thumbnail_url: 'https://yt3.googleusercontent.com/c=w544-h544' }),
    ];
    expect(pickCategoryArt(cards, 'Bollywood')).toBe('https://yt3.googleusercontent.com/b=w544-h544');
  });

  it('uses the first ranked thumbnail when no title matches', () => {
    const cards = [
      card({ browse_id: 'VL1', title: 'Charts 2026', thumbnail_url: 'https://yt3.googleusercontent.com/first=w544-h544' }),
      card({ browse_id: 'VL2', title: 'Top 40', thumbnail_url: 'https://yt3.googleusercontent.com/second=w544-h544' }),
    ];
    expect(pickCategoryArt(cards, 'Jazz')).toBe('https://yt3.googleusercontent.com/first=w544-h544');
    // Same input, same output: no random or time-based choice.
    expect(pickCategoryArt(cards, 'Jazz')).toBe('https://yt3.googleusercontent.com/first=w544-h544');
  });

  it('never returns a thumbnail-less or non-artist card', () => {
    const cards = [
      card({ kind: 'playlist', browse_id: 'VL1', title: 'Bollywood Playlist' }),
      card({ browse_id: 'VL2', title: 'Bollywood Playlist', thumbnail_url: null }),
      card({ browse_id: 'VL3', title: 'Rock', thumbnail_url: 'https://yt3.googleusercontent.com/rock=w544-h544' }),
    ];
    expect(pickCategoryArt(cards, 'Bollywood')).toBe('https://yt3.googleusercontent.com/rock=w544-h544');
    expect(pickCategoryArt([], 'Bollywood')).toBeNull();
  });
});
