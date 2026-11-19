//! Decode probing: turn audio bytes into an RMS figure without an audio
//! device. Used by the sidecar round-trip test ("a resolved stream actually
//! contains sound") and by future format checks.

use std::io::{Read, Seek};
use std::time::Duration;

#[derive(Debug, thiserror::Error)]
pub enum ProbeError {
    #[error("symphonia: {0}")]
    Symphonia(String),
    #[error("no audio track in stream")]
    NoAudioTrack,
    #[error("not enough audio to decode {0:?}")]
    TooShort(Duration),
}

/// Decodes up to `window` of audio and returns its RMS level (0.0 = digital
/// silence). Decodes real samples — no device, no rodio.
pub fn decode_rms(
    reader: impl Read + Seek + Send + Sync + 'static,
    window: Duration,
) -> Result<f32, ProbeError> {
    use symphonia::core::audio::SampleBuffer;
    use symphonia::core::codecs::DecoderOptions;
    use symphonia::core::formats::FormatOptions;
    use symphonia::core::io::MediaSourceStream;
    use symphonia::core::meta::MetadataOptions;
    use symphonia::core::probe::Hint;

    let mss = MediaSourceStream::new(Box::new(ProbeSource::new(reader)), Default::default());
    let mut hint = Hint::new();
    // AAC-in-MP4 is what the sidecar's `prefer=aac` answers with; a real
    // extension helps the prober when the container is bare ADTS.
    hint.with_extension("m4a");

    let probed = symphonia::default::get_probe()
        .format(
            &hint,
            mss,
            &FormatOptions::default(),
            &MetadataOptions::default(),
        )
        .map_err(|e| ProbeError::Symphonia(e.to_string()))?;
    let mut format = probed.format;

    // These containers carry one audio track; take the first non-null one.
    let track_index = format
        .tracks()
        .iter()
        .position(|t| t.codec_params.codec != symphonia::core::codecs::CODEC_TYPE_NULL)
        .ok_or(ProbeError::NoAudioTrack)?;
    let track_rate = format.tracks()[track_index]
        .codec_params
        .sample_rate
        .ok_or(ProbeError::NoAudioTrack)?;
    let mut decoder = symphonia::default::get_codecs()
        .make(
            &format.tracks()[track_index].codec_params,
            &DecoderOptions::default(),
        )
        .map_err(|e| ProbeError::Symphonia(e.to_string()))?;

    let frames_target = (track_rate as f64 * window.as_secs_f64()) as u64;
    let mut frames = 0u64;
    let mut sum_squares = 0.0f64;

    loop {
        let packet = match format.next_packet() {
            Ok(p) => p,
            Err(symphonia::core::errors::Error::IoError(ref e))
                if e.kind() == std::io::ErrorKind::UnexpectedEof =>
            {
                break; // clean end of stream before the window filled
            }
            Err(symphonia::core::errors::Error::ResetRequired) => break,
            Err(e) => return Err(ProbeError::Symphonia(e.to_string())),
        };
        match decoder.decode(&packet) {
            Ok(decoded) => {
                let mut samples =
                    SampleBuffer::<f32>::new(decoded.capacity() as u64, *decoded.spec());
                samples.copy_interleaved_ref(decoded);
                for sample in samples.samples() {
                    let s = *sample;
                    sum_squares += (s as f64) * (s as f64);
                    frames += 1;
                }
                if frames >= frames_target {
                    break;
                }
            }
            Err(symphonia::core::errors::Error::DecodeError(_)) => continue,
            Err(e) => return Err(ProbeError::Symphonia(e.to_string())),
        }
    }

    if (frames as f64) < frames_target as f64 * 0.5 {
        return Err(ProbeError::TooShort(window));
    }
    Ok((sum_squares / frames.max(1) as f64).sqrt() as f32)
}

/// Symphonia's `MediaSourceStream` wants a `MediaSource`; forward the
/// cursor's Read/Seek and report the length captured at construction (the
/// MP4 demuxer seeks against it; `&self` cannot re-derive it by seeking).
struct ProbeSource<R> {
    inner: R,
    len: u64,
}

impl<R: Read + Seek> ProbeSource<R> {
    fn new(mut inner: R) -> Self {
        let len = inner.seek(std::io::SeekFrom::End(0)).unwrap_or(0);
        let _ = inner.seek(std::io::SeekFrom::Start(0));
        Self { inner, len }
    }
}

impl<R: Read + Seek> Read for ProbeSource<R> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        self.inner.read(buf)
    }
}

impl<R: Read + Seek> Seek for ProbeSource<R> {
    fn seek(&mut self, pos: std::io::SeekFrom) -> std::io::Result<u64> {
        self.inner.seek(pos)
    }
}

impl<R: Read + Seek + Send + Sync> symphonia::core::io::MediaSource for ProbeSource<R> {
    fn is_seekable(&self) -> bool {
        true
    }

    fn byte_len(&self) -> Option<u64> {
        Some(self.len)
    }
}

#[cfg(test)]
mod codec_matrix_tests {
    use super::*;
    use crate::remote::HttpRangeReader;

    /// A synthetic WAV (the only self-generatable format without ffmpeg).
    fn wav_bytes(frames: usize) -> Vec<u8> {
        use std::io::Write;
        let mut buf = Vec::new();
        let data_len = frames * 2 * 2;
        buf.extend_from_slice(b"RIFF");
        buf.extend_from_slice(&((36 + data_len) as u32).to_le_bytes());
        buf.extend_from_slice(b"WAVEfmt ");
        buf.extend_from_slice(&16u32.to_le_bytes());
        buf.extend_from_slice(&1u16.to_le_bytes());
        buf.extend_from_slice(&2u16.to_le_bytes());
        buf.extend_from_slice(&44100u32.to_le_bytes());
        buf.extend_from_slice(&((44100u32 * 4).to_le_bytes()));
        buf.extend_from_slice(&4u16.to_le_bytes());
        buf.extend_from_slice(&16u16.to_le_bytes());
        buf.extend_from_slice(b"data");
        buf.extend_from_slice(&(data_len as u32).to_le_bytes());
        for i in 0..frames {
            let v =
                (9000.0 * (2.0 * std::f64::consts::PI * 220.0 * i as f64 / 44100.0).sin()) as i16;
            buf.extend_from_slice(&v.to_le_bytes());
            buf.extend_from_slice(&v.to_le_bytes());
        }
        let _ = buf.flush();
        buf
    }

    #[test]
    fn wav_decodes_with_sound() {
        let bytes = wav_bytes(22050); // 0.5 s
                                      // Pipe through a memory-backed range reader.
        let data: &'static [u8] = Box::leak(bytes.into_boxed_slice());
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        std::thread::spawn(move || {
            use std::io::Write;
            for mut stream in listener.incoming().flatten() {
                let mut buf = [0u8; 4096];
                let _ = std::io::Read::read(&mut &stream, &mut buf);
                let head = format!(
                    "HTTP/1.1 206 Partial Content\r\nContent-Range: bytes 0-{}/{}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    data.len() - 1,
                    data.len(),
                    data.len()
                );
                let _ = stream.write_all(head.as_bytes());
                let _ = stream.write_all(data);
            }
        });
        let reader =
            HttpRangeReader::new(format!("http://127.0.0.1:{port}/t.wav"), vec![]).unwrap();
        let rms = decode_rms(reader, std::time::Duration::from_millis(300)).unwrap();
        assert!(rms > 0.005, "WAV produced sound, RMS {rms}");
    }
}
