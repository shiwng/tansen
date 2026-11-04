//! Metadata extraction via lofty. One function, one job: file → `Track`.

use lofty::prelude::*;
use persistence::models::Track;
use std::path::Path;

#[derive(Debug, thiserror::Error)]
pub enum MetaError {
    #[error("lofty: {0}")]
    Lofty(#[from] lofty::error::LoftyError),
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
}

pub const SUPPORTED_EXT: &[&str] = &[
    "mp3", "flac", "m4a", "mp4", "aac", "ogg", "oga", "opus", "wav", "aiff", "aif", "wma", "ape",
    "wv", "alac",
];

pub fn is_supported(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|e| SUPPORTED_EXT.contains(&e.to_ascii_lowercase().as_str()))
        .unwrap_or(false)
}

/// Reads tags and audio properties. Falls back to filename-derived title when
/// tags are absent — an untagged file should still appear in the library.
pub fn read(path: &Path, folder_id: i64) -> Result<Track, MetaError> {
    let file = lofty::read_from_path(path)?;
    let props = file.properties();
    let tag = file.primary_tag().or_else(|| file.first_tag());

    let file_name = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or_default()
        .to_string();
    let stem = path
        .file_stem()
        .and_then(|n| n.to_str())
        .unwrap_or(&file_name)
        .to_string();

    let get = |key: &lofty::tag::ItemKey| -> String {
        tag.and_then(|t| t.get_string(key).map(|s| s.trim().to_string()))
            .unwrap_or_default()
    };

    let title = {
        let t = get(&lofty::tag::ItemKey::TrackTitle);
        if t.is_empty() { stem } else { t }
    };

    let codec = format_codec(path, Some(file.file_type()));

    Ok(Track {
        id: 0,
        folder_id,
        path: path.to_string_lossy().into_owned(),
        file_name,
        title,
        artist: get(&lofty::tag::ItemKey::TrackArtist),
        album: get(&lofty::tag::ItemKey::AlbumTitle),
        album_artist: get(&lofty::tag::ItemKey::AlbumArtist),
        track_no: tag.and_then(|t| t.track()).map(|v| v as i64),
        disc_no: tag.and_then(|t| t.disk()).map(|v| v as i64),
        year: tag.and_then(|t| t.year()).map(|v| v as i64),
        genre: get(&lofty::tag::ItemKey::Genre),
        duration_sec: props.duration().as_secs_f64(),
        codec,
        bitrate_kbps: props
            .audio_bitrate()
            .or(props.overall_bitrate())
            .filter(|v| *v > 0)
            .map(|v| v as i64),
        sample_rate: props.sample_rate().filter(|v| *v > 0).map(|v| v as i64),
        bit_depth: props.bit_depth().filter(|v| *v > 0).map(|v| v as i64),
        channels: props.channels().filter(|v| *v > 0).map(|v| v as i64),
        art_path: None,
        missing: false,
    })
}

fn format_codec(path: &Path, ft: Option<lofty::file::FileType>) -> String {
    let from_ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_uppercase();
    match ft {
        Some(lofty::file::FileType::Mpeg) => "MP3".into(),
        Some(lofty::file::FileType::Flac) => "FLAC".into(),
        Some(lofty::file::FileType::Mp4) => {
            // F2 fix: ALAC lives in MP4 containers next to AAC, and the two
            // classify oppositely on the lossless table. The generic probe
            // carries no codec, so read the file's own audio codec (one
            // extra open per MP4-family file, scan-time only).
            mp4_codec(path).unwrap_or_else(|| "MP4".into())
        }
        Some(lofty::file::FileType::Vorbis) => "OGG".into(),
        Some(lofty::file::FileType::Opus) => "OPUS".into(),
        Some(lofty::file::FileType::Wav) => "WAV".into(),
        Some(lofty::file::FileType::Aiff) => "AIFF".into(),
        Some(lofty::file::FileType::WavPack) => "WV".into(),
        Some(lofty::file::FileType::Ape) => "APE".into(),
        Some(lofty::file::FileType::Aac) => "AAC".into(),
        _ => from_ext,
    }
}


/// F2: the audio codec of an MP4-family file, from lofty's Mp4File probe.
fn mp4_codec(path: &Path) -> Option<String> {
    use lofty::mp4::Mp4Codec;
    let file = lofty::mp4::Mp4File::read_from(&mut std::fs::File::open(path).ok()?, lofty::config::ParseOptions::new()).ok()?;
    let codec = match file.properties().codec() {
        Mp4Codec::ALAC => "ALAC",
        Mp4Codec::AAC => "AAC",
        _ => "MP4",
    };
    Some(codec.to_string())
}
