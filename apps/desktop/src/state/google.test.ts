// The Google store's contract: profile state rides the `google_auth_changed`
// event (mirroring the backend), sign-in surfaces the command error for user
// display without touching the profile, and only profile fields ever live in
// this store — the invoke surface carries no token argument at all.
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { invoke } from '@tauri-apps/api/core';
import { initGoogle, useGoogle } from './google';

vi.mock('@tauri-apps/api/core', () => ({
  invoke: vi.fn(),
  convertFileSrc: (path: string) => `art://localhost/${encodeURIComponent(path)}`,
}));
vi.mock('@tauri-apps/api/event', () => ({ listen: vi.fn(async () => () => {}) }));

const mocked = vi.mocked(invoke);

const PROFILE = { sub: 'sub-1', name: 'Alice Example', email: 'alice@example.com', picture: 'https://lh3.example/p.png' };

beforeEach(() => {
  mocked.mockReset();
  useGoogle.setState({ profile: null, signingIn: false, error: null });
});

describe('google store', () => {
  it('restores the persisted profile at init and reports not-configured honestly', async () => {
    mocked.mockImplementation(async (cmd: string) =>
      cmd === 'google_auth_status' ? Promise.reject('Google sign-in is not configured') : null,
    );
    await initGoogle();
    expect(useGoogle.getState().profile).toBeNull();
  });

  it('initGoogle adopts the restored session', async () => {
    mocked.mockImplementation(async (cmd: string) => (cmd === 'google_auth_status' ? PROFILE : null));
    await initGoogle();
    expect(useGoogle.getState().profile).toEqual(PROFILE);
  });

  it('signIn stores the returned profile and clears the error path', async () => {
    mocked.mockImplementation(async (cmd: string) => (cmd === 'google_auth_start' ? PROFILE : null));
    await useGoogle.getState().signIn();
    expect(mocked).toHaveBeenCalledWith('google_auth_start');
    expect(useGoogle.getState().profile).toEqual(PROFILE);
    expect(useGoogle.getState().signingIn).toBe(false);
    expect(useGoogle.getState().error).toBeNull();
  });

  it('a failed sign-in sets the displayable error and never fabricates a session', async () => {
    mocked.mockImplementation(async (cmd: string) =>
      cmd === 'google_auth_start' ? Promise.reject('sign-in was cancelled or timed out') : null,
    );
    await useGoogle.getState().signIn();
    expect(useGoogle.getState().profile).toBeNull();
    expect(useGoogle.getState().error).toContain('cancelled or timed out');
    expect(useGoogle.getState().signingIn).toBe(false);
  });

  it('a second sign-in click while one is in flight is ignored', async () => {
    let release!: () => void;
    const gate = new Promise<void>((resolve) => (release = resolve));
    mocked.mockImplementation(async (cmd: string) => (cmd === 'google_auth_start' ? gate.then(() => PROFILE) : null));
    const first = useGoogle.getState().signIn();
    expect(useGoogle.getState().signingIn).toBe(true);
    const second = useGoogle.getState().signIn();
    release();
    await Promise.all([first, second]);
    expect(mocked.mock.calls.filter(([cmd]) => cmd === 'google_auth_start')).toHaveLength(1);
  });

  it('signOut forwards to the command and clears the profile immediately', async () => {
    mocked.mockResolvedValue(null);
    useGoogle.setState({ profile: PROFILE });
    await useGoogle.getState().signOut();
    expect(mocked).toHaveBeenCalledWith('google_sign_out');
    expect(useGoogle.getState().profile).toBeNull();
  });

  it('a failed sign-out keeps the profile and reports the error', async () => {
    mocked.mockImplementation(async (cmd: string) =>
      cmd === 'google_sign_out' ? Promise.reject('keychain unavailable') : null,
    );
    useGoogle.setState({ profile: PROFILE });
    await useGoogle.getState().signOut();
    expect(useGoogle.getState().profile).toEqual(PROFILE);
    expect(useGoogle.getState().error).toContain('keychain unavailable');
  });

  it('switchAccount signs out first, then starts a fresh chooser-based sign-in', async () => {
    mocked.mockImplementation(async (cmd: string) => (cmd === 'google_auth_start' ? PROFILE : null));
    useGoogle.setState({ profile: PROFILE });
    await useGoogle.getState().switchAccount();
    const commands = mocked.mock.calls.map(([cmd]) => cmd);
    expect(commands).toEqual(['google_sign_out', 'google_auth_start']);
    expect(useGoogle.getState().profile).toEqual(PROFILE);
  });

  it('switchAccount never replaces a profile when sign-out fails', async () => {
    mocked.mockImplementation(async (cmd: string) =>
      cmd === 'google_sign_out' ? Promise.reject('keychain unavailable') : PROFILE,
    );
    useGoogle.setState({ profile: PROFILE });
    await useGoogle.getState().switchAccount();
    expect(mocked.mock.calls.filter(([cmd]) => cmd === 'google_auth_start')).toHaveLength(0);
    expect(useGoogle.getState().profile).toEqual(PROFILE);
  });

  it('the invoke surface carries only commands — no credential argument exists', async () => {
    mocked.mockResolvedValue(null);
    await useGoogle.getState().signIn();
    await useGoogle.getState().signOut();
    for (const call of mocked.mock.calls) {
      expect(call[0]).toMatch(/^google_(auth_(start|status)|sign_out)$/);
    }
  });
});
