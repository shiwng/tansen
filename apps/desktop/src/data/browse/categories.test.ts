import { describe, expect, it } from 'vitest';
import { BROWSE_CATEGORIES, findBrowseCategory } from './categories';

const REQUIRED = [
  'Bollywood',
  'Punjabi',
  'Tamil',
  'Pop',
  'Telugu',
  'Charts',
  'Rock',
  'Metal',
  'Hip-Hop',
  'Dance',
  'Love',
  'Fitness',
  'Sleep',
  'Focus',
  'Jazz',
  'Live Music',
  'K-Pop',
  'Indie',
  'Best of 2026',
];

describe('Browse Categories data', () => {
  it('covers every requested category with a unique slug and query', () => {
    const labels = BROWSE_CATEGORIES.map((category) => category.label);
    expect(labels).toEqual(expect.arrayContaining(REQUIRED));
    expect(new Set(BROWSE_CATEGORIES.map((category) => category.id)).size).toBe(BROWSE_CATEGORIES.length);
    expect(new Set(labels).size).toBe(labels.length);
    for (const category of BROWSE_CATEGORIES) {
      expect(category.id).toMatch(/^[a-z0-9-]+$/);
      expect(category.query.trim().length).toBeGreaterThan(0);
      expect(category.description.trim().length).toBeGreaterThan(24);
      expect(category.tint[0]).toMatch(/^#[0-9a-f]{6}$/i);
      expect(category.tint[1]).toMatch(/^#[0-9a-f]{6}$/i);
    }
  });

  it('resolves route slugs (encoded or plain) and rejects unknown ids', () => {
    expect(findBrowseCategory('bollywood')?.label).toBe('Bollywood');
    expect(findBrowseCategory('best-of-2026')?.label).toBe('Best of 2026');
    expect(findBrowseCategory(encodeURIComponent('k-pop'))?.label).toBe('K-Pop');
    expect(findBrowseCategory('not-a-category')).toBeUndefined();
    expect(findBrowseCategory(undefined)).toBeUndefined();
  });
});
