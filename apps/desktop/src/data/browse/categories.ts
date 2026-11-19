/**
 * Browse Categories: the genres, moods and moments Tansen can actually search
 * in the catalogue. `query` is sent to `discovery_search`; every shelf on the
 * detail page reuses it with a different filter, so the content is always
 * real catalogue data rather than a local hand-written list.
 *
 * `tint` is the fallback artwork: a category whose catalogue search has no
 * usable thumbnail still gets an identifiable, readable tile while the image
 * pipeline is loading or offline.
 */
export interface BrowseCategory {
  /** Stable URL slug, also the React Query cache key. */
  id: string;
  /** Card and detail-page title. */
  label: string;
  /** Catalogue query shared by the detail shelves. */
  query: string;
  /** Artist whose catalogue portrait is the tile's hero image. */
  art: string;
  /** A short description of the genre or moment, shown on the detail page. */
  description: string;
  /** Gradient start/end used behind and over the artwork. */
  tint: readonly [string, string];
}

export const BROWSE_CATEGORIES: readonly BrowseCategory[] = [
  {
    id: 'bollywood',
    art: 'Arijit Singh',
    label: 'Bollywood',
    query: 'Bollywood',
    description: 'Hindi film songs, evergreen classics and the latest soundtracks from Mumbai.',
    tint: ['#8f5bd6', '#3f2a73'],
  },
  {
    id: 'punjabi',
    art: 'Diljit Dosanjh',
    label: 'Punjabi',
    query: 'Punjabi',
    description: 'Bhangra beats, Punjabi pop and folk-rooted hits from Punjab and the diaspora.',
    tint: ['#7d57c9', '#2f2450'],
  },
  {
    id: 'tamil',
    art: 'Anirudh Ravichander',
    label: 'Tamil',
    query: 'Tamil',
    description: 'Kollywood soundtracks, independent Tamil pop and melodies from the Tamil industry.',
    tint: ['#7a5bd0', '#33255c'],
  },
  {
    id: 'pop',
    art: 'Dua Lipa',
    label: 'Pop',
    query: 'Pop hits',
    description: 'The biggest pop hooks, chart singles and rising names from around the world.',
    tint: ['#ff375f', '#b3123a'],
  },
  {
    id: 'telugu',
    art: 'S. P. Balasubrahmanyam',
    label: 'Telugu',
    query: 'Telugu',
    description: 'Tollywood songs, Telugu film soundtracks and the voices shaping Andhra and Telangana pop.',
    tint: ['#8f6bd8', '#3a2a6b'],
  },
  {
    id: 'charts',
    art: 'Ariana Grande',
    label: 'Charts',
    query: 'Music charts',
    description: "What's climbing this week across pop, hip-hop, rock and dance.",
    tint: ['#7f7a1f', '#3c3a0d'],
  },
  {
    id: 'rock',
    art: 'Nirvana',
    label: 'Rock',
    query: 'Rock',
    description: 'Guitar-driven anthems, classic rock and new bands carrying the sound forward.',
    tint: ['#ff453a', '#8c1d16'],
  },
  {
    id: 'metal',
    art: 'Metallica',
    label: 'Metal',
    query: 'Metal',
    description: 'Heavy riffs, extreme metal and the louder edges of rock.',
    tint: ['#e5484d', '#7c1618'],
  },
  {
    id: 'hip-hop',
    art: 'Drake',
    label: 'Hip-Hop',
    query: 'Hip-Hop',
    description: 'Rap, trap and drill from the artists defining the sound right now.',
    tint: ['#b86bd6', '#4a2364'],
  },
  {
    id: 'dance',
    art: 'Calvin Harris',
    label: 'Dance',
    query: 'Dance music',
    description: 'Club cuts, house, techno and electronic tracks built for the floor.',
    tint: ['#12b5a5', '#0a5d55'],
  },
  {
    id: 'love',
    art: 'Adele',
    label: 'Love',
    query: 'Love songs',
    description: 'Romantic ballads, duets and slow songs for every kind of love story.',
    tint: ['#ff6b8a', '#a52a41'],
  },
  {
    id: 'fitness',
    art: 'David Guetta',
    label: 'Fitness',
    query: 'Workout music',
    description: 'High-energy tracks for the gym, running and everything that makes you sweat.',
    tint: ['#7ac943', '#2f5c14'],
  },
  {
    id: 'sleep',
    art: 'Brian Eno',
    label: 'Sleep',
    query: 'Sleep music',
    description: 'Quiet instrumentals, ambient soundscapes and music for winding down.',
    tint: ['#5a6bd8', '#25306b'],
  },
  {
    id: 'focus',
    art: 'Hans Zimmer',
    label: 'Focus',
    query: 'Deep focus',
    description: 'Deep-focus instrumentals, ambient and classical music for study and work.',
    tint: ['#e0a21a', '#7a5306'],
  },
  {
    id: 'jazz',
    art: 'Miles Davis',
    label: 'Jazz',
    query: 'Jazz',
    description: 'Standards, modern jazz and the players moving the music forward.',
    tint: ['#c98a3a', '#5c3a12'],
  },
  {
    id: 'live-music',
    art: 'The 1975',
    label: 'Live Music',
    query: 'Live music',
    description: 'Concert recordings, festival sets and performances captured on stage.',
    tint: ['#b8871f', '#543d0b'],
  },
  {
    id: 'k-pop',
    art: 'BLACKPINK',
    label: 'K-Pop',
    query: 'K-Pop',
    description: 'Korean pop, idol groups and the singles driving the global K-pop wave.',
    tint: ['#ff5d8f', '#a01f47'],
  },
  {
    id: 'indie',
    art: 'Arctic Monkeys',
    label: 'Indie',
    query: 'Indie',
    description: 'Independent voices, bedroom pop and guitar music outside the mainstream.',
    tint: ['#3fbf8f', '#155842'],
  },
  {
    id: 'best-of-2026',
    art: 'Taylor Swift',
    label: 'Best of 2026',
    query: 'Best of 2026',
    description: 'The standout songs, albums and playlists from 2026 so far, refreshed from the catalogue.',
    tint: ['#0a84ff', '#003a80'],
  },
];

const BY_ID = new Map(BROWSE_CATEGORIES.map((category) => [category.id, category]));

/** The category for a route param, tolerating an encoded slug. */
export function findBrowseCategory(raw: string | undefined): BrowseCategory | undefined {
  if (!raw) return undefined;
  let id = raw;
  try {
    id = decodeURIComponent(raw);
  } catch {
    // A malformed escape is not a category slug; the raw value gets its chance.
  }
  return BY_ID.get(id);
}
