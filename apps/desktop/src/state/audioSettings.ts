/**
 * Audio preferences backed by the Rust `audio_settings_*` commands.
 *
 * The backend is the owner: every patch is persisted in SQLite and applied to
 * the live pipeline before the command returns. The hook exposes the last
 * known snapshot plus the actual negotiated output format, so the UI can show
 * what the device really accepted instead of what was requested.
 */
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { invoke } from '@tauri-apps/api/core';

/** The flattened snapshot: AudioSettings + OutputCapabilitiesReport. */
export interface AudioSettings {
  outputBits: number;
  gapless: boolean;
  crossfadeMs: number;
  crossfadeCurve: 'linear' | 'equal-power';
  loudness: boolean;
  skipSilence: boolean;
  skipThresholdDb: number;
  skipMinMs: number;
  eqEnabled: boolean;
  eqGains: number[];
  eqPreset: string | null;
  /** Real output device facts (null when no device could be queried). */
  deviceName: string | null;
  channels: number | null;
  sampleRate: number | null;
  formats: string[];
  supports16Bit: boolean;
  supports32Bit: boolean;
  atmosSupported: boolean;
  atmosReason: string;
  negotiatedBits: number;
  negotiatedFormat: string | null;
}

export interface AudioSettingsPatch {
  outputBits?: number;
  gapless?: boolean;
  crossfadeMs?: number;
  crossfadeCurve?: 'linear' | 'equal-power';
  loudness?: boolean;
  skipSilence?: boolean;
  skipThresholdDb?: number;
  skipMinMs?: number;
  eqEnabled?: boolean;
  eqGains?: number[];
  eqPreset?: string;
}

export const AUDIO_SETTINGS_KEY = ['audio-settings'] as const;

async function getSettings(): Promise<AudioSettings> {
  return invoke<AudioSettings>('audio_settings_get');
}

export function useAudioSettings() {
  const queryClient = useQueryClient();
  const query = useQuery({
    queryKey: AUDIO_SETTINGS_KEY,
    queryFn: getSettings,
    staleTime: 30_000,
  });
  const mutation = useMutation({
    mutationFn: (patch: AudioSettingsPatch) =>
      invoke<AudioSettings>('audio_settings_set', { patch }),
    onSuccess: (data) => {
      queryClient.setQueryData(AUDIO_SETTINGS_KEY, data);
    },
  });
  return {
    settings: query.data,
    loading: query.isLoading,
    error: query.error ?? mutation.error,
    set: (patch: AudioSettingsPatch) => mutation.mutate(patch),
    setAsync: (patch: AudioSettingsPatch) => mutation.mutateAsync(patch),
  };
}

/** Frequencies of the ten EQ bands, mirrored from the Rust DSP. */
export const EQ_BANDS_HZ = [
  31, 62, 125, 250, 500, 1_000, 2_000, 4_000, 8_000, 16_000,
] as const;

export const EQ_PRESETS = [
  'flat',
  'bass-boost',
  'treble-boost',
  'rock',
  'pop',
  'classical',
  'jazz',
  'electronic',
  'vocal',
] as const;

/** Hz → compact display label ("31", "1k", "16k"). */
export function bandLabel(hz: number): string {
  if (hz >= 1_000) return `${hz / 1_000}k`;
  return String(hz);
}
