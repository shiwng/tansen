/**
 * Google sign-in state. The backend owns the whole OAuth flow (Authorization
 * Code + PKCE in the system browser, loopback redirect, Keychain storage);
 * this store holds only the public profile fields the sidebar shows and
 * mirrors the `google_auth_changed` event so every surface stays in sync.
 * Tokens never reach the frontend.
 */
import { create } from 'zustand';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';

export interface GoogleProfile {
  sub: string;
  name: string;
  email: string;
  picture: string;
}

interface GoogleState {
  profile: GoogleProfile | null;
  /** A sign-in round trip is in flight (the browser may be open). */
  signingIn: boolean;
  /** The last sign-in failure, user-displayable; cleared on the next attempt. */
  error: string | null;
  signIn: () => Promise<void>;
  signOut: () => Promise<void>;
  /** Sign out, then run a fresh chooser-based sign-in. */
  switchAccount: () => Promise<void>;
}

export const useGoogle = create<GoogleState>((set) => ({
  profile: null,
  signingIn: false,
  error: null,
  signIn: async () => {
    if (useGoogle.getState().signingIn) return;
    set({ signingIn: true, error: null });
    try {
      const profile = await invoke<GoogleProfile | null>('google_auth_start');
      // The event is authoritative; this guards against a dropped event.
      if (profile) set({ profile });
    } catch (e) {
      set({ error: String(e) });
    } finally {
      set({ signingIn: false });
    }
  },
  signOut: async () => {
    try {
      await invoke('google_sign_out');
      // The backend emits the same null; clearing here too makes the row
      // update immediately even if the event is delayed.
      set({ profile: null, error: null });
    } catch (e) {
      set({ error: String(e) });
    }
  },
  switchAccount: async () => {
    await useGoogle.getState().signOut();
    // Only open the chooser after a successful local sign-out; on failure
    // the current profile is still valid and must not be replaced.
    if (useGoogle.getState().profile === null) await useGoogle.getState().signIn();
  },
}));

/** Session restore + event mirror; called once from main.tsx before render. */
export async function initGoogle(): Promise<void> {
  try {
    const profile = await invoke<GoogleProfile | null>('google_auth_status');
    useGoogle.setState({ profile });
  } catch {
    // Not configured or no session: the sidebar shows the sign-in row.
  }
  void listen<GoogleProfile | null>('google_auth_changed', (e) => {
    useGoogle.setState({ profile: e.payload ?? null, error: null });
  });
}
