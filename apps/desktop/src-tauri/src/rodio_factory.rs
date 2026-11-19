//! The real `PipelineFactory` over the playback crate's rodio pipeline:
//! one audio thread + one sink; `prepare` swaps the sink's content,
//! `release` silences it, `transport` drives pause/resume/seek/volume.
//! Shared by the app shell and the `engine_demo` debug binary.

use app_core::playback_engine::{PipelineFactory, Transport};
use app_core::playback_types::{EntryId, ErrorKind};
use playback::dsp::DspParams;
use playback::pipeline::{AudioPipeline, OutputCapabilities, PipelineConfig, StreamStatus};
use playback::StreamCache;
use sources::resolver::Resolver;
use sources::{ResolvedMedia, SourceKind, StreamRequest, TrackKey};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use tokio::sync::watch;

pub struct RodioFactory {
    /// EntryId → resolved media (URL + expiry when the source states it).
    /// Library lookups happen live (scans add files).
    resolve: Box<dyn Fn(u64) -> Option<ResolvedMedia> + Send + Sync>,
    /// The catalogue resolver for source-keyed entries. Library entries
    /// keep the closure path; a TrackKey (if registered) resolves through
    /// here.
    resolver: Option<Arc<Resolver>>,
    /// EntryId → TrackKey for remote/source entries.
    track_keys: Mutex<std::collections::HashMap<u64, TrackKey>>,
    /// Quality ceiling used for resolves (setting). A Mutex so
    /// `quality_set` can change it for future prepares without touching
    /// the playing track.
    quality: Mutex<StreamRequest>,
    pipeline: Mutex<Option<AudioPipeline>>,
    cache: Option<Arc<StreamCache>>,
    /// The current stream's fill status; the engine subscribes once.
    status_tx: watch::Sender<StreamStatus>,
    /// EntryId → the ResolvedMedia the last successful `prepare` for
    /// that entry produced. Read by `stream_info`; removed by `invalidate`.
    last_resolved: Mutex<std::collections::HashMap<u64, ResolvedMedia>>,
    /// Real-time DSP parameters shared with every stream (EQ, loudness,
    /// master volume).
    params: Arc<DspParams>,
    /// Output negotiation + crossfade/gapless/skip-silence configuration.
    config: Arc<PipelineConfig>,
    /// Loudness normalization (background analysis + cached gains).
    loudness: Option<Arc<LoudnessHandle>>,
}

/// The one sanctioned read-only stream-info query payload. Facts
/// come from the last successful resolution for the entry; `is_fallback`
/// compares the source the entry was keyed with against the source that
/// actually answered.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StreamInfo {
    pub source: String,
    pub codec: Option<String>,
    pub kbps: Option<u32>,
    pub is_lossless: Option<bool>,
    pub below_request: bool,
    pub is_fallback: bool,
}

impl RodioFactory {
    pub fn new(
        resolve: Box<dyn Fn(u64) -> Option<ResolvedMedia> + Send + Sync>,
        cache: Option<Arc<StreamCache>>,
    ) -> Self {
        Self::with_resolver(resolve, None, cache, StreamRequest::Best)
    }

    pub fn with_resolver(
        resolve: Box<dyn Fn(u64) -> Option<ResolvedMedia> + Send + Sync>,
        resolver: Option<Arc<Resolver>>,
        cache: Option<Arc<StreamCache>>,
        quality: StreamRequest,
    ) -> Self {
        let (status_tx, _) = watch::channel(StreamStatus::Refilled);
        let params = Arc::new(DspParams::default());
        let config = Arc::new(PipelineConfig::default());
        Self {
            resolve,
            resolver,
            track_keys: Mutex::new(std::collections::HashMap::new()),
            last_resolved: Mutex::new(std::collections::HashMap::new()),
            quality: Mutex::new(quality),
            pipeline: Mutex::new(None),
            cache,
            status_tx,
            params,
            config,
            loudness: None,
        }
    }

    pub fn with_loudness(mut self, loudness: Arc<LoudnessHandle>) -> Self {
        self.loudness = Some(loudness);
        self
    }

    pub fn dsp(&self) -> Arc<DspParams> {
        self.params.clone()
    }

    /// Apply user audio preferences without touching the audio callback.
    pub fn configure_playback(
        &self,
        crossfade_ms: u64,
        crossfade_curve: u32,
        gapless: bool,
        skip_silence: bool,
        skip_threshold_db: f32,
        skip_min_ms: u64,
    ) {
        self.config
            .crossfade_ms
            .store(crossfade_ms, Ordering::Relaxed);
        let curve = if crossfade_curve == playback::pipeline::FADE_CURVE_EQUAL_POWER {
            playback::pipeline::FADE_CURVE_EQUAL_POWER
        } else {
            playback::pipeline::FADE_CURVE_LINEAR
        };
        self.config.crossfade_curve.store(curve, Ordering::Relaxed);
        self.config.gapless.store(gapless, Ordering::Relaxed);
        self.config
            .skip_silence
            .store(skip_silence, Ordering::Relaxed);
        self.config.skip_threshold_db.store(
            skip_threshold_db.clamp(-96.0, -12.0).to_bits(),
            Ordering::Relaxed,
        );
        self.config
            .skip_min_ms
            .store(skip_min_ms.clamp(250, 30_000), Ordering::Relaxed);
    }

    /// Requested hardware output depth. Takes effect the next time the
    /// output stream is opened (startup, device change, or `RebuildOutput`).
    pub fn set_output_bits(&self, bits: u32) {
        self.config
            .output_bits
            .store(if bits == 16 { 16 } else { 32 }, Ordering::Relaxed);
    }

    pub fn set_eq(&self, enabled: bool, gains: &[f32; 10]) {
        self.params.set_eq(enabled, gains);
    }

    pub fn set_loudness_enabled(&self, on: bool) {
        if let Some(loudness) = &self.loudness {
            loudness.set_enabled(on);
        } else if !on {
            self.params.set_loudness_gain_db(0.0);
        }
    }

    pub fn output_capabilities(&self) -> OutputCapabilitiesReport {
        let capabilities = playback::pipeline::output_capabilities();
        let negotiated_bits = self.config.negotiated_bits.load(Ordering::Relaxed);
        let negotiated_format = self
            .config
            .negotiated_format
            .lock()
            .ok()
            .and_then(|f| f.clone());
        OutputCapabilitiesReport {
            capabilities,
            negotiated_bits,
            negotiated_format,
        }
    }

    /// Resolve media for an entry without loading it. Shared by `prepare`
    /// and the preload path.
    fn resolve_media(
        &self,
        entry: EntryId,
        key: Option<&sources::TrackKey>,
    ) -> Result<ResolvedMedia, ErrorKind> {
        if let Some(key) = key {
            if let Ok(mut keys) = self.track_keys.lock() {
                keys.insert(entry.0, key.clone());
            }
        }
        let media = {
            let key = self
                .track_keys
                .lock()
                .ok()
                .and_then(|k| k.get(&entry.0).cloned());
            match (key, self.resolver.as_ref()) {
                (Some(key), Some(resolver)) => {
                    let resolver = resolver.clone();
                    let quality = self.quality();
                    let media = match tokio::runtime::Handle::try_current() {
                        Ok(handle) => tokio::task::block_in_place(|| {
                            handle.block_on(async move { resolver.resolve(&key, &quality).await })
                        }),
                        Err(_) => resolver.resolve_blocking(&key, &quality),
                    };
                    media.map_err(|e| {
                        tracing::warn!(error = %e, "resolver failed for track");
                        ErrorKind::Resolve
                    })?
                }
                _ => (self.resolve)(entry.0).ok_or(ErrorKind::Resolve)?,
            }
        };
        if let Some(expiry) = media.expiry {
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs() as i64)
                .unwrap_or(i64::MAX);
            if now >= expiry {
                tracing::info!(
                    entry = entry.0,
                    "resolved media past expiry; refusing to load"
                );
                return Err(ErrorKind::Resolve);
            }
        }
        Ok(media)
    }

    fn ensure_pipeline(
        &self,
    ) -> Result<std::sync::MutexGuard<'_, Option<AudioPipeline>>, ErrorKind> {
        let mut guard = self.pipeline.lock().map_err(|_| ErrorKind::Device)?;
        if guard.is_none() {
            let pipeline = AudioPipeline::spawn_with_config(
                self.cache.clone(),
                self.status_tx.clone(),
                self.params.clone(),
                self.config.clone(),
            )
            .map_err(|_| ErrorKind::Device)?;
            *guard = Some(pipeline);
        }
        Ok(guard)
    }

    fn apply_loudness_for(&self, media: &ResolvedMedia) {
        let Some(loudness) = &self.loudness else {
            return;
        };
        if !loudness.enabled() {
            self.params.set_loudness_gain_db(0.0);
            return;
        }
        // Only local files and cached streams have bytes on disk to analyse.
        // A remote URL gets unity gain — never a guess.
        if media.source == SourceKind::Local || media.url.starts_with('/') {
            loudness.prepare_gain(&media.url);
        } else {
            self.params.set_loudness_gain_db(0.0);
        }
    }

    /// (frames_played, pos_ms) of the live pipeline, for observability.
    fn record_resolved(&self, entry: EntryId, media: ResolvedMedia) {
        if let Ok(mut m) = self.last_resolved.lock() {
            m.insert(entry.0, media);
        }
    }

    /// The read-only stream-info query for one entry.
    pub fn stream_info(&self, entry: u64) -> Option<StreamInfo> {
        let media = self.last_resolved.lock().ok()?.get(&entry).cloned()?;
        let requested = self
            .track_keys
            .lock()
            .ok()
            .and_then(|k| k.get(&entry).map(|t| t.source));
        let is_fallback = matches!(requested, Some(req) if req != media.source);
        Some(StreamInfo {
            source: format!("{:?}", media.source).to_lowercase(),
            codec: media
                .format
                .as_ref()
                .map(|f| f.codec.clone())
                .filter(|c| !c.is_empty()),
            kbps: media.format.as_ref().and_then(|f| f.kbps),
            is_lossless: media.format.as_ref().and_then(|f| f.is_lossless),
            below_request: media.below_request,
            is_fallback,
        })
    }

    fn quality(&self) -> StreamRequest {
        self.quality
            .lock()
            .map(|q| q.clone())
            .unwrap_or(StreamRequest::Best)
    }

    /// The quality ceiling for future resolves. The playing track keeps
    /// its already-resolved media; the next prepare uses the new value.
    pub fn set_quality(&self, quality: StreamRequest) {
        if let Ok(mut q) = self.quality.lock() {
            *q = quality;
        }
    }

    pub fn frames(&self) -> (u64, u64) {
        match self.pipeline.lock() {
            Ok(guard) => match guard.as_ref() {
                Some(p) => (p.frames_played(), p.pos_ms()),
                None => (0, 0),
            },
            Err(_) => (0, 0),
        }
    }
}

impl PipelineFactory for RodioFactory {
    fn prepare(&self, entry: EntryId, key: Option<&sources::TrackKey>) -> Result<(), ErrorKind> {
        // Source-keyed entries resolve through the resolver (singleflight,
        // cached, pinned per track); everything else keeps the library
        // closure.
        let media = self.resolve_media(entry, key)?;
        self.apply_loudness_for(&media);
        #[cfg(debug_assertions)]
        tracing::debug!(
            entry = entry.0,
            source = ?media.source,
            codec = ?media.format.as_ref().map(|f| f.codec.as_str()),
            container = ?media.container,
            mime = ?media.mime,
            expiry = ?media.expiry,
            "resolved media accepted for pipeline"
        );
        let guard = self.ensure_pipeline()?;
        let pipeline = guard.as_ref().ok_or(ErrorKind::Device)?;
        // A radio key resolves to the station's own URL: play it as a live
        // broadcast (unbounded body, no seeking) instead of a ranged file.
        let live = media.source == sources::SourceKind::Radio;
        let loaded = if live {
            pipeline.load_live(&media.url)
        } else {
            pipeline.load(&media.url)
        }
        .map_err(|e| {
            if e.contains("no audio output device") {
                ErrorKind::Device
            } else {
                ErrorKind::Decode
            }
        })?;
        // Record only after a successful load (mid-play re-resolves
        // overwrite; a failed load never poisons the query).
        self.record_resolved(entry, media);
        Ok(loaded)
    }

    fn release(&self, entry: EntryId) {
        // Evict the stream-info record when the engine releases an entry —
        // bounds the map to the current entry + stragglers.
        if let Ok(mut m) = self.last_resolved.lock() {
            m.remove(&entry.0);
        }
        if let Ok(guard) = self.pipeline.lock() {
            if let Some(p) = guard.as_ref() {
                p.clear();
            }
        }
    }

    fn transport(&self, action: Transport) {
        if let Ok(guard) = self.pipeline.lock() {
            if let Some(p) = guard.as_ref() {
                match action {
                    Transport::Play => p.play(),
                    Transport::Pause => p.pause(),
                    Transport::Seek { ms } => p.seek_ms(ms),
                    Transport::Volume(v) => p.set_volume(v),
                    Transport::ReopenOutput => p.reopen_output(),
                }
            }
        }
    }

    fn position_ms(&self) -> Option<u64> {
        let guard = self.pipeline.lock().ok()?;
        guard.as_ref().map(|p| p.pos_ms())
    }

    fn duration_ms(&self) -> Option<u64> {
        let guard = self.pipeline.lock().ok()?;
        guard.as_ref().and_then(|p| p.duration_ms())
    }

    fn status_watch(&self) -> Option<watch::Receiver<StreamStatus>> {
        Some(self.status_tx.subscribe())
    }

    fn crossfade_ms(&self) -> u64 {
        self.config.crossfade_ms()
    }

    fn gapless(&self) -> bool {
        self.config.gapless()
    }

    fn preload_next(
        &self,
        entry: EntryId,
        key: Option<&sources::TrackKey>,
    ) -> Result<(), ErrorKind> {
        let media = self.resolve_media(entry, key)?;
        self.apply_loudness_for(&media);
        let guard = self.ensure_pipeline()?;
        let pipeline = guard.as_ref().ok_or(ErrorKind::Device)?;
        pipeline.preload(&media.url).map_err(|e| {
            if e.contains("no audio output device") {
                ErrorKind::Device
            } else {
                ErrorKind::Decode
            }
        })?;
        self.record_resolved(entry, media);
        Ok(())
    }

    fn adopt_preloaded(&self, crossfade_ms: u64) -> Result<(), ErrorKind> {
        let guard = self.pipeline.lock().map_err(|_| ErrorKind::Device)?;
        let pipeline = guard.as_ref().ok_or(ErrorKind::Device)?;
        pipeline.adopt_preloaded(crossfade_ms).map_err(|e| {
            if e.contains("preloaded stream was discarded") {
                ErrorKind::Decode
            } else {
                ErrorKind::Device
            }
        })
    }

    fn discard_preloaded(&self) {
        if let Ok(guard) = self.pipeline.lock() {
            if let Some(pipeline) = guard.as_ref() {
                pipeline.discard_preloaded();
            }
        }
    }

    fn invalidate(&self, entry: EntryId) {
        if let Ok(mut keys) = self.track_keys.lock() {
            if let Some(key) = keys.remove(&entry.0) {
                if let Some(resolver) = &self.resolver {
                    resolver.invalidate(&key);
                }
            }
        }
        if let Ok(mut m) = self.last_resolved.lock() {
            m.remove(&entry.0);
        }
    }

    /// Resolve-ahead — warm the resolver cache for a future remote
    /// entry off the audio path. A later `prepare` for the same key hits the
    /// URL cache instead of the network. Local entries (no key) and the
    /// no-resolver case are no-ops.
    fn prefetch_resolve(
        &self,
        _entry: EntryId,
        key: Option<&sources::TrackKey>,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = ()> + Send>> {
        let Some(key) = key.cloned() else {
            return Box::pin(std::future::ready(()));
        };
        let Some(resolver) = self.resolver.clone() else {
            return Box::pin(std::future::ready(()));
        };
        let request = self.quality();
        Box::pin(async move {
            // Async context — resolve directly (no nested runtime).
            let _ = resolver.resolve(&key, &request).await;
        })
    }
}

/// Output capability + the format the open stream really negotiated.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OutputCapabilitiesReport {
    #[serde(flatten)]
    pub capabilities: OutputCapabilities,
    pub negotiated_bits: u32,
    pub negotiated_format: Option<String>,
}

/// Loudness normalization: background EBU R128 analysis with a persistent
/// per-file cache. The audio callback only ever reads one atomic gain.
pub struct LoudnessHandle {
    db: Arc<persistence::Db>,
    params: Arc<DspParams>,
    enabled: Arc<AtomicBool>,
    current_path: Arc<Mutex<Option<String>>>,
    in_flight: Arc<Mutex<std::collections::HashSet<String>>>,
}

impl LoudnessHandle {
    pub fn new(db: Arc<persistence::Db>, params: Arc<DspParams>, enabled: bool) -> Self {
        Self {
            db,
            params,
            enabled: Arc::new(AtomicBool::new(enabled)),
            current_path: Arc::new(Mutex::new(None)),
            in_flight: Arc::new(Mutex::new(std::collections::HashSet::new())),
        }
    }

    pub fn enabled(&self) -> bool {
        self.enabled.load(Ordering::Relaxed)
    }

    pub fn set_enabled(&self, on: bool) {
        self.enabled.store(on, Ordering::Relaxed);
        if !on {
            self.params.set_loudness_gain_db(0.0);
            return;
        }
        let path = self.current_path.lock().ok().and_then(|p| p.clone());
        if let Some(path) = path {
            self.apply(&path);
        }
    }

    /// Called from `prepare`/`preload_next` before the stream starts. Uses a
    /// cached measurement when it is newer than the file, otherwise schedules
    /// analysis off the engine task and plays at unity gain meanwhile.
    pub fn prepare_gain(&self, path: &str) {
        if let Ok(mut current) = self.current_path.lock() {
            *current = Some(path.to_owned());
        }
        if self.enabled() {
            self.apply(path);
        } else {
            self.params.set_loudness_gain_db(0.0);
        }
    }

    fn apply(&self, path: &str) {
        let mtime = std::fs::metadata(path)
            .and_then(|m| m.modified())
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);
        if let Ok(Some((gain, measured_at))) = persistence::repo::get_track_loudness(&self.db, path)
        {
            if measured_at >= mtime {
                self.params.set_loudness_gain_db(gain);
                return;
            }
        }
        self.params.set_loudness_gain_db(0.0);
        self.analyze_async(path.to_owned());
    }

    fn analyze_async(&self, path: String) {
        {
            let Ok(mut in_flight) = self.in_flight.lock() else {
                return;
            };
            if !in_flight.insert(path.clone()) {
                return; // already being analysed
            }
        }
        let db = self.db.clone();
        let params = self.params.clone();
        let enabled = self.enabled.clone();
        let current_path = self.current_path.clone();
        let in_flight = self.in_flight.clone();
        std::thread::Builder::new()
            .name("loudness-analysis".into())
            .spawn(move || {
                let measured = playback::loudness::analyze_path(std::path::Path::new(&path));
                if let Some(a) = measured {
                    let now = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .map(|d| d.as_secs() as i64)
                        .unwrap_or(0);
                    let _ = persistence::repo::set_track_loudness(
                        &db,
                        &path,
                        a.gain_db,
                        a.integrated_lufs,
                        a.peak,
                        now,
                    );
                    // Only apply if this file is still the one playing.
                    let current = current_path.lock().ok().and_then(|p| p.clone());
                    if enabled.load(Ordering::Relaxed) && current.as_deref() == Some(path.as_str())
                    {
                        params.set_loudness_gain_db(a.gain_db);
                    }
                }
                if let Ok(mut set) = in_flight.lock() {
                    set.remove(&path);
                }
            })
            .ok();
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, reason = "test code")]
mod stream_info_tests {
    use super::*;
    use sources::{Container, SourceKind, StreamFormat};

    fn media(source: SourceKind, codec: &str, lossless: Option<bool>) -> ResolvedMedia {
        ResolvedMedia {
            url: "https://x/y".into(),
            headers: vec![],
            expiry: None,
            length: None,
            mime: None,
            format: Some(StreamFormat {
                codec: codec.into(),
                is_lossless: lossless,
                kbps: Some(131),
                is_dolby_atmos: false,
                sample_rate: None,
                bit_depth: None,
                kbps_inferred: false,
            }),
            container: Some(Container::Progressive),
            source,
            below_request: false,
        }
    }

    #[test]
    fn stream_info_maps_fields_and_fallback() {
        let factory = RodioFactory::new(Box::new(|_| None), None);
        // Local file entry: no registered key => never a fallback.
        factory.record_resolved(EntryId(1), media(SourceKind::Local, "FLAC", Some(true)));
        let info = factory.stream_info(1).unwrap();
        assert_eq!(info.source, "local");
        assert_eq!(info.codec.as_deref(), Some("FLAC"));
        assert_eq!(info.kbps, Some(131));
        assert_eq!(info.is_lossless, Some(true));
        assert!(!info.is_fallback);
        assert!(!info.below_request);

        // Remote entry keyed YouTube, answered by the local prefer seam:
        // fallback detected by source mismatch.
        factory.record_resolved(EntryId(2), media(SourceKind::Local, "FLAC", Some(true)));
        if let Ok(mut k) = factory.track_keys.lock() {
            k.insert(
                2,
                sources::TrackKey::new(SourceKind::YouTube, "v1").unwrap(),
            );
        }
        let info = factory.stream_info(2).unwrap();
        assert!(
            info.is_fallback,
            "keyed YouTube answered by Local = fallback"
        );

        // Same source => not a fallback.
        factory.record_resolved(EntryId(3), media(SourceKind::YouTube, "AAC", Some(false)));
        if let Ok(mut k) = factory.track_keys.lock() {
            k.insert(
                3,
                sources::TrackKey::new(SourceKind::YouTube, "v2").unwrap(),
            );
        }
        assert!(!factory.stream_info(3).unwrap().is_fallback);

        // Unknown entry => None; invalidate clears.
        assert!(factory.stream_info(99).is_none());
        factory.invalidate(EntryId(1));
        assert!(
            factory.stream_info(1).is_none(),
            "invalidate clears the record"
        );
    }

    #[test]
    fn stream_info_dto_uses_camel_case_wire_keys() {
        let info = StreamInfo {
            source: "youtube".into(),
            codec: Some("AAC".into()),
            kbps: Some(131),
            is_lossless: Some(false),
            below_request: true,
            is_fallback: false,
        };
        let json = serde_json::to_value(&info).unwrap();
        assert!(json.get("isLossless").is_some(), "{json}");
        assert!(json.get("belowRequest").is_some());
        assert!(json.get("isFallback").is_some());
        assert!(json.get("is_lossless").is_none());
    }
}
