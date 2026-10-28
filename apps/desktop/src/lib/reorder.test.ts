import { describe, expect, it } from 'vitest';
import { reorderMove } from './reorder';

describe('reorderMove', () => {
  it('moves down', () => {
    expect(reorderMove(['a', 'b', 'c'], 0, 2)).toEqual(['b', 'c', 'a']);
  });
  it('moves up', () => {
    expect(reorderMove(['a', 'b', 'c'], 2, 0)).toEqual(['c', 'a', 'b']);
  });
  it('adjacent swap keeps the other in place', () => {
    expect(reorderMove(['a', 'b', 'c'], 0, 1)).toEqual(['b', 'a', 'c']);
  });
  it('same index is a no-op', () => {
    expect(reorderMove(['a', 'b'], 1, 1)).toEqual(['a', 'b']);
  });
  it('out-of-range is a no-op copy', () => {
    expect(reorderMove(['a'], 0, 5)).toEqual(['a']);
    expect(reorderMove(['a'], -1, 0)).toEqual(['a']);
  });
  it('does not mutate the input', () => {
    const input = ['a', 'b', 'c'];
    reorderMove(input, 0, 2);
    expect(input).toEqual(['a', 'b', 'c']);
  });
});
