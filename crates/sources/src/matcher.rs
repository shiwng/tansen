//! Pure matching between a target track's metadata and a candidate
//! found on another source. No I/O, no dependencies beyond std — only
//! string normalization + equality + a duration tolerance. Used by the
//! resolver's fallback ladder.

/// Metadata a fallback candidate must match against.
#[derive(Debug, Clone, PartialEq)]
pub struct TrackMeta {
    pub title: String,
    pub artist: String,
    pub duration_sec: Option<f64>,
}

/// Duration tolerance for a match, in seconds (named const).
pub const DURATION_TOLERANCE_SEC: f64 = 10.0;

/// Marker words whose presence must agree between target and candidate:
/// a live/remix/remaster/acoustic take is NOT the same track as the
/// studio original, so marker differences reject the match.
const MARKER_WORDS: [(&str, u8); 4] = [("live", 1), ("remix", 2), ("remaster", 4), ("acoustic", 8)];

/// Fold one common accented character to its ASCII base (std only; the
/// common Latin-1 / Latin Extended-A set).
fn fold_diacritic(c: char) -> char {
    match c {
        'à' | 'á' | 'â' | 'ã' | 'ä' | 'å' | 'ā' | 'ă' | 'ą' => 'a',
        'ç' | 'ć' | 'ĉ' | 'ċ' | 'č' => 'c',
        'è' | 'é' | 'ê' | 'ë' | 'ē' | 'ĕ' | 'ė' | 'ę' | 'ě' => 'e',
        'ğ' | 'ĝ' | 'ġ' | 'ģ' => 'g',
        'ì' | 'í' | 'î' | 'ï' | 'ī' | 'ĭ' | 'į' | 'ı' => 'i',
        'ĵ' => 'j',
        'ķ' => 'k',
        'ĺ' | 'ļ' | 'ľ' | 'ŀ' | 'ł' => 'l',
        'ñ' | 'ń' | 'ņ' | 'ň' | 'ŉ' | 'ŋ' => 'n',
        'ò' | 'ó' | 'ô' | 'õ' | 'ö' | 'ø' | 'ō' | 'ŏ' | 'ő' => 'o',
        'ŕ' | 'ŗ' | 'ř' => 'r',
        'ś' | 'ŝ' | 'ş' | 'š' => 's',
        'ţ' | 'ť' | 'ŧ' => 't',
        'ù' | 'ú' | 'û' | 'ü' | 'ū' | 'ŭ' | 'ů' | 'ű' | 'ų' => 'u',
        'ŵ' => 'w',
        'ý' | 'ÿ' | 'ŷ' => 'y',
        'ź' | 'ż' | 'ž' => 'z',
        'æ' => 'a',
        'œ' => 'o',
        'ß' => 's',
        'ð' => 'd',
        'þ' => 't',
        c => c,
    }
}

/// Strip feat/ft credit phrases: parenthetical groups and bare tokens
/// ("(feat. X)", "feat. X", "ft. X", "featuring …"). The cut happens on
/// the first feat token, so "Song feat. A & B" → "Song".
fn strip_feat(s: &str) -> String {
    // Parenthetical groups whose inner text starts with feat/ft.
    let mut out = String::with_capacity(s.len());
    let chars: Vec<char> = s.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '(' {
            if let Some(close) = chars[i + 1..].iter().position(|&c| c == ')') {
                let inner: String = chars[i + 1..i + 1 + close].iter().collect();
                let inner = inner.trim();
                if inner.starts_with("feat") || inner.starts_with("ft") {
                    i += close + 2;
                    continue;
                }
            }
        }
        out.push(chars[i]);
        i += 1;
    }
    // Bare feat token: cut everything from it on.
    let tokens: Vec<&str> = out.split_whitespace().collect();
    match tokens.iter().position(|t| {
        t.eq_ignore_ascii_case("feat")
            || t.eq_ignore_ascii_case("ft")
            || t.eq_ignore_ascii_case("featuring")
            || t.to_ascii_lowercase().starts_with("feat.")
            || t.to_ascii_lowercase().starts_with("ft.")
    }) {
        Some(i) => tokens[..i].join(" "),
        None => tokens.join(" "),
    }
}

/// Normalize for comparison: lowercase, fold diacritics, drop punctuation
/// (keep letters/digits), collapse whitespace, strip feat credits.
pub fn normalize(s: &str) -> String {
    let lower = s.to_lowercase();
    let stripped = strip_feat(&lower);
    let mut out = String::with_capacity(stripped.len());
    let mut prev_space = true;
    for c in stripped.chars() {
        let c = fold_diacritic(c);
        if c.is_alphanumeric() {
            out.push(c);
            prev_space = false;
        } else if !prev_space {
            out.push(' ');
            prev_space = true;
        }
    }
    out.trim().to_string()
}

/// The marker mask of a title: which of live/remix/remaster/acoustic it
/// carries. Token-based (word boundaries), so "deliver" ≠ "live".
fn marker_mask(title: &str) -> u8 {
    let stripped = strip_feat(&title.to_lowercase());
    let mut mask = 0u8;
    for token in stripped.split(|c: char| !c.is_alphanumeric()) {
        if token.is_empty() {
            continue;
        }
        for (word, bit) in MARKER_WORDS {
            if token == word
                || (word == "remix" && token.starts_with("remix"))
                || (word == "remaster" && token.starts_with("remaster"))
            {
                mask |= bit;
            }
        }
    }
    mask
}

/// Whether a candidate is plausibly the same recording as the target:
/// normalized title and artist equal, marker sets equal, and durations
/// within [`DURATION_TOLERANCE_SEC`] — both must be known. A candidate
/// that omits its duration used to pass unconditionally, which let a
/// fallback-ladder server echo the search query back and win the match
/// without ever risking the duration check; identity is now fail-closed.
pub fn matches(target: &TrackMeta, candidate: &TrackMeta) -> bool {
    if normalize(&target.title) != normalize(&candidate.title) {
        return false;
    }
    if normalize(&target.artist) != normalize(&candidate.artist) {
        return false;
    }
    if marker_mask(&target.title) != marker_mask(&candidate.title) {
        return false;
    }
    match (target.duration_sec, candidate.duration_sec) {
        (Some(a), Some(b)) => (a - b).abs() <= DURATION_TOLERANCE_SEC,
        _ => false,
    }
}

/// Search queries for finding the target on another source: best first.
/// "title artist", then "title" (both normalized).
pub fn queries(meta: &TrackMeta) -> Vec<String> {
    let title = normalize(&meta.title);
    let artist = normalize(&meta.artist);
    let mut out = Vec::new();
    if title.is_empty() {
        return out;
    }
    if !artist.is_empty() {
        out.push(format!("{title} {artist}"));
    }
    out.push(title);
    out
}

#[cfg(test)]
#[allow(clippy::unwrap_used, reason = "test code")]
mod tests {
    use super::*;

    fn meta(title: &str, artist: &str, dur: Option<f64>) -> TrackMeta {
        TrackMeta {
            title: title.into(),
            artist: artist.into(),
            duration_sec: dur,
        }
    }

    /// Regression (resolver-ladder-metadata-capture): a
    /// fallback-ladder server that echoes the leaked "title artist" query
    /// used to win the match with duration omitted, because the `_ =>`
    /// arm passed every unknown-duration pair. Identity is fail-closed:
    /// the duration must be known on both sides and agree.
    #[test]
    fn duration_must_be_known_on_both_sides() {
        assert!(
            !matches(
                &meta("Song", "Artist", Some(200.0)),
                &meta("Song", "Artist", None)
            ),
            "candidate without duration must not match"
        );
        assert!(
            !matches(
                &meta("Song", "Artist", None),
                &meta("Song", "Artist", Some(200.0))
            ),
            "target without duration must not match"
        );
        assert!(
            !matches(&meta("Song", "Artist", None), &meta("Song", "Artist", None)),
            "two unknown durations must not match"
        );
        // Known durations within tolerance still match; echoes with a
        // guessed duration are rejected by the tolerance check itself.
        assert!(matches(
            &meta("Song", "Artist", Some(200.0)),
            &meta("Song", "Artist", Some(201.0))
        ));
        assert!(!matches(
            &meta("Song", "Artist", Some(200.0)),
            &meta("Song", "Artist", Some(230.0))
        ));
    }

    #[test]
    fn normalization_folds_case_punctuation_diacritics_and_feat() {
        assert_eq!(normalize("Café"), "cafe");
        assert_eq!(
            normalize("U2 – With or Without You"),
            "u2 with or without you"
        );
        assert_eq!(normalize("Song (feat. Drake)"), "song");
        assert_eq!(normalize("Song ft. Drake & Future"), "song");
        assert_eq!(normalize("Song (live)"), "song live");
        assert_eq!(normalize("  Multi   Spaces  "), "multi spaces");
    }

    #[test]
    fn exact_and_normalized_matches_pass() {
        let t = meta("Song (feat. Drake)", "Artist", Some(200.0));
        let c = meta("Song", "Artist", Some(200.0));
        assert!(matches(&t, &c));
        let t = meta("Café", "Mø", Some(180.0));
        let c = meta("cafe", "mo", Some(182.0));
        assert!(matches(&t, &c));
    }

    #[test]
    fn duration_within_tolerance_passes_outside_rejects() {
        let t = meta("Song", "Artist", Some(200.0));
        assert!(matches(&t, &meta("Song", "Artist", Some(209.0))));
        assert!(matches(&t, &meta("Song", "Artist", Some(191.0))));
        assert!(!matches(&t, &meta("Song", "Artist", Some(211.0))));
        assert!(!matches(&t, &meta("Song", "Artist", Some(189.0))));
        // Unknown duration now rejects (fail-closed identity): see
        // duration_must_be_known_on_both_sides.
        assert!(!matches(&t, &meta("Song", "Artist", None)));
    }

    #[test]
    fn near_miss_false_positives_are_rejected() {
        let studio = meta("Song", "Artist", Some(200.0));
        assert!(
            !matches(&studio, &meta("Song (live)", "Artist", Some(200.0))),
            "live vs studio"
        );
        assert!(!matches(
            &studio,
            &meta("Song (Live At Wembley)", "Artist", Some(200.0))
        ));
        assert!(
            !matches(&studio, &meta("Song (remix)", "Artist", Some(200.0))),
            "remix"
        );
        assert!(
            !matches(&studio, &meta("Song (remastered)", "Artist", Some(200.0))),
            "remaster"
        );
        assert!(
            !matches(&studio, &meta("Song (acoustic)", "Artist", Some(200.0))),
            "acoustic"
        );
        assert!(
            !matches(&studio, &meta("Song", "Other Artist", Some(200.0))),
            "wrong artist"
        );
        assert!(
            !matches(&studio, &meta("Other Song", "Artist", Some(200.0))),
            "wrong title"
        );
    }

    #[test]
    fn marker_words_need_word_boundaries() {
        // "deliver" contains "live" but is not a live marker.
        assert_eq!(marker_mask("deliver"), 0);
        assert_eq!(marker_mask("Live in Tokyo"), 1);
        assert_eq!(marker_mask("Song (Remix)"), 2);
        assert_eq!(marker_mask("Remastered 2024"), 4);
        assert_eq!(marker_mask("Acoustic Session"), 8);
        assert_eq!(marker_mask("Live Remix"), 1 | 2);
    }

    #[test]
    fn queries_return_title_artist_then_title_normalized() {
        let m = meta("Café (feat. X)", "Mø", Some(100.0));
        let q = queries(&m);
        assert_eq!(q, vec!["cafe mo".to_string(), "cafe".to_string()]);
        let no_artist = queries(&meta("Song", "", None));
        assert_eq!(no_artist, vec!["song".to_string()]);
        assert!(queries(&meta("", "", None)).is_empty());
    }
}
