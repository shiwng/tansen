//! Track loudness analysis for normalization.
//!
//! Implements an EBU R128-style integrated loudness measurement: K-weighting
//! (high-pass + high-shelf), 400 ms blocks with 75 % overlap, absolute
//! (-70 LUFS) and relative (-10 LU) gating. Runs on a background blocking
//! thread — never on the audio callback and never on the engine task.
//!
//! The result is stored as a gain in dB toward a -14 LUFS target, pre-clamped
//! so the measured sample peak cannot clip. Tracks that fail to analyse stay
//! at unity gain; the pipeline never guesses.

use rodio::Source;
use std::fs::File;
use std::io::BufReader;
use std::path::Path;

/// Streaming target. -14 LUFS matches the common streaming loudness target
/// and leaves a little headroom for lossy masters.
pub const TARGET_LUFS: f32 = -14.0;
/// Peak ceiling used when deriving the gain: -0.5 dBFS.
pub const PEAK_HEADROOM_DB: f32 = -0.5;
/// Gains beyond this are refused: a very quiet recording should be left
/// alone rather than amplified into noise.
pub const MAX_GAIN_DB: f32 = 12.0;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LoudnessAnalysis {
    /// Integrated loudness in LUFS.
    pub integrated_lufs: f32,
    /// Linear sample peak (0..n).
    pub peak: f32,
    /// Normalization gain toward the target, clip-safe.
    pub gain_db: f32,
}

/// A biquad used only by the analyser (not real-time).
#[derive(Clone, Copy)]
struct Biquad {
    b0: f32,
    b1: f32,
    b2: f32,
    a1: f32,
    a2: f32,
    z1: f32,
    z2: f32,
}

impl Biquad {
    #[inline]
    fn process(&mut self, x: f32) -> f32 {
        let y = self.b0 * x + self.z1;
        self.z1 = self.b1 * x - self.a1 * y + self.z2;
        self.z2 = self.b2 * x - self.a2 * y;
        y
    }
}

/// K-weighting stage 1 (high-pass at ~38 Hz) at 48 kHz. Coefficients are
/// re-derived for other rates in `k_weighting`.
fn highpass(rate: f32) -> Biquad {
    let f0 = 38.0;
    let q = 0.5;
    let w0 = 2.0 * std::f32::consts::PI * f0 / rate;
    let alpha = w0.sin() / (2.0 * q);
    let cos = w0.cos();
    let a0 = 1.0 + alpha;
    Biquad {
        b0: (1.0 + cos) / 2.0 / a0,
        b1: -(1.0 + cos) / a0,
        b2: (1.0 + cos) / 2.0 / a0,
        a1: (-2.0 * cos) / a0,
        a2: (1.0 - alpha) / a0,
        z1: 0.0,
        z2: 0.0,
    }
}

/// K-weighting stage 2 (high shelf, +4 dB above ~1.5 kHz).
fn highshelf(rate: f32) -> Biquad {
    let f0 = 1_500.0;
    let gain_db = 4.0;
    let a = 10f32.powf(gain_db / 40.0);
    let w0 = 2.0 * std::f32::consts::PI * f0 / rate;
    let cos = w0.cos();
    let sin = w0.sin();
    let alpha = sin / (2.0 * 0.707);
    let a0 = (a + 1.0) - (a - 1.0) * cos + 2.0 * a.sqrt() * alpha;
    Biquad {
        b0: a * ((a + 1.0) + (a - 1.0) * cos + 2.0 * a.sqrt() * alpha) / a0,
        b1: -2.0 * a * ((a - 1.0) + (a + 1.0) * cos) / a0,
        b2: a * ((a + 1.0) + (a - 1.0) * cos - 2.0 * a.sqrt() * alpha) / a0,
        a1: 2.0 * ((a - 1.0) - (a + 1.0) * cos) / a0,
        a2: ((a + 1.0) - (a - 1.0) * cos - 2.0 * a.sqrt() * alpha) / a0,
        z1: 0.0,
        z2: 0.0,
    }
}

fn energy_to_lufs(energy: f64) -> f32 {
    if energy <= 0.0 {
        f32::NEG_INFINITY
    } else {
        (-0.691 + 10.0 * energy.log10()) as f32
    }
}

/// Analyses one decoded file. Returns `None` when the file cannot be opened
/// or has no audible content.
pub fn analyze_file(path: &Path) -> Option<LoudnessAnalysis> {
    analyze_path(path)
}

/// Analyses a local file or a cached stream path. (Remote URLs are analysed
/// once the stream cache has a local copy.)
pub fn analyze_path(path: &Path) -> Option<LoudnessAnalysis> {
    let file = File::open(path).ok()?;
    let decoder = rodio::Decoder::new(BufReader::new(file)).ok()?;
    let rate = Source::sample_rate(&decoder);
    analyze_with_rate(decoder, rate)
}

/// Core measurement over any decoded f32 source.
pub fn analyze_source<S>(source: S) -> Option<LoudnessAnalysis>
where
    S: Iterator<Item = f32>,
{
    analyze_with_rate(source, 44_100)
}

/// Measures a decoded source whose native sample rate is known.
pub fn analyze_with_rate<S>(source: S, sample_rate: u32) -> Option<LoudnessAnalysis>
where
    S: Iterator<Item = f32>,
{
    let rate = sample_rate.max(8_000) as f32;
    let mut hp_l = highpass(rate);
    let mut hp_r = highpass(rate);
    let mut shelf_l = highshelf(rate);
    let mut shelf_r = highshelf(rate);
    let block_frames = (rate * 0.4) as usize;
    let hop_frames = (rate * 0.1) as usize;
    let mut block_energy = 0.0f64;
    let mut block_frames_seen = 0usize;
    let mut energies: Vec<f64> = Vec::new();
    let mut peak = 0.0f32;
    let mut channels = 0u8;
    let mut frame_channel = 0u8;
    let mut any_audio = false;

    for sample in source {
        if frame_channel == 0 {
            channels = 0; // recomputed below for first frame only
        }
        // The decoder is interleaved; we don't know the channel count here,
        // so treat the stream as stereo when possible. Symmetrically process
        // even samples through the left filter and odd through the right.
        let weighted = if frame_channel == 0 {
            shelf_l.process(hp_l.process(sample))
        } else {
            shelf_r.process(hp_r.process(sample))
        };
        peak = peak.max(sample.abs());
        if sample.abs() > 1e-5 {
            any_audio = true;
        }
        block_energy += f64::from(weighted) * f64::from(weighted);
        block_frames_seen += 1;
        frame_channel ^= 1;
        channels |= 1;
        if block_frames_seen >= block_frames {
            energies.push(block_energy / block_frames_seen as f64);
            // 75 % overlap: discard one hop worth of samples from the
            // running energy and frame count (approximation for streaming).
            let hop_energy = block_energy * (hop_frames as f64 / block_frames_seen as f64);
            block_energy -= hop_energy;
            block_frames_seen -= hop_frames;
        }
    }
    let _ = channels;
    if !any_audio || energies.is_empty() {
        return None;
    }
    // Absolute gate: -70 LUFS.
    let abs_gate = 10f64.powf((-70.0 + 0.691) / 10.0);
    let gated: Vec<f64> = energies
        .iter()
        .copied()
        .filter(|e| *e >= abs_gate)
        .collect();
    if gated.is_empty() {
        return None;
    }
    let mean = gated.iter().sum::<f64>() / gated.len() as f64;
    let relative_lufs = energy_to_lufs(mean);
    // Relative gate: -10 LU below the ungated mean.
    let rel_gate_lufs = relative_lufs - 10.0;
    let rel_energy = 10f64.powf((f64::from(rel_gate_lufs) + 0.691) / 10.0);
    let gated_rel: Vec<f64> = gated.iter().copied().filter(|e| *e >= rel_energy).collect();
    let final_mean = if gated_rel.is_empty() {
        mean
    } else {
        gated_rel.iter().sum::<f64>() / gated_rel.len() as f64
    };
    let integrated_lufs = energy_to_lufs(final_mean);
    if !integrated_lufs.is_finite() {
        return None;
    }
    let desired = TARGET_LUFS - integrated_lufs;
    // A gain that would push the measured peak above the headroom is
    // clamped: loudness normalization must never clip.
    let peak_linear_ceiling = 10f32.powf(PEAK_HEADROOM_DB / 20.0);
    let clip_safe = if peak > 1e-6 {
        20.0 * (peak_linear_ceiling / peak).log10()
    } else {
        MAX_GAIN_DB
    };
    let gain_db = desired.min(clip_safe).clamp(-24.0, MAX_GAIN_DB);
    Some(LoudnessAnalysis {
        integrated_lufs,
        peak,
        gain_db,
    })
}

#[cfg(test)]
#[allow(clippy::unwrap_used, reason = "test code")]
mod tests {
    use super::*;

    fn sine(freq: f32, amp: f32, seconds: f32) -> impl Iterator<Item = f32> {
        let n = (44_100.0 * seconds) as usize;
        (0..n).map(move |i| (2.0 * std::f32::consts::PI * freq * i as f32 / 44_100.0).sin() * amp)
    }

    #[test]
    fn a_quiet_track_gets_positive_gain_and_a_loud_one_negative() {
        let quiet = analyze_source(sine(440.0, 0.02, 4.0)).expect("quiet");
        let loud = analyze_source(sine(440.0, 0.9, 4.0)).expect("loud");
        assert!(
            quiet.gain_db > loud.gain_db,
            "{} vs {}",
            quiet.gain_db,
            loud.gain_db
        );
        assert!(
            loud.gain_db <= 0.0,
            "loud material is attenuated: {}",
            loud.gain_db
        );
    }

    #[test]
    fn gain_never_makes_the_peak_clip() {
        let loud = analyze_source(sine(1_000.0, 0.99, 2.0)).expect("loud");
        let gain_linear = 10f32.powf(loud.gain_db / 20.0);
        assert!(loud.peak * gain_linear <= 10f32.powf(PEAK_HEADROOM_DB / 20.0) + 1e-4);
    }

    #[test]
    fn silence_is_not_analysed() {
        assert!(analyze_source(std::iter::repeat(0.0).take(44_100 * 2)).is_none());
    }
}
