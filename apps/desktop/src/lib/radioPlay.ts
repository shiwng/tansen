/**
 * Starting a station: cache its favicon, remember it as recently played,
 * register the click with the directory, then hand the live stream to the
 * normal player store. Playback itself is the engine's Radio source.
 */
import { usePlayer } from '@/state/player';
import { stationTrack, useRadio } from '@/state/radio';
import { radioArtwork, radioClick, type RadioStation } from './radio';

export async function playStation(station: RadioStation): Promise<void> {
  const artwork = await radioArtwork(station.favicon);
  useRadio.getState().addRecent(station);
  radioClick(station.id);
  await usePlayer.getState().playTrack(stationTrack(station, artwork), null);
}

/** A user-added stream URL: no directory metadata exists, so none is shown. */
export async function playCustomStation(name: string, url: string): Promise<void> {
  const track = stationTrack(
    { id: `custom:${url}`, name: name.trim() || url, url, country: '', tags: '' },
    null,
  );
  await usePlayer.getState().playTrack(track, null);
}
