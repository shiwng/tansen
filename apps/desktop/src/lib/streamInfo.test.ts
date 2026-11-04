import { describe, expect, it } from 'vitest';
import { mapStreamInfo } from './streamInfo';

describe('mapStreamInfo', () => {
  it('null dto maps to null (entry not resolved yet)', () => {
    expect(mapStreamInfo(null)).toBeNull();
  });

  it('maps a lossless remote answer with facts in order', () => {
    const v = mapStreamInfo({
      source: 'youtube',
      codec: 'FLAC',
      kbps: 900,
      isLossless: true,
      belowRequest: false,
      isFallback: false,
    })!;
    expect(v.source).toBe('YouTube Music');
    expect(v.facts).toBe('FLAC · lossless · 900 kbps');
    expect(v.belowRequest).toBe(false);
    expect(v.isFallback).toBe(false);
  });

  it('maps a lossy answer and flags below-request (fell back / below quality)', () => {
    const v = mapStreamInfo({
      source: 'youtube',
      codec: 'AAC',
      kbps: 131,
      isLossless: false,
      belowRequest: true,
      isFallback: false,
    })!;
    expect(v.facts).toBe('AAC · lossy · 131 kbps');
    expect(v.belowRequest).toBe(true);
  });

  it('flags a fallback source (prefer-local answered for a remote key)', () => {
    const v = mapStreamInfo({
      source: 'local',
      codec: 'FLAC',
      kbps: null,
      isLossless: true,
      belowRequest: false,
      isFallback: true,
    })!;
    expect(v.source).toBe('Local Library');
    expect(v.isFallback).toBe(true);
  });

  it('handles unknown codec/lossless (facts may be just the bitrate)', () => {
    const v = mapStreamInfo({
      source: 'addon',
      codec: null,
      kbps: null,
      isLossless: null,
      belowRequest: false,
      isFallback: false,
    })!;
    expect(v.facts).toBe('');
    expect(v.source).toBe('Addon');
  });
});
