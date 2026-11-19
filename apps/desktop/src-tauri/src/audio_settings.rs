//! Persistent audio preferences (output depth, gapless, crossfade, loudness,
//! skip-silence, equalizer). One struct read at startup and patched from the
//! Settings UI; every change is stored in SQLite before it is applied.

use persistence::{repo, Db};
use playback::dsp::{preset_gains, EQ_BANDS_HZ, EQ_GAIN_LIMIT_DB};

pub const KEY_OUTPUT_BITS: &str = "audio_output_bits";
pub const KEY_GAPLESS: &str = "audio_gapless";
pub const KEY_CROSSFADE_MS: &str = "audio_crossfade_ms";
pub const KEY_CROSSFADE_CURVE: &str = "audio_crossfade_curve";
pub const KEY_LOUDNESS: &str = "audio_loudness";
pub const KEY_SKIP_SILENCE: &str = "audio_skip_silence";
pub const KEY_SKIP_THRESHOLD: &str = "audio_skip_threshold_db";
pub const KEY_SKIP_MIN_MS: &str = "audio_skip_min_ms";
pub const KEY_EQ_ENABLED: &str = "audio_eq_enabled";
pub const KEY_EQ_GAINS: &str = "audio_eq_gains";
pub const KEY_EQ_PRESET: &str = "audio_eq_preset";

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AudioSettings {
    /// Requested hardware depth: 16 or 32.
    pub output_bits: u32,
    pub gapless: bool,
    /// Crossfade / auto-mix duration in milliseconds (0 = off).
    pub crossfade_ms: u64,
    /// Crossfade shape: "linear" or "equal-power".
    pub crossfade_curve: String,
    pub loudness: bool,
    pub skip_silence: bool,
    /// dBFS threshold, -96..-12.
    pub skip_threshold_db: f32,
    pub skip_min_ms: u64,
    pub eq_enabled: bool,
    /// Ten band gains in dB, in `EQ_BANDS_HZ` order.
    pub eq_gains: [f32; 10],
    /// Last applied preset name, or null for a manual curve.
    pub eq_preset: Option<String>,
}

impl Default for AudioSettings {
    fn default() -> Self {
        Self {
            // 32-bit float is the pipeline's internal precision and the
            // broadest hardware default; 16-bit is opt-in.
            output_bits: 32,
            gapless: true,
            crossfade_ms: 0,
            crossfade_curve: "linear".into(),
            loudness: false,
            skip_silence: false,
            skip_threshold_db: -50.0,
            skip_min_ms: 2_000,
            eq_enabled: false,
            eq_gains: [0.0; 10],
            eq_preset: Some("flat".into()),
        }
    }
}

fn parse_bool(raw: Option<String>, default: bool) -> bool {
    match raw.as_deref() {
        Some("true") => true,
        Some("false") => false,
        _ => default,
    }
}

fn parse_f32(raw: Option<String>, default: f32) -> f32 {
    raw.and_then(|v| v.parse::<f32>().ok())
        .filter(|v| v.is_finite())
        .unwrap_or(default)
}

fn parse_u64(raw: Option<String>, default: u64) -> u64 {
    raw.and_then(|v| v.parse::<u64>().ok()).unwrap_or(default)
}

/// Crossfade curves the DSP actually implements. Unknown names fall back to
/// linear instead of being persisted as a label with no behaviour.
fn normalize_curve(raw: Option<&str>) -> String {
    match raw.unwrap_or("linear").trim().to_ascii_lowercase().as_str() {
        "equal-power" | "equal_power" | "equalpower" => "equal-power".into(),
        _ => "linear".into(),
    }
}

impl AudioSettings {
    pub fn load(db: &Db) -> Self {
        let d = Self::default();
        let get = |key: &str| repo::get_setting(db, key).ok().flatten();
        let output_bits = match parse_u64(get(KEY_OUTPUT_BITS), u64::from(d.output_bits)) {
            16 => 16,
            _ => 32,
        };
        let mut eq_gains = d.eq_gains;
        if let Some(raw) = get(KEY_EQ_GAINS) {
            if let Ok(values) = serde_json::from_str::<Vec<f32>>(&raw) {
                if values.len() == EQ_BANDS_HZ.len() {
                    for (slot, value) in eq_gains.iter_mut().zip(values) {
                        *slot = value.clamp(-EQ_GAIN_LIMIT_DB, EQ_GAIN_LIMIT_DB);
                    }
                }
            }
        }
        let eq_preset = get(KEY_EQ_PRESET).filter(|p| !p.is_empty());
        Self {
            output_bits,
            gapless: parse_bool(get(KEY_GAPLESS), d.gapless),
            crossfade_ms: parse_u64(get(KEY_CROSSFADE_MS), d.crossfade_ms).min(12_000),
            crossfade_curve: normalize_curve(get(KEY_CROSSFADE_CURVE).as_deref()),
            loudness: parse_bool(get(KEY_LOUDNESS), d.loudness),
            skip_silence: parse_bool(get(KEY_SKIP_SILENCE), d.skip_silence),
            skip_threshold_db: parse_f32(get(KEY_SKIP_THRESHOLD), d.skip_threshold_db)
                .clamp(-96.0, -12.0),
            skip_min_ms: parse_u64(get(KEY_SKIP_MIN_MS), d.skip_min_ms).clamp(250, 30_000),
            eq_enabled: parse_bool(get(KEY_EQ_ENABLED), d.eq_enabled),
            eq_gains,
            eq_preset,
        }
    }

    /// Effective crossfade for the pipeline: explicit crossfade first, then
    /// 0 (gapless) when the toggle is on.
    pub fn effective_crossfade_ms(&self) -> u64 {
        if self.crossfade_ms > 0 {
            self.crossfade_ms
        } else {
            0
        }
    }

    pub fn persist(&self, db: &Db) -> Result<(), persistence::DbError> {
        let write = |key: &str, value: String| repo::set_setting(db, key, &value);
        write(KEY_OUTPUT_BITS, self.output_bits.to_string())?;
        write(KEY_GAPLESS, self.gapless.to_string())?;
        write(KEY_CROSSFADE_MS, self.crossfade_ms.to_string())?;
        write(KEY_CROSSFADE_CURVE, self.crossfade_curve.clone())?;
        write(KEY_LOUDNESS, self.loudness.to_string())?;
        write(KEY_SKIP_SILENCE, self.skip_silence.to_string())?;
        write(KEY_SKIP_THRESHOLD, self.skip_threshold_db.to_string())?;
        write(KEY_SKIP_MIN_MS, self.skip_min_ms.to_string())?;
        write(KEY_EQ_ENABLED, self.eq_enabled.to_string())?;
        write(
            KEY_EQ_GAINS,
            serde_json::to_string(&self.eq_gains)
                .unwrap_or_else(|_| "[0,0,0,0,0,0,0,0,0,0]".into()),
        )?;
        write(KEY_EQ_PRESET, self.eq_preset.clone().unwrap_or_default())?;
        Ok(())
    }

    pub fn apply_preset(&mut self, preset: &str) -> bool {
        let Some(gains) = preset_gains(preset) else {
            return false;
        };
        self.eq_gains = gains;
        self.eq_preset = Some(preset.to_owned());
        true
    }
}

/// A partial update from the Settings UI. Only present fields change; the
/// rest keep their stored value.
#[derive(Debug, Clone, Default, serde::Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct AudioSettingsPatch {
    pub output_bits: Option<u32>,
    pub gapless: Option<bool>,
    pub crossfade_ms: Option<u64>,
    pub crossfade_curve: Option<String>,
    pub loudness: Option<bool>,
    pub skip_silence: Option<bool>,
    pub skip_threshold_db: Option<f32>,
    pub skip_min_ms: Option<u64>,
    pub eq_enabled: Option<bool>,
    pub eq_gains: Option<Vec<f32>>,
    pub eq_preset: Option<String>,
}

impl AudioSettingsPatch {
    /// Applies the patch and returns true when the output stream must be
    /// rebuilt (brief interruption, only on an explicit depth change).
    pub fn apply(self, settings: &mut AudioSettings) -> bool {
        let mut reopen = false;
        if let Some(bits) = self.output_bits {
            let bits = if bits == 16 { 16 } else { 32 };
            reopen = bits != settings.output_bits;
            settings.output_bits = bits;
        }
        if let Some(v) = self.gapless {
            settings.gapless = v;
        }
        if let Some(v) = self.crossfade_ms {
            settings.crossfade_ms = v.min(12_000);
        }
        if let Some(v) = self.crossfade_curve {
            settings.crossfade_curve = normalize_curve(Some(&v));
        }
        if let Some(v) = self.loudness {
            settings.loudness = v;
        }
        if let Some(v) = self.skip_silence {
            settings.skip_silence = v;
        }
        if let Some(v) = self.skip_threshold_db {
            if v.is_finite() {
                settings.skip_threshold_db = v.clamp(-96.0, -12.0);
            }
        }
        if let Some(v) = self.skip_min_ms {
            settings.skip_min_ms = v.clamp(250, 30_000);
        }
        if let Some(v) = self.eq_enabled {
            settings.eq_enabled = v;
        }
        if let Some(values) = self.eq_gains {
            if values.len() == EQ_BANDS_HZ.len() {
                for (slot, value) in settings.eq_gains.iter_mut().zip(values) {
                    *slot = if value.is_finite() {
                        value.clamp(-EQ_GAIN_LIMIT_DB, EQ_GAIN_LIMIT_DB)
                    } else {
                        0.0
                    };
                }
                settings.eq_preset = None;
            }
        }
        if let Some(preset) = self.eq_preset {
            if preset.is_empty() {
                settings.eq_preset = None;
            } else {
                let _ = settings.apply_preset(&preset);
            }
        }
        reopen
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, reason = "test code")]
mod tests {
    use super::*;

    #[test]
    fn patch_clamps_and_reports_output_change() {
        let mut settings = AudioSettings::default();
        assert!(!AudioSettingsPatch {
            gapless: Some(false),
            ..Default::default()
        }
        .apply(&mut settings));
        assert!(!settings.gapless);
        assert!(AudioSettingsPatch {
            output_bits: Some(16),
            ..Default::default()
        }
        .apply(&mut settings));
        assert_eq!(settings.output_bits, 16);
        let mut patch = AudioSettingsPatch {
            eq_preset: Some("rock".into()),
            ..Default::default()
        };
        patch.eq_gains = None;
        assert!(!patch.apply(&mut settings));
        assert_eq!(settings.eq_preset.as_deref(), Some("rock"));
        assert!(settings.eq_gains.iter().any(|g| g.abs() > 0.1));
    }

    #[test]
    fn crossfade_curve_is_validated() {
        let mut settings = AudioSettings::default();
        assert_eq!(settings.crossfade_curve, "linear");
        AudioSettingsPatch {
            crossfade_curve: Some("equal-power".into()),
            ..Default::default()
        }
        .apply(&mut settings);
        assert_eq!(settings.crossfade_curve, "equal-power");
        AudioSettingsPatch {
            crossfade_curve: Some("nonsense".into()),
            ..Default::default()
        }
        .apply(&mut settings);
        assert_eq!(settings.crossfade_curve, "linear");
    }
}
