/** Video playback is a YouTube-catalogue feature and is not implemented on desktop. */
import { create } from 'zustand';

interface VideoState {
  videoId: string | null;
  quality: 'auto' | number;
}

export const useVideo = create<VideoState>(() => ({ videoId: null, quality: 'auto' }));

export function useHasVideo(_videoId: string | null | undefined): boolean {
  return false;
}
