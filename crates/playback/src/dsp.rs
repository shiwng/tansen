//! Real-time safe DSP for the playback pipeline (EQ, loudness gain, fade).
//!
//! Everything here runs on the audio callback thread. The rules:
//! - no allocation, no locks, no I/O after construction;
//! - parameters arrive through atomics (written by the UI/engine threads);
//! - every gain change is smoothed sample-by-sample, so no zipper noise and
//!   no abrupt volume jumps.
//!
//! The EQ is a cascade of ten RBJ peaking biquads per channel. Coefficients
//! are recomputed every `COEFF_INTERVAL` frames from the smoothed gains —
//! a handful of floating-point operations, no branches on heap state.

use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};

/// The fixed ten-band layout, in Hz. Kept in sync with the Settings UI.
pub const EQ_BANDS_HZ: [f32; 10] = [
    31.0, 62.0, 125.0, 250.0, 500.0, 1_000.0, 2_000.0, 4_000.0, 8_000.0, 16_000.0,
];
/// Gain range the UI and the engine enforce.
pub const EQ_GAIN_LIMIT_DB: f32 = 12.0;
/// Preset names the backend accepts. `Flat` is also the reset state.
pub const EQ_PRESETS: [&str; 9] = [
    "flat",
    "bass-boost",
    "treble-boost",
    "rock",
    "pop",
    "classical",
    "jazz",
    "electronic",
    "vocal",
];

/// The gain curve for a named preset, in dB per band.
pub fn preset_gains(preset: &str) -> Option<[f32; 10]> {
    let g = |v: [f32; 10]| Some(v);
    match preset {
        "flat" => g([0.0; 10]),
        "bass-boost" => g([7.0, 6.0, 4.5, 2.5, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0]),
        "treble-boost" => g([0.0, 0.0, 0.0, 0.0, 0.0, 0.5, 2.0, 4.0, 5.5, 6.5]),
        "rock" => g([5.0, 4.0, 2.0, -1.0, -2.0, 0.0, 2.5, 4.0, 4.5, 3.5]),
        "pop" => g([-1.0, 0.0, 1.5, 3.0, 3.5, 2.0, 0.0, -1.0, -1.5, -1.0]),
        "classical" => g([3.0, 2.0, 1.0, 0.0, 0.0, 0.0, -1.0, -1.5, -2.0, -2.5]),
        "jazz" => g([3.5, 2.5, 1.0, 1.5, -1.0, -1.5, 0.0, 1.5, 2.5, 3.0]),
        "electronic" => g([6.0, 5.0, 2.0, 0.0, -2.5, 1.5, 0.0, 1.0, 4.5, 5.5]),
        "vocal" => g([-3.0, -2.0, -1.0, 1.5, 4.0, 4.5, 3.5, 1.5, 0.0, -1.5]),
        _ => None,
    }
}

/// Shared, lock-free DSP parameters. One instance lives in `AudioPipeline`;
/// every `RingSource` (current, preloaded, fading) reads the same values.
pub struct DspParams {
    pub eq_enabled: AtomicBool,
    pub eq_bands_db: [AtomicU32; 10],
    pub loudness_gain_db: AtomicU32,
    /// Playback master volume (the player bar). 0.0 – 1.0.
    pub master_volume: AtomicU32,
}

impl Default for DspParams {
    fn default() -> Self {
        Self {
            eq_enabled: AtomicBool::new(false),
            eq_bands_db: std::array::from_fn(|_| AtomicU32::new(0.0f32.to_bits())),
            loudness_gain_db: AtomicU32::new(0.0f32.to_bits()),
            master_volume: AtomicU32::new(1.0f32.to_bits()),
        }
    }
}

impl DspParams {
    pub fn set_eq(&self, enabled: bool, gains: &[f32; 10]) {
        self.eq_enabled.store(enabled, Ordering::Relaxed);
        for (atomic, gain) in self.eq_bands_db.iter().zip(gains.iter()) {
            let clamped = gain.clamp(-EQ_GAIN_LIMIT_DB, EQ_GAIN_LIMIT_DB);
            atomic.store(clamped.to_bits(), Ordering::Relaxed);
        }
    }

    pub fn eq_snapshot(&self) -> (bool, [f32; 10]) {
        let mut out = [0.0f32; 10];
        for (slot, atomic) in out.iter_mut().zip(self.eq_bands_db.iter()) {
            *slot = f32::from_bits(atomic.load(Ordering::Relaxed));
        }
        (self.eq_enabled.load(Ordering::Relaxed), out)
    }

    pub fn set_loudness_gain_db(&self, gain_db: f32) {
        let gain = if gain_db.is_finite() {
            gain_db.clamp(-24.0, 24.0)
        } else {
            0.0
        };
        self.loudness_gain_db
            .store(gain.to_bits(), Ordering::Relaxed);
    }

    pub fn loudness_gain_db(&self) -> f32 {
        f32::from_bits(self.loudness_gain_db.load(Ordering::Relaxed))
    }

    pub fn set_master_volume(&self, v: f32) {
        let v = if v.is_finite() {
            v.clamp(0.0, 1.0)
        } else {
            1.0
        };
        self.master_volume.store(v.to_bits(), Ordering::Relaxed);
    }

    pub fn master_volume(&self) -> f32 {
        f32::from_bits(self.master_volume.load(Ordering::Relaxed))
    }
}

/// One RBJ peaking-EQ biquad, Direct Form II transposed.
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
    const IDENTITY: Self = Self {
        b0: 1.0,
        b1: 0.0,
        b2: 0.0,
        a1: 0.0,
        a2: 0.0,
        z1: 0.0,
        z2: 0.0,
    };

    #[inline]
    fn process(&mut self, x: f32) -> f32 {
        let y = self.b0 * x + self.z1;
        self.z1 = self.b1 * x - self.a1 * y + self.z2;
        self.z2 = self.b2 * x - self.a2 * y;
        y
    }

    fn set_peaking(&mut self, freq: f32, gain_db: f32, sample_rate: f32) {
        if gain_db.abs() < 0.001 || sample_rate <= 0.0 {
            self.b0 = 1.0;
            self.b1 = 0.0;
            self.b2 = 0.0;
            self.a1 = 0.0;
            self.a2 = 0.0;
            return;
        }
        let a = 10f32.powf(gain_db / 40.0);
        let w0 = 2.0 * std::f32::consts::PI * (freq / sample_rate).clamp(0.0, 0.49);
        let cos_w0 = w0.cos();
        let sin_w0 = w0.sin();
        // A moderate Q keeps neighbouring bands from ringing into each other.
        let alpha = sin_w0 / (2.0 * 1.41);
        let a0 = 1.0 + alpha / a;
        self.b0 = (1.0 + alpha * a) / a0;
        self.b1 = (-2.0 * cos_w0) / a0;
        self.b2 = (1.0 - alpha * a) / a0;
        self.a1 = (-2.0 * cos_w0) / a0;
        self.a2 = (1.0 - alpha / a) / a0;
    }
}

/// Frames between coefficient refreshes. 32 frames at 44.1 kHz is well under
/// a millisecond and keeps the smoothing audible-smooth.
const COEFF_INTERVAL: u32 = 32;

/// Per-stream EQ state (one biquad bank per channel).
pub struct EqProcessor {
    channels: usize,
    bands: Vec<[Biquad; 10]>,
    current_db: [f32; 10],
    counter: u32,
    sample_rate: f32,
    last_enabled: bool,
}

impl EqProcessor {
    pub fn new(channels: u16, sample_rate: f32) -> Self {
        let channels = usize::from(channels.max(1));
        Self {
            channels,
            bands: vec![[Biquad::IDENTITY; 10]; channels],
            current_db: [0.0; 10],
            counter: 0,
            sample_rate,
            last_enabled: false,
        }
    }

    /// Processes one interleaved frame in place. `sample_index` is the
    /// interleaved position (used to pick the channel).
    #[inline]
    pub fn process(&mut self, sample: f32, channel: usize, params: &DspParams) -> f32 {
        if !params.eq_enabled.load(Ordering::Relaxed) {
            if self.last_enabled {
                // Disabling: leave the filters bypassed but keep their state
                // intact; re-enabling resumes from the smoothed values.
                self.last_enabled = false;
            }
            return sample;
        }
        self.last_enabled = true;
        if channel >= self.channels {
            return sample;
        }
        let mut y = sample;
        for biquad in self.bands[channel].iter_mut() {
            y = biquad.process(y);
        }
        y
    }

    /// Called once per interleaved frame (channel 0). Advances the gain
    /// smoothing and refreshes coefficients periodically.
    #[inline]
    pub fn tick(&mut self, params: &DspParams) {
        if !params.eq_enabled.load(Ordering::Relaxed) {
            return;
        }
        self.counter = self.counter.wrapping_add(1);
        // Smooth toward the target every frame (cheap) and recompute the
        // coefficients every COEFF_INTERVAL frames.
        let (_, target) = params.eq_snapshot();
        for (current, target) in self.current_db.iter_mut().zip(target.iter()) {
            let delta = target - *current;
            if delta.abs() > 0.0005 {
                *current += delta * 0.004;
            } else {
                *current = *target;
            }
        }
        if self.counter % COEFF_INTERVAL == 0 {
            for bank in &mut self.bands {
                for (band, (freq, gain)) in bank
                    .iter_mut()
                    .zip(EQ_BANDS_HZ.iter().zip(self.current_db.iter()))
                {
                    band.set_peaking(*freq, *gain, self.sample_rate);
                }
            }
        }
    }
}

/// A one-pole loudness-gain smoother. Keeps changes inaudible (no clicks)
/// while staying sample-accurate and real-time safe.
pub struct GainSmoother {
    current_db: f32,
    initialized: bool,
}

impl GainSmoother {
    pub fn new() -> Self {
        Self {
            current_db: 0.0,
            initialized: false,
        }
    }

    #[inline]
    pub fn next_linear(&mut self, target_db: f32, frames_per_step: f32) -> f32 {
        let target = if target_db.is_finite() {
            target_db
        } else {
            0.0
        };
        if !self.initialized {
            self.current_db = target;
            self.initialized = true;
        } else {
            let delta = target - self.current_db;
            if delta.abs() > 0.001 {
                let step = if frames_per_step > 0.0 {
                    1.0 / frames_per_step
                } else {
                    1.0
                };
                self.current_db += delta * step.min(1.0);
            } else {
                self.current_db = target;
            }
        }
        db_to_linear(self.current_db)
    }
}

impl Default for GainSmoother {
    fn default() -> Self {
        Self::new()
    }
}

#[inline]
pub fn db_to_linear(db: f32) -> f32 {
    10f32.powf(db / 20.0)
}

/// A per-stream fade ramp used for crossfades. `target` is 0..1 and the
/// processor moves toward it by `step` each frame. All fields are plain
/// floats owned by the audio callback, so no atomics or locks are needed
/// once the ramp is armed from the control thread.
pub struct FadeRamp {
    current: f32,
    target: f32,
    step: f32,
}

impl FadeRamp {
    pub fn unity() -> Self {
        Self {
            current: 1.0,
            target: 1.0,
            step: 1.0,
        }
    }

    pub fn start(current: f32, target: f32, frames: u64) -> Self {
        let step = if frames == 0 {
            1.0
        } else {
            1.0 / frames as f32
        };
        Self {
            current: current.clamp(0.0, 1.0),
            target: target.clamp(0.0, 1.0),
            step,
        }
    }

    #[inline]
    pub fn next(&mut self) -> f32 {
        if self.current < self.target {
            self.current = (self.current + self.step).min(self.target);
        } else if self.current > self.target {
            self.current = (self.current - self.step).max(self.target);
        }
        self.current
    }
}

/// Silence-skipping configuration, handed to the decode thread (not the
/// audio callback). Threshold is in dBFS; duration in milliseconds.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SilenceSkipConfig {
    pub enabled: bool,
    pub threshold_db: f32,
    pub min_duration_ms: u64,
}

impl Default for SilenceSkipConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            threshold_db: -50.0,
            min_duration_ms: 2_000,
        }
    }
}

impl SilenceSkipConfig {
    /// Linear amplitude for the threshold.
    pub fn threshold(&self) -> f32 {
        db_to_linear(self.threshold_db.clamp(-96.0, -12.0))
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, reason = "test code")]
mod tests {
    use super::*;

    #[test]
    fn presets_cover_the_required_set_and_are_bounded() {
        for name in EQ_PRESETS {
            let gains = preset_gains(name).unwrap_or_else(|| panic!("{name}"));
            assert!(gains.iter().all(|g| g.abs() <= EQ_GAIN_LIMIT_DB), "{name}");
        }
        assert!(preset_gains("not-a-preset").is_none());
        assert_eq!(preset_gains("flat").unwrap(), [0.0; 10]);
        assert!(preset_gains("bass-boost").unwrap()[0] > 0.0);
        assert!(preset_gains("treble-boost").unwrap()[9] > 0.0);
    }

    #[test]
    fn eq_boosts_the_band_it_targets() {
        let params = DspParams::default();
        let mut gains = [0.0f32; 10];
        gains[0] = 9.0; // 31 Hz
        params.set_eq(true, &gains);
        let mut eq = EqProcessor::new(1, 44_100.0);
        // Feed a 31 Hz sine and compare RMS after settling with unity.
        let mut peak = 0.0f32;
        for i in 0..44_100 {
            let x = (2.0 * std::f32::consts::PI * 31.0 * i as f32 / 44_100.0).sin() * 0.1;
            eq.tick(&params);
            let y = eq.process(x, 0, &params);
            if i > 20_000 {
                peak = peak.max(y.abs());
            }
        }
        assert!(peak > 0.2, "31 Hz band boost must be audible; peak {peak}");
    }

    #[test]
    fn eq_disable_is_exactly_bypass() {
        let params = DspParams::default();
        let mut gains = [12.0f32; 10];
        params.set_eq(false, &gains);
        let mut eq = EqProcessor::new(2, 44_100.0);
        for _ in 0..1000 {
            eq.tick(&params);
        }
        assert_eq!(eq.process(0.25, 0, &params), 0.25);
        assert_eq!(eq.process(-0.25, 1, &params), -0.25);
    }

    #[test]
    fn gain_smoother_moves_without_jumping() {
        let mut s = GainSmoother::new();
        // First call initializes at the target: a started track must not
        // ramp from an unrelated previous value.
        assert_eq!(s.next_linear(6.0, 32.0), db_to_linear(6.0));
        let first = s.next_linear(0.0, 32.0);
        assert!(
            first < db_to_linear(6.0) && first > db_to_linear(0.0),
            "smoothed"
        );
        for _ in 0..400 {
            s.next_linear(0.0, 32.0);
        }
        assert!((s.next_linear(0.0, 32.0) - 1.0).abs() < 1e-4);
    }

    #[test]
    fn fade_ramp_reaches_targets() {
        let mut fade = FadeRamp::start(1.0, 0.0, 100);
        for _ in 0..100 {
            fade.next();
        }
        assert_eq!(fade.next(), 0.0);
        let mut up = FadeRamp::start(0.0, 1.0, 50);
        for _ in 0..50 {
            up.next();
        }
        assert_eq!(up.next(), 1.0);
    }
}
