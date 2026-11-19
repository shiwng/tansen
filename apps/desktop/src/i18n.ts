/**
 * Lightweight UI localization. No runtime dependency: one dictionary per
 * locale, a zustand store for the live switch, and `Intl` for plurals,
 * numbers and dates. Untranslated keys fall back to English, so adding a
 * language never blanks the UI.
 *
 * Persistence is the same localStorage contract the rest of the player
 * preferences use; the UI updates immediately (`document.documentElement`
 * lang/dir) and every subscribed component re-renders.
 */
import { create } from 'zustand';
import { useMemo } from 'react';

export const LOCALES = [
  { id: 'en', name: 'English', native: 'English', dir: 'ltr' },
  { id: 'hi', name: 'Hindi', native: 'हिन्दी', dir: 'ltr' },
  { id: 'es', name: 'Spanish', native: 'Español', dir: 'ltr' },
  { id: 'fr', name: 'French', native: 'Français', dir: 'ltr' },
  { id: 'de', name: 'German', native: 'Deutsch', dir: 'ltr' },
  { id: 'pt', name: 'Portuguese', native: 'Português', dir: 'ltr' },
  { id: 'ja', name: 'Japanese', native: '日本語', dir: 'ltr' },
  { id: 'ko', name: 'Korean', native: '한국어', dir: 'ltr' },
  { id: 'zh', name: 'Simplified Chinese', native: '简体中文', dir: 'ltr' },
  { id: 'ar', name: 'Arabic', native: 'العربية', dir: 'rtl' },
  { id: 'ru', name: 'Russian', native: 'Русский', dir: 'ltr' },
] as const;

export type Locale = (typeof LOCALES)[number]['id'];

export type MessageKey =
  | 'nav.home'
  | 'nav.explore'
  | 'nav.library'
  | 'nav.settings'
  | 'nav.search'
  | 'nav.radio'
  | 'nav.recent'
  | 'nav.artists'
  | 'nav.albums'
  | 'nav.songs'
  | 'nav.librarySection'
  | 'nav.playlists'
  | 'nav.allPlaylists'
  | 'common.play'
  | 'common.pause'
  | 'common.next'
  | 'common.previous'
  | 'common.save'
  | 'common.cancel'
  | 'common.remove'
  | 'common.done'
  | 'settings.title'
  | 'settings.audio'
  | 'settings.playback'
  | 'settings.appearance'
  | 'settings.language'
  | 'settings.library'
  | 'settings.lyrics'
  | 'settings.sources'
  | 'settings.spotify'
  | 'settings.about'
  | 'settings.downloads'
  | 'audio.streamingQuality'
  | 'audio.streamingQualityHint'
  | 'audio.downloads'
  | 'audio.downloadsHint'
  | 'audio.atmos'
  | 'audio.atmosHint'
  | 'audio.atmosUnavailable'
  | 'audio.outputPrecision'
  | 'audio.outputPrecisionHint'
  | 'audio.outputAuto'
  | 'audio.output16'
  | 'audio.output32'
  | 'audio.negotiated'
  | 'audio.device'
  | 'playback.repeat'
  | 'playback.repeatHint'
  | 'playback.shuffle'
  | 'playback.shuffleHint'
  | 'playback.autoplay'
  | 'playback.autoplayHint'
  | 'playback.gapless'
  | 'playback.gaplessHint'
  | 'playback.crossfade'
  | 'playback.crossfadeHint'
  | 'playback.crossfadeOff'
  | 'playback.crossfadeCurve'
  | 'playback.crossfadeCurveHint'
  | 'playback.curveLinear'
  | 'playback.curveEqualPower'
  | 'playback.loudness'
  | 'playback.loudnessHint'
  | 'playback.skipSilence'
  | 'playback.skipSilenceHint'
  | 'playback.skipThreshold'
  | 'playback.skipMin'
  | 'playback.equalizer'
  | 'playback.equalizerHint'
  | 'playback.eqPreset'
  | 'playback.eqReset'
  | 'playback.nerdStats'
  | 'playback.nerdStatsHint'
  | 'language.appLanguage'
  | 'language.appLanguageHint'
  | 'about.version'
  | 'downloads.chooseFolder'
  | 'downloads.storage'
  | 'downloads.queueEmpty'
  | 'downloads.queued'
  | 'downloads.downloading'
  | 'downloads.paused'
  | 'downloads.complete'
  | 'downloads.failed'
  | 'downloads.cancelled'
  | 'downloads.pause'
  | 'downloads.resume'
  | 'downloads.retry'
  | 'downloads.cancel'
  | 'downloads.remove'
  | 'downloads.clearCompleted'
  | 'downloads.unsupported'
  | 'downloads.unavailable'
  | 'downloads.itemCount'
  | 'downloads.itemCount.zero'
  | 'downloads.itemCount.one'
  | 'downloads.itemCount.two'
  | 'downloads.itemCount.few'
  | 'downloads.itemCount.many'
  | 'downloads.itemCount.other'
  | 'downloads.completedAt'
  | 'downloads.chooseAction'
  | 'downloads.supported'
  | 'downloads.activeCount'
  | 'downloads.emptyHint';

type Dict = Partial<Record<MessageKey, string>>;

const en: Record<MessageKey, string> = {
  'nav.home': 'Home',
  'nav.explore': 'Explore',
  'nav.library': 'Library',
  'nav.settings': 'Settings',
  'nav.search': 'Search',
  'nav.radio': 'Radio',
  'nav.recent': 'Recently Added',
  'nav.artists': 'Artists',
  'nav.albums': 'Albums',
  'nav.songs': 'Songs',
  'nav.librarySection': 'Library',
  'nav.playlists': 'Playlists',
  'nav.allPlaylists': 'All Playlists',
  'common.play': 'Play',
  'common.pause': 'Pause',
  'common.next': 'Next',
  'common.previous': 'Previous',
  'common.save': 'Save',
  'common.cancel': 'Cancel',
  'common.remove': 'Remove',
  'common.done': 'Done',
  'settings.title': 'Settings',
  'settings.audio': 'Audio',
  'settings.playback': 'Playback',
  'settings.appearance': 'Appearance',
  'settings.language': 'Language',
  'settings.library': 'Library',
  'settings.lyrics': 'Lyrics',
  'settings.sources': 'Sources',
  'settings.spotify': 'Spotify Import',
  'settings.about': 'About',
  'settings.downloads': 'Downloads',
  'audio.streamingQuality': 'Streaming quality',
  'audio.streamingQualityHint':
    "Caps future streams. Sources that can't honor it say so in the stream info: YouTube Music serves one AAC tier, Subsonic streams raw, addon servers map Low/High",
  'audio.downloads': 'Downloads',
  'audio.downloadsHint':
    'Download tracks you own from Subsonic or export local files. YouTube Music and Spotify audio cannot be downloaded.',
  'audio.atmos': 'Dolby Atmos',
  'audio.atmosHint':
    'Only offered when the whole path supports Dolby rendering. Tansen mixes to stereo PCM.',
  'audio.atmosUnavailable': 'Not available',
  'audio.outputPrecision': 'Output precision',
  'audio.outputPrecisionHint':
    'Negotiated with the real output device. Internal mixing stays 32-bit float.',
  'audio.outputAuto': 'Device default',
  'audio.output16': '16-bit integer',
  'audio.output32': '32-bit float',
  'audio.negotiated': 'Active format',
  'audio.device': 'Device',
  'playback.repeat': 'Repeat',
  'playback.repeatHint': 'Cycles off, all, one — the pill’s repeat button does the same',
  'playback.shuffle': 'Shuffle',
  'playback.shuffleHint': 'Shuffles the next selection when you press play on a list',
  'playback.autoplay': 'Autoplay',
  'playback.autoplayHint': 'Adds related tracks when the queue ends (catalogue sources)',
  'playback.gapless': 'Gapless playback',
  'playback.gaplessHint': 'Preloads the next track so albums run without a gap',
  'playback.crossfade': 'Crossfade / Auto Mix',
  'playback.crossfadeHint': 'Overlaps the end of one track with the start of the next',
  'playback.crossfadeOff': 'Off',
  'playback.crossfadeCurve': 'Crossfade curve',
  'playback.crossfadeCurveHint':
    'Linear ramps evenly; equal-power keeps the summed level steady through the overlap',
  'playback.curveLinear': 'Linear',
  'playback.curveEqualPower': 'Equal power',
  'playback.loudness': 'Loudness normalization',
  'playback.loudnessHint':
    'Measures local files in the background (EBU R128) and evens out their level, clip-safe. Remote streams play at unity until analysed.',
  'playback.skipSilence': 'Skip silence',
  'playback.skipSilenceHint':
    'Fast-forwards quiet passages longer than the minimum. The first moments are still heard, and quiet music is left alone.',
  'playback.skipThreshold': 'Silence threshold',
  'playback.skipMin': 'Minimum silence',
  'playback.equalizer': 'Equalizer',
  'playback.equalizerHint': 'Ten-band peaking EQ applied in the audio callback',
  'playback.eqPreset': 'Preset',
  'playback.eqReset': 'Reset to Flat',
  'playback.nerdStats': 'Nerd stats',
  'playback.nerdStatsHint': 'Shows the stream source, codec and bitrate line in the full-screen player',
  'language.appLanguage': 'App language',
  'language.appLanguageHint': 'Applies immediately; Arabic switches the layout to right-to-left',
  'about.version': 'Version',
  'downloads.chooseFolder': 'Choose downloads folder…',
  'downloads.storage': 'Storage',
  'downloads.queueEmpty': 'No downloads yet',
  'downloads.queued': 'Queued',
  'downloads.downloading': 'Downloading',
  'downloads.paused': 'Paused',
  'downloads.complete': 'Complete',
  'downloads.failed': 'Failed',
  'downloads.cancelled': 'Cancelled',
  'downloads.pause': 'Pause',
  'downloads.resume': 'Resume',
  'downloads.retry': 'Retry',
  'downloads.cancel': 'Cancel',
  'downloads.remove': 'Remove',
  'downloads.clearCompleted': 'Clear completed',
  'downloads.unsupported': 'Not supported by this provider',
  'downloads.unavailable': 'Unavailable',
  'downloads.itemCount': '{count} items',
  'downloads.itemCount.zero': '{count} items',
  'downloads.itemCount.one': '{count} item',
  'downloads.itemCount.two': '{count} items',
  'downloads.itemCount.few': '{count} items',
  'downloads.itemCount.many': '{count} items',
  'downloads.itemCount.other': '{count} items',
  'downloads.completedAt': 'Completed {date}',
  'downloads.chooseAction': 'Choose…',
  'downloads.supported': 'Supported',
  'downloads.activeCount': '{count} downloading…',
  'downloads.emptyHint':
    'Use a track’s menu (“Download”) on Subsonic tracks or local files.',
};

const hi: Dict = {
  'nav.home': 'होम', 'nav.explore': 'खोजें', 'nav.library': 'लाइब्रेरी', 'nav.settings': 'सेटिंग्स',
  'nav.search': 'खोज', 'nav.radio': 'रेडियो', 'nav.recent': 'हाल में जोड़े', 'nav.artists': 'कलाकार',
  'nav.albums': 'एल्बम', 'nav.songs': 'गाने', 'nav.librarySection': 'लाइब्रेरी', 'nav.playlists': 'प्लेलिस्ट',
  'nav.allPlaylists': 'सभी प्लेलिस्ट',
  'common.play': 'चलाएँ', 'common.pause': 'रोकें', 'common.next': 'अगला', 'common.previous': 'पिछला',
  'common.save': 'सहेजें', 'common.cancel': 'रद्द करें', 'common.remove': 'हटाएँ', 'common.done': 'हो गया',
  'settings.title': 'सेटिंग्स', 'settings.audio': 'ऑडियो', 'settings.playback': 'प्लेबैक',
  'settings.appearance': 'रूप', 'settings.language': 'भाषा', 'settings.library': 'लाइब्रेरी',
  'settings.lyrics': 'गीत', 'settings.sources': 'स्रोत', 'settings.spotify': 'Spotify आयात',
  'settings.about': 'परिचय', 'settings.downloads': 'डाउनलोड',
  'audio.streamingQuality': 'स्ट्रीमिंग गुणवत्ता', 'audio.downloads': 'डाउनलोड',
  'audio.atmos': 'डॉल्बी एटमॉस', 'audio.outputPrecision': 'आउटपुट परिशुद्धता',
  'audio.outputAuto': 'डिवाइस डिफ़ॉल्ट', 'audio.output16': '16-बिट पूर्णांक', 'audio.output32': '32-बिट फ़्लोट',
  'audio.negotiated': 'सक्रिय फ़ॉर्मेट', 'audio.device': 'डिवाइस', 'audio.atmosUnavailable': 'उपलब्ध नहीं',
  'playback.repeat': 'दोहराएँ', 'playback.shuffle': 'शफ़ल', 'playback.autoplay': 'ऑटोप्ले',
  'playback.gapless': 'गैपलेस प्लेबैक', 'playback.crossfade': 'क्रॉसफ़ेड / ऑटो मिक्स',
  'playback.crossfadeOff': 'बंद', 'playback.loudness': 'लाउडनेस सामान्यीकरण',
  'playback.skipSilence': 'मौन छोड़ें', 'playback.equalizer': 'इक्वलाइज़र',
  'playback.eqPreset': 'प्रीसेट', 'playback.eqReset': 'फ़्लैट पर रीसेट', 'playback.nerdStats': 'तकनीकी जानकारी',
  'language.appLanguage': 'ऐप भाषा',
  'about.version': 'संस्करण',
  'downloads.itemCount.one': '{count} आइटम', 'downloads.itemCount.other': '{count} आइटम',
  'downloads.completedAt': '{date} को पूर्ण',
  'downloads.chooseAction': 'चुनें…', 'downloads.supported': 'समर्थित', 'downloads.activeCount': '{count} डाउनलोड हो रहे हैं…',
  'downloads.emptyHint': 'Subsonic ट्रैक या स्थानीय फ़ाइलों पर ट्रैक मेनू (“Download”) से करें।',
  'playback.crossfadeCurve': 'क्रॉसफ़ेड वक्र', 'playback.crossfadeCurveHint': 'लीनियर समान रूप से बदलता है; इक्वल-पावर ओवरलैप में कुल स्तर स्थिर रखता है',
  'playback.curveLinear': 'लीनियर', 'playback.curveEqualPower': 'इक्वल पावर',
};

const es: Dict = {
  'nav.home': 'Inicio', 'nav.explore': 'Explorar', 'nav.library': 'Biblioteca', 'nav.settings': 'Ajustes',
  'nav.search': 'Buscar', 'nav.radio': 'Radio', 'nav.recent': 'Añadidos recientemente', 'nav.artists': 'Artistas',
  'nav.albums': 'Álbumes', 'nav.songs': 'Canciones', 'nav.librarySection': 'Biblioteca',
  'nav.playlists': 'Listas', 'nav.allPlaylists': 'Todas las listas',
  'common.play': 'Reproducir', 'common.pause': 'Pausa', 'common.next': 'Siguiente', 'common.previous': 'Anterior',
  'common.save': 'Guardar', 'common.cancel': 'Cancelar', 'common.remove': 'Quitar', 'common.done': 'Listo',
  'settings.title': 'Ajustes', 'settings.audio': 'Audio', 'settings.playback': 'Reproducción',
  'settings.appearance': 'Apariencia', 'settings.language': 'Idioma', 'settings.library': 'Biblioteca',
  'settings.lyrics': 'Letras', 'settings.sources': 'Fuentes', 'settings.spotify': 'Importar Spotify',
  'settings.about': 'Acerca de', 'settings.downloads': 'Descargas',
  'audio.streamingQuality': 'Calidad de streaming', 'audio.downloads': 'Descargas',
  'audio.atmos': 'Dolby Atmos', 'audio.outputPrecision': 'Precisión de salida',
  'audio.outputAuto': 'Predeterminado del dispositivo', 'audio.output16': 'Entero de 16 bits',
  'audio.output32': 'Flotante de 32 bits', 'audio.negotiated': 'Formato activo', 'audio.device': 'Dispositivo',
  'audio.atmosUnavailable': 'No disponible',
  'playback.repeat': 'Repetir', 'playback.shuffle': 'Aleatorio', 'playback.autoplay': 'Reproducción automática',
  'playback.gapless': 'Reproducción sin pausas', 'playback.crossfade': 'Fundido / Auto Mix',
  'playback.crossfadeOff': 'Desactivado', 'playback.loudness': 'Normalización de volumen',
  'playback.skipSilence': 'Saltar silencio', 'playback.equalizer': 'Ecualizador',
  'playback.eqPreset': 'Preajuste', 'playback.eqReset': 'Restablecer a plano', 'playback.nerdStats': 'Datos técnicos',
  'language.appLanguage': 'Idioma de la app',
  'about.version': 'Versión',
  'downloads.itemCount.one': '{count} elemento', 'downloads.itemCount.other': '{count} elementos',
  'downloads.completedAt': 'Completado {date}',
  'downloads.chooseAction': 'Elegir…', 'downloads.supported': 'Compatible', 'downloads.activeCount': '{count} descargando…',
  'downloads.emptyHint': 'Usa el menú de un título («Descargar») en títulos de Subsonic o archivos locales.',
  'playback.crossfadeCurve': 'Curva de fundido', 'playback.crossfadeCurveHint': 'Lineal cambia de forma uniforme; igual potencia mantiene estable el nivel sumado durante el solapamiento',
  'playback.curveLinear': 'Lineal', 'playback.curveEqualPower': 'Igual potencia',
};

const fr: Dict = {
  'nav.home': 'Accueil', 'nav.explore': 'Explorer', 'nav.library': 'Bibliothèque', 'nav.settings': 'Réglages',
  'nav.search': 'Rechercher', 'nav.radio': 'Radio', 'nav.recent': 'Ajouts récents', 'nav.artists': 'Artistes',
  'nav.albums': 'Albums', 'nav.songs': 'Titres', 'nav.librarySection': 'Bibliothèque',
  'nav.playlists': 'Playlists', 'nav.allPlaylists': 'Toutes les playlists',
  'common.play': 'Lire', 'common.pause': 'Pause', 'common.next': 'Suivant', 'common.previous': 'Précédent',
  'common.save': 'Enregistrer', 'common.cancel': 'Annuler', 'common.remove': 'Retirer', 'common.done': 'Terminé',
  'settings.title': 'Réglages', 'settings.audio': 'Audio', 'settings.playback': 'Lecture',
  'settings.appearance': 'Apparence', 'settings.language': 'Langue', 'settings.library': 'Bibliothèque',
  'settings.lyrics': 'Paroles', 'settings.sources': 'Sources', 'settings.spotify': 'Import Spotify',
  'settings.about': 'À propos', 'settings.downloads': 'Téléchargements',
  'audio.streamingQuality': 'Qualité du streaming', 'audio.downloads': 'Téléchargements',
  'audio.atmos': 'Dolby Atmos', 'audio.outputPrecision': 'Précision de sortie',
  'audio.outputAuto': 'Par défaut de l’appareil', 'audio.output16': 'Entier 16 bits',
  'audio.output32': 'Flottant 32 bits', 'audio.negotiated': 'Format actif', 'audio.device': 'Appareil',
  'audio.atmosUnavailable': 'Indisponible',
  'playback.repeat': 'Répéter', 'playback.shuffle': 'Aléatoire', 'playback.autoplay': 'Lecture automatique',
  'playback.gapless': 'Lecture sans coupure', 'playback.crossfade': 'Fondu / Auto Mix',
  'playback.crossfadeOff': 'Désactivé', 'playback.loudness': 'Normalisation du volume',
  'playback.skipSilence': 'Ignorer le silence', 'playback.equalizer': 'Égaliseur',
  'playback.eqPreset': 'Préréglage', 'playback.eqReset': 'Réinitialiser à plat', 'playback.nerdStats': 'Infos techniques',
  'language.appLanguage': 'Langue de l’application',
  'about.version': 'Version',
  'downloads.itemCount.one': '{count} élément', 'downloads.itemCount.other': '{count} éléments',
  'downloads.completedAt': 'Terminé le {date}',
  'downloads.chooseAction': 'Choisir…', 'downloads.supported': 'Pris en charge', 'downloads.activeCount': '{count} en téléchargement…',
  'downloads.emptyHint': 'Utilisez le menu d’un titre (« Télécharger ») pour les titres Subsonic ou les fichiers locaux.',
  'playback.crossfadeCurve': 'Courbe de fondu', 'playback.crossfadeCurveHint': 'Linéaire varie régulièrement ; à puissance égale, le niveau total reste stable pendant le chevauchement',
  'playback.curveLinear': 'Linéaire', 'playback.curveEqualPower': 'Puissance égale',
};

const de: Dict = {
  'nav.home': 'Start', 'nav.explore': 'Entdecken', 'nav.library': 'Mediathek', 'nav.settings': 'Einstellungen',
  'nav.search': 'Suchen', 'nav.radio': 'Radio', 'nav.recent': 'Kürzlich hinzugefügt', 'nav.artists': 'Künstler',
  'nav.albums': 'Alben', 'nav.songs': 'Titel', 'nav.librarySection': 'Mediathek',
  'nav.playlists': 'Playlists', 'nav.allPlaylists': 'Alle Playlists',
  'common.play': 'Abspielen', 'common.pause': 'Pause', 'common.next': 'Weiter', 'common.previous': 'Zurück',
  'common.save': 'Speichern', 'common.cancel': 'Abbrechen', 'common.remove': 'Entfernen', 'common.done': 'Fertig',
  'settings.title': 'Einstellungen', 'settings.audio': 'Audio', 'settings.playback': 'Wiedergabe',
  'settings.appearance': 'Darstellung', 'settings.language': 'Sprache', 'settings.library': 'Mediathek',
  'settings.lyrics': 'Songtexte', 'settings.sources': 'Quellen', 'settings.spotify': 'Spotify-Import',
  'settings.about': 'Über', 'settings.downloads': 'Downloads',
  'audio.streamingQuality': 'Streaming-Qualität', 'audio.downloads': 'Downloads',
  'audio.atmos': 'Dolby Atmos', 'audio.outputPrecision': 'Ausgabegenauigkeit',
  'audio.outputAuto': 'Gerätestandard', 'audio.output16': '16-Bit-Ganzzahl',
  'audio.output32': '32-Bit-Gleitkomma', 'audio.negotiated': 'Aktives Format', 'audio.device': 'Gerät',
  'audio.atmosUnavailable': 'Nicht verfügbar',
  'playback.repeat': 'Wiederholen', 'playback.shuffle': 'Zufall', 'playback.autoplay': 'Autoplay',
  'playback.gapless': 'Lückenlose Wiedergabe', 'playback.crossfade': 'Überblenden / Auto Mix',
  'playback.crossfadeOff': 'Aus', 'playback.loudness': 'Lautheitsnormalisierung',
  'playback.skipSilence': 'Stille überspringen', 'playback.equalizer': 'Equalizer',
  'playback.eqPreset': 'Voreinstellung', 'playback.eqReset': 'Auf Flat zurücksetzen', 'playback.nerdStats': 'Technische Infos',
  'language.appLanguage': 'App-Sprache',
  'about.version': 'Version',
  'downloads.itemCount.one': '{count} Element', 'downloads.itemCount.other': '{count} Elemente',
  'downloads.completedAt': 'Abgeschlossen am {date}',
  'downloads.chooseAction': 'Auswählen…', 'downloads.supported': 'Unterstützt', 'downloads.activeCount': '{count} werden heruntergeladen…',
  'downloads.emptyHint': 'Nutze das Titelmenü („Herunterladen“) bei Subsonic-Titeln oder lokalen Dateien.',
  'playback.crossfadeCurve': 'Überblendkurve', 'playback.crossfadeCurveHint': 'Linear blendet gleichmäßig; gleiche Leistung hält den Summenpegel während der Überlappung stabil',
  'playback.curveLinear': 'Linear', 'playback.curveEqualPower': 'Gleiche Leistung',
};

const pt: Dict = {
  'nav.home': 'Início', 'nav.explore': 'Explorar', 'nav.library': 'Biblioteca', 'nav.settings': 'Ajustes',
  'nav.search': 'Pesquisar', 'nav.radio': 'Rádio', 'nav.recent': 'Adicionados recentemente', 'nav.artists': 'Artistas',
  'nav.albums': 'Álbuns', 'nav.songs': 'Músicas', 'nav.librarySection': 'Biblioteca',
  'nav.playlists': 'Playlists', 'nav.allPlaylists': 'Todas as playlists',
  'common.play': 'Reproduzir', 'common.pause': 'Pausar', 'common.next': 'Próxima', 'common.previous': 'Anterior',
  'common.save': 'Salvar', 'common.cancel': 'Cancelar', 'common.remove': 'Remover', 'common.done': 'Concluído',
  'settings.title': 'Ajustes', 'settings.audio': 'Áudio', 'settings.playback': 'Reprodução',
  'settings.appearance': 'Aparência', 'settings.language': 'Idioma', 'settings.library': 'Biblioteca',
  'settings.lyrics': 'Letras', 'settings.sources': 'Fontes', 'settings.spotify': 'Importar Spotify',
  'settings.about': 'Sobre', 'settings.downloads': 'Downloads',
  'audio.streamingQuality': 'Qualidade do streaming', 'audio.downloads': 'Downloads',
  'audio.atmos': 'Dolby Atmos', 'audio.outputPrecision': 'Precisão de saída',
  'audio.outputAuto': 'Padrão do dispositivo', 'audio.output16': 'Inteiro de 16 bits',
  'audio.output32': 'Ponto flutuante de 32 bits', 'audio.negotiated': 'Formato ativo', 'audio.device': 'Dispositivo',
  'audio.atmosUnavailable': 'Indisponível',
  'playback.repeat': 'Repetir', 'playback.shuffle': 'Aleatório', 'playback.autoplay': 'Reprodução automática',
  'playback.gapless': 'Reprodução sem intervalos', 'playback.crossfade': 'Fade / Auto Mix',
  'playback.crossfadeOff': 'Desligado', 'playback.loudness': 'Normalização de volume',
  'playback.skipSilence': 'Pular silêncio', 'playback.equalizer': 'Equalizador',
  'playback.eqPreset': 'Predefinição', 'playback.eqReset': 'Redefinir para plano', 'playback.nerdStats': 'Dados técnicos',
  'language.appLanguage': 'Idioma do app',
  'about.version': 'Versão',
  'downloads.itemCount.one': '{count} item', 'downloads.itemCount.other': '{count} itens',
  'downloads.completedAt': 'Concluído em {date}',
  'downloads.chooseAction': 'Escolher…', 'downloads.supported': 'Compatível', 'downloads.activeCount': '{count} baixando…',
  'downloads.emptyHint': 'Use o menu de uma faixa (“Baixar”) em faixas do Subsonic ou arquivos locais.',
  'playback.crossfadeCurve': 'Curva de fade', 'playback.crossfadeCurveHint': 'Linear varia uniformemente; potência igual mantém o nível somado estável durante a sobreposição',
  'playback.curveLinear': 'Linear', 'playback.curveEqualPower': 'Potência igual',
};

const ja: Dict = {
  'nav.home': 'ホーム', 'nav.explore': '探す', 'nav.library': 'ライブラリ', 'nav.settings': '設定',
  'nav.search': '検索', 'nav.radio': 'ラジオ', 'nav.recent': '最近追加した項目', 'nav.artists': 'アーティスト',
  'nav.albums': 'アルバム', 'nav.songs': '曲', 'nav.librarySection': 'ライブラリ',
  'nav.playlists': 'プレイリスト', 'nav.allPlaylists': 'すべてのプレイリスト',
  'common.play': '再生', 'common.pause': '一時停止', 'common.next': '次へ', 'common.previous': '前へ',
  'common.save': '保存', 'common.cancel': 'キャンセル', 'common.remove': '削除', 'common.done': '完了',
  'settings.title': '設定', 'settings.audio': 'オーディオ', 'settings.playback': '再生',
  'settings.appearance': '外観', 'settings.language': '言語', 'settings.library': 'ライブラリ',
  'settings.lyrics': '歌詞', 'settings.sources': 'ソース', 'settings.spotify': 'Spotify インポート',
  'settings.about': 'このアプリについて', 'settings.downloads': 'ダウンロード',
  'audio.streamingQuality': 'ストリーミング品質', 'audio.downloads': 'ダウンロード',
  'audio.atmos': 'Dolby Atmos', 'audio.outputPrecision': '出力精度',
  'audio.outputAuto': 'デバイスの既定', 'audio.output16': '16 ビット整数',
  'audio.output32': '32 ビット浮動小数点', 'audio.negotiated': '使用中の形式', 'audio.device': 'デバイス',
  'audio.atmosUnavailable': '利用不可',
  'playback.repeat': 'リピート', 'playback.shuffle': 'シャッフル', 'playback.autoplay': '自動再生',
  'playback.gapless': 'ギャップレス再生', 'playback.crossfade': 'クロスフェード / オートミックス',
  'playback.crossfadeOff': 'オフ', 'playback.loudness': 'ラウドネス正規化',
  'playback.skipSilence': '無音をスキップ', 'playback.equalizer': 'イコライザー',
  'playback.eqPreset': 'プリセット', 'playback.eqReset': 'フラットに戻す', 'playback.nerdStats': '技術情報',
  'language.appLanguage': 'アプリの言語',
  'about.version': 'バージョン',
  'downloads.itemCount.one': '{count} 件', 'downloads.itemCount.other': '{count} 件',
  'downloads.completedAt': '{date} に完了',
  'downloads.chooseAction': '選択…', 'downloads.supported': '対応', 'downloads.activeCount': '{count} 件をダウンロード中…',
  'downloads.emptyHint': 'Subsonic の曲やローカルファイルで曲メニューの「ダウンロード」を使ってください。',
  'playback.crossfadeCurve': 'クロスフェード曲線', 'playback.crossfadeCurveHint': 'リニアは均等に変化し、等パワーは重なりの間も合計レベルを一定に保ちます',
  'playback.curveLinear': 'リニア', 'playback.curveEqualPower': '等パワー',
};

const ko: Dict = {
  'nav.home': '홈', 'nav.explore': '탐색', 'nav.library': '보관함', 'nav.settings': '설정',
  'nav.search': '검색', 'nav.radio': '라디오', 'nav.recent': '최근 추가됨', 'nav.artists': '아티스트',
  'nav.albums': '앨범', 'nav.songs': '곡', 'nav.librarySection': '보관함',
  'nav.playlists': '재생목록', 'nav.allPlaylists': '모든 재생목록',
  'common.play': '재생', 'common.pause': '일시정지', 'common.next': '다음', 'common.previous': '이전',
  'common.save': '저장', 'common.cancel': '취소', 'common.remove': '제거', 'common.done': '완료',
  'settings.title': '설정', 'settings.audio': '오디오', 'settings.playback': '재생',
  'settings.appearance': '모양', 'settings.language': '언어', 'settings.library': '보관함',
  'settings.lyrics': '가사', 'settings.sources': '소스', 'settings.spotify': 'Spotify 가져오기',
  'settings.about': '정보', 'settings.downloads': '다운로드',
  'audio.streamingQuality': '스트리밍 품질', 'audio.downloads': '다운로드',
  'audio.atmos': 'Dolby Atmos', 'audio.outputPrecision': '출력 정밀도',
  'audio.outputAuto': '기기 기본값', 'audio.output16': '16비트 정수',
  'audio.output32': '32비트 부동소수점', 'audio.negotiated': '활성 형식', 'audio.device': '기기',
  'audio.atmosUnavailable': '사용할 수 없음',
  'playback.repeat': '반복', 'playback.shuffle': '셔플', 'playback.autoplay': '자동 재생',
  'playback.gapless': '끊김 없는 재생', 'playback.crossfade': '크로스페이드 / 오토 믹스',
  'playback.crossfadeOff': '끔', 'playback.loudness': '음량 정규화',
  'playback.skipSilence': '무음 건너뛰기', 'playback.equalizer': '이퀄라이저',
  'playback.eqPreset': '프리셋', 'playback.eqReset': '플랫으로 초기화', 'playback.nerdStats': '기술 정보',
  'language.appLanguage': '앱 언어',
  'about.version': '버전',
  'downloads.itemCount.one': '{count}개 항목', 'downloads.itemCount.other': '{count}개 항목',
  'downloads.completedAt': '{date}에 완료',
  'downloads.chooseAction': '선택…', 'downloads.supported': '지원됨', 'downloads.activeCount': '{count}개 다운로드 중…',
  'downloads.emptyHint': 'Subsonic 트랙이나 로컬 파일에서 트랙 메뉴의 “다운로드”를 사용하세요.',
  'playback.crossfadeCurve': '크로스페이드 곡선', 'playback.crossfadeCurveHint': '선형은 균등하게 변하고, 등출력은 겹치는 동안 합산 레벨을 일정하게 유지합니다',
  'playback.curveLinear': '선형', 'playback.curveEqualPower': '등출력',
};

const zh: Dict = {
  'nav.home': '首页', 'nav.explore': '探索', 'nav.library': '资料库', 'nav.settings': '设置',
  'nav.search': '搜索', 'nav.radio': '电台', 'nav.recent': '最近添加', 'nav.artists': '艺人',
  'nav.albums': '专辑', 'nav.songs': '歌曲', 'nav.librarySection': '资料库',
  'nav.playlists': '播放列表', 'nav.allPlaylists': '所有播放列表',
  'common.play': '播放', 'common.pause': '暂停', 'common.next': '下一首', 'common.previous': '上一首',
  'common.save': '保存', 'common.cancel': '取消', 'common.remove': '移除', 'common.done': '完成',
  'settings.title': '设置', 'settings.audio': '音频', 'settings.playback': '播放',
  'settings.appearance': '外观', 'settings.language': '语言', 'settings.library': '资料库',
  'settings.lyrics': '歌词', 'settings.sources': '来源', 'settings.spotify': '导入 Spotify',
  'settings.about': '关于', 'settings.downloads': '下载',
  'audio.streamingQuality': '流媒体音质', 'audio.downloads': '下载',
  'audio.atmos': '杜比全景声', 'audio.outputPrecision': '输出精度',
  'audio.outputAuto': '设备默认', 'audio.output16': '16 位整数',
  'audio.output32': '32 位浮点', 'audio.negotiated': '当前格式', 'audio.device': '设备',
  'audio.atmosUnavailable': '不可用',
  'playback.repeat': '重复', 'playback.shuffle': '随机播放', 'playback.autoplay': '自动播放',
  'playback.gapless': '无缝播放', 'playback.crossfade': '交叉淡化 / 自动混音',
  'playback.crossfadeOff': '关闭', 'playback.loudness': '响度标准化',
  'playback.skipSilence': '跳过静音', 'playback.equalizer': '均衡器',
  'playback.eqPreset': '预设', 'playback.eqReset': '恢复为平直', 'playback.nerdStats': '技术信息',
  'language.appLanguage': '应用语言',
  'about.version': '版本',
  'downloads.itemCount.one': '{count} 项', 'downloads.itemCount.other': '{count} 项',
  'downloads.completedAt': '完成于 {date}',
  'downloads.chooseAction': '选择…', 'downloads.supported': '支持', 'downloads.activeCount': '正在下载 {count} 项…',
  'downloads.emptyHint': '在 Subsonic 曲目或本地文件上使用曲目菜单中的“下载”。',
  'playback.crossfadeCurve': '交叉淡化曲线', 'playback.crossfadeCurveHint': '线性均匀变化；等功率在重叠期间保持总和电平稳定',
  'playback.curveLinear': '线性', 'playback.curveEqualPower': '等功率',
};

const ar: Dict = {
  'nav.home': 'الرئيسية', 'nav.explore': 'استكشاف', 'nav.library': 'المكتبة', 'nav.settings': 'الإعدادات',
  'nav.search': 'بحث', 'nav.radio': 'راديو', 'nav.recent': 'أضيف حديثًا', 'nav.artists': 'الفنانون',
  'nav.albums': 'الألبومات', 'nav.songs': 'الأغاني', 'nav.librarySection': 'المكتبة',
  'nav.playlists': 'قوائم التشغيل', 'nav.allPlaylists': 'كل قوائم التشغيل',
  'common.play': 'تشغيل', 'common.pause': 'إيقاف مؤقت', 'common.next': 'التالي', 'common.previous': 'السابق',
  'common.save': 'حفظ', 'common.cancel': 'إلغاء', 'common.remove': 'إزالة', 'common.done': 'تم',
  'settings.title': 'الإعدادات', 'settings.audio': 'الصوت', 'settings.playback': 'التشغيل',
  'settings.appearance': 'المظهر', 'settings.language': 'اللغة', 'settings.library': 'المكتبة',
  'settings.lyrics': 'الكلمات', 'settings.sources': 'المصادر', 'settings.spotify': 'استيراد Spotify',
  'settings.about': 'حول', 'settings.downloads': 'التنزيلات',
  'audio.streamingQuality': 'جودة البث', 'audio.downloads': 'التنزيلات',
  'audio.atmos': 'Dolby Atmos', 'audio.outputPrecision': 'دقة الإخراج',
  'audio.outputAuto': 'افتراضي الجهاز', 'audio.output16': 'عدد صحيح 16 بت',
  'audio.output32': 'عائم 32 بت', 'audio.negotiated': 'التنسيق النشط', 'audio.device': 'الجهاز',
  'audio.atmosUnavailable': 'غير متاح',
  'playback.repeat': 'تكرار', 'playback.shuffle': 'عشوائي', 'playback.autoplay': 'تشغيل تلقائي',
  'playback.gapless': 'تشغيل بدون فجوات', 'playback.crossfade': 'تلاشي متقاطع / مزج تلقائي',
  'playback.crossfadeOff': 'إيقاف', 'playback.loudness': 'تطبيع مستوى الصوت',
  'playback.skipSilence': 'تخطي الصمت', 'playback.equalizer': 'معادل الصوت',
  'playback.eqPreset': 'إعداد مسبق', 'playback.eqReset': 'إعادة إلى مسطح', 'playback.nerdStats': 'معلومات تقنية',
  'language.appLanguage': 'لغة التطبيق',
  'about.version': 'الإصدار',
  'downloads.itemCount.zero': 'لا عناصر', 'downloads.itemCount.one': 'عنصر واحد',
  'downloads.itemCount.two': 'عنصران', 'downloads.itemCount.few': '{count} عناصر',
  'downloads.itemCount.many': '{count} عنصرًا', 'downloads.itemCount.other': '{count} عنصر',
  'downloads.completedAt': 'اكتمل في {date}',
  'downloads.chooseAction': 'اختيار…', 'downloads.supported': 'مدعوم', 'downloads.activeCount': 'جارٍ تنزيل {count}…',
  'downloads.emptyHint': 'استخدم قائمة المقطع (“تنزيل”) في مقاطع Subsonic أو الملفات المحلية.',
  'playback.crossfadeCurve': 'منحنى التلاشي المتقاطع', 'playback.crossfadeCurveHint': 'الخطي يتغير بالتساوي؛ متساوي القدرة يحافظ على ثبات المستوى الكلي أثناء التداخل',
  'playback.curveLinear': 'خطي', 'playback.curveEqualPower': 'متساوي القدرة',
};

const ru: Dict = {
  'nav.home': 'Главная', 'nav.explore': 'Обзор', 'nav.library': 'Медиатека', 'nav.settings': 'Настройки',
  'nav.search': 'Поиск', 'nav.radio': 'Радио', 'nav.recent': 'Недавно добавленные', 'nav.artists': 'Исполнители',
  'nav.albums': 'Альбомы', 'nav.songs': 'Треки', 'nav.librarySection': 'Медиатека',
  'nav.playlists': 'Плейлисты', 'nav.allPlaylists': 'Все плейлисты',
  'common.play': 'Играть', 'common.pause': 'Пауза', 'common.next': 'Следующий', 'common.previous': 'Предыдущий',
  'common.save': 'Сохранить', 'common.cancel': 'Отмена', 'common.remove': 'Удалить', 'common.done': 'Готово',
  'settings.title': 'Настройки', 'settings.audio': 'Аудио', 'settings.playback': 'Воспроизведение',
  'settings.appearance': 'Оформление', 'settings.language': 'Язык', 'settings.library': 'Медиатека',
  'settings.lyrics': 'Тексты', 'settings.sources': 'Источники', 'settings.spotify': 'Импорт Spotify',
  'settings.about': 'О программе', 'settings.downloads': 'Загрузки',
  'audio.streamingQuality': 'Качество потока', 'audio.downloads': 'Загрузки',
  'audio.atmos': 'Dolby Atmos', 'audio.outputPrecision': 'Точность вывода',
  'audio.outputAuto': 'По умолчанию устройства', 'audio.output16': '16-битное целое',
  'audio.output32': '32-битное с плавающей точкой', 'audio.negotiated': 'Активный формат', 'audio.device': 'Устройство',
  'audio.atmosUnavailable': 'Недоступно',
  'playback.repeat': 'Повтор', 'playback.shuffle': 'Перемешать', 'playback.autoplay': 'Автовоспроизведение',
  'playback.gapless': 'Без пауз', 'playback.crossfade': 'Кроссфейд / Auto Mix',
  'playback.crossfadeOff': 'Выкл.', 'playback.loudness': 'Нормализация громкости',
  'playback.skipSilence': 'Пропускать тишину', 'playback.equalizer': 'Эквалайзер',
  'playback.eqPreset': 'Пресет', 'playback.eqReset': 'Сбросить на плоский', 'playback.nerdStats': 'Технические данные',
  'language.appLanguage': 'Язык приложения',
  'about.version': 'Версия',
  'downloads.itemCount.one': '{count} элемент', 'downloads.itemCount.few': '{count} элемента',
  'downloads.itemCount.many': '{count} элементов', 'downloads.itemCount.other': '{count} элемента',
  'downloads.completedAt': 'Завершено {date}',
  'downloads.chooseAction': 'Выбрать…', 'downloads.supported': 'Поддерживается', 'downloads.activeCount': 'Загружается: {count}…',
  'downloads.emptyHint': 'Используйте меню трека («Загрузить») для треков Subsonic или локальных файлов.',
  'playback.crossfadeCurve': 'Кривая кроссфейда', 'playback.crossfadeCurveHint': 'Линейная меняется равномерно; равная мощность сохраняет суммарный уровень во время перекрытия',
  'playback.curveLinear': 'Линейная', 'playback.curveEqualPower': 'Равная мощность',
};

const DICTS: Record<Locale, Dict> = { en, hi, es, fr, de, pt, ja, ko, zh, ar, ru };

const LOCALE_KEY = 'tansen:locale';

function isLocale(value: string | null): value is Locale {
  return !!value && LOCALES.some((l) => l.id === value);
}

export function loadLocale(): Locale {
  try {
    const saved = localStorage.getItem(LOCALE_KEY);
    if (isLocale(saved)) return saved;
    // OS hint: first matching primary language, else English.
    const primary = navigator.language?.split('-')[0];
    return LOCALES.find((l) => l.id === primary)?.id ?? 'en';
  } catch {
    return 'en';
  }
}

export function localeDir(locale: Locale): 'ltr' | 'rtl' {
  return LOCALES.find((l) => l.id === locale)?.dir ?? 'ltr';
}

function applyDocumentLocale(locale: Locale) {
  try {
    document.documentElement.lang = locale;
    document.documentElement.dir = localeDir(locale);
  } catch {
    // non-browser test environment
  }
}

interface LocaleState {
  locale: Locale;
  setLocale: (locale: Locale) => void;
}

export const useLocale = create<LocaleState>((set) => ({
  locale: loadLocale(),
  setLocale: (locale) => {
    applyDocumentLocale(locale);
    try {
      localStorage.setItem(LOCALE_KEY, locale);
    } catch {
      // private mode: the switch still works for this session
    }
    set({ locale });
  },
}));

applyDocumentLocale(useLocale.getState().locale);

export interface Translator {
  (key: MessageKey, vars?: Record<string, string | number>): string;
  plural: (key: MessageKey, count: number, vars?: Record<string, string | number>) => string;
  number: (value: number) => string;
  date: (value: Date | number, options?: Intl.DateTimeFormatOptions) => string;
}

export function translator(locale: Locale): Translator {
  const dict = DICTS[locale] ?? {};
  const t = ((key: MessageKey, vars?: Record<string, string | number>) => {
    const template = dict[key] ?? en[key] ?? key;
    if (!vars) return template;
    return template.replace(/\{(\w+)\}/g, (whole, name: string) =>
      name in vars ? String(vars[name]) : whole,
    );
  }) as Translator;
  t.plural = (key, count, vars) => {
    const category = new Intl.PluralRules(locale).select(count);
    const pluralKey = `${key}.${category}` as MessageKey;
    // Locales have different CLDR categories (Arabic zero/two/few/many,
    // Russian few/many). Fall back to `.other` before the bare key so a
    // missing category never leaks a key name into the UI.
    const otherKey = `${key}.other` as MessageKey;
    const template = dict[pluralKey] ?? en[pluralKey] ?? dict[otherKey] ?? en[otherKey];
    if (template) {
      const chosen = dict[pluralKey] ?? en[pluralKey] ? pluralKey : otherKey;
      return t(chosen, { count: t.number(count), ...vars });
    }
    return t(key, { count: t.number(count), ...vars });
  };
  t.number = (value) => new Intl.NumberFormat(locale).format(value);
  t.date = (value, options) =>
    new Intl.DateTimeFormat(locale, options ?? { dateStyle: 'medium' }).format(
      typeof value === 'number' ? new Date(value) : value,
    );
  return t;
}

/** Subscribe to the active locale; components re-render on switch. */
export function useT(): Translator {
  const locale = useLocale((s) => s.locale);
  return useMemo(() => translator(locale), [locale]);
}
