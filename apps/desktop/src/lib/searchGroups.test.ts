import { describe, expect, it } from 'vitest';
import {
  attachStatuses,
  groupBySource,
  rowToTrack,
  sourceLabel,
  statusLine,
  type SearchTrackRow,
  type SourceOutcome,
} from './searchGroups';

const row = (source: string, id: string, title = `T ${id}`): SearchTrackRow => ({
  key: { source, id },
  title,
  artist: 'Artist',
  album: null,
  duration_sec: 180,
  artwork_url: null,
});

const ok = (source: string): SourceOutcome => ({
  source,
  status: { state: 'ok' },
});
const timeout = (source: string): SourceOutcome => ({
  source,
  status: { state: 'timeout' },
});
const error = (source: string, message = 'boom'): SourceOutcome => ({
  source,
  status: { state: 'error', message },
});

describe('groupBySource', () => {
  it('groups contiguous rows per source in backend rank order', () => {
    // Backend rank: addon(0) < local(1) < subsonic(2) < youtube(3).
    const groups = groupBySource([
      row('addon', 'a1'),
      row('addon', 'a2'),
      row('local', '42'),
      row('subsonic', 's1'),
      row('youtube', 'y1'),
    ]);
    expect(groups.map((g) => g.source)).toEqual([
      'addon',
      'local',
      'subsonic',
      'youtube',
    ]);
    expect(groups[0].rows).toHaveLength(2);
  });

  it('does not merge non-adjacent same-source groups (backend order is truth)', () => {
    const groups = groupBySource([row('addon', 'a1'), row('local', '42'), row('addon', 'a2')]);
    expect(groups.map((g) => g.source)).toEqual(['addon', 'local', 'addon']);
  });

  it('maps local rows to the local: videoId convention and drops sourceKind', () => {
    const t = rowToTrack(row('local', '42'));
    expect(t.videoId).toBe('local:42');
    expect(t.sourceKind).toBeUndefined();
    const r = rowToTrack(row('youtube', 'v1'));
    expect(r.videoId).toBe('v1');
    expect(r.sourceKind).toBe('youtube');
  });
});

describe('attachStatuses', () => {
  it('attaches statuses to matching groups', () => {
    const groups = groupBySource([row('youtube', 'y1')]);
    const withStatus = attachStatuses(groups, [ok('youtube'), timeout('addon')]);
    expect(withStatus[0].status?.state).toBe('ok');
    // The timed-out source with no rows still appears.
    expect(withStatus.some((g) => g.source === 'addon' && g.status?.state === 'timeout')).toBe(true);
  });

  it('keeps ok-but-empty sources out of the list', () => {
    const withStatus = attachStatuses([], [ok('subsonic')]);
    expect(withStatus).toHaveLength(0);
  });
});

describe('statusLine (partial-failure presentation)', () => {
  it('null when every source answered ok', () => {
    expect(statusLine([ok('youtube'), ok('addon')])).toBeNull();
  });
  it('names the timed-out source', () => {
    expect(statusLine([ok('youtube'), timeout('addon')])).toContain('Addon timed out');
  });
  it('names the failed source with its message', () => {
    expect(statusLine([error('subsonic', 'auth')])).toContain('Subsonic failed: auth');
    expect(statusLine([error('subsonic')])).toContain('results below are from the rest');
  });
});

describe('sourceLabel', () => {
  it('labels the known kinds and passes unknowns through', () => {
    expect(sourceLabel('youtube')).toBe('YouTube Music');
    expect(sourceLabel('local')).toBe('Local Library');
    expect(sourceLabel('mystery')).toBe('mystery');
  });
});
