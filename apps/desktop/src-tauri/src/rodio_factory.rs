//! The real `PipelineFactory` over the playback crate's rodio pipeline (M7):
//! one audio thread + one sink; `prepare` swaps the sink's content,
//! `release` silences it, `transport` drives pause/resume/seek/volume.
//! Shared by the app shell (M9 wiring) and the `engine_demo` debug binary.

use app_core::playback_engine::{PipelineFactory, Transport};
use playback::pipeline::StreamStatus;
use sources::resolver::Resolver;
use sources::{ResolvedMedia, StreamRequest, TrackKey};
use tokio::sync::watch;
use app_core::playback_types::{EntryId, ErrorKind};
use playback::pipeline::AudioPipeline;
use playback::StreamCache;
use std::sync::{Arc, Mutex};

pub struct RodioFactory {
    /// EntryId → resolved media (URL + expiry when the source states it).
    /// Library lookups happen live (scans add files); the M1 bench sentinel
    /// resolves through the env hook.
    resolve: Box<dyn Fn(u64) -> Option<ResolvedMedia> + Send + Sync>,
    /// S3: the catalogue resolver for source-keyed entries. Library/bench
    /// entries keep the closure path; a TrackKey (if registered) resolves
    /// through here.
    resolver: Option<Arc<Resolver>>,
    /// EntryId → TrackKey for remote/source entries (S3).
    track_keys: Mutex<std::collections::HashMap<u64, TrackKey>>,
    /// Quality ceiling used for resolves (S2 setting). A Mutex so
    /// `quality_set` can change it for future prepares without touching
    /// the playing track (F5).
    quality: Mutex<StreamRequest>,
    pipeline: Mutex<Option<AudioPipeline>>,
    cache: Option<Arc<StreamCache>>,
    /// M15: the current stream's fill status; the engine subscribes once.
    status_tx: watch::Sender<StreamStatus>,
    /// F5: EntryId → the ResolvedMedia the last successful `prepare` for
    /// that entry produced. Read by `stream_info`; removed by `invalidate`.
    last_resolved: Mutex<std::collections::HashMap<u64, ResolvedMedia>>,
}

/// F5: the one sanctioned read-only stream-info query payload (F4). Facts
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
        Self {
            resolve,
            resolver,
            track_keys: Mutex::new(std::collections::HashMap::new()),
            last_resolved: Mutex::new(std::collections::HashMap::new()),
            quality: Mutex::new(quality),
            pipeline: Mutex::new(None),
            cache,
            status_tx,
        }
    }

    /// S3: register a source-keyed entry so `prepare` routes through the
    /// resolver (sequential, cached, pinned per track).
    pub fn register_track(&self, entry: u64, key: TrackKey) {
        if let Ok(mut keys) = self.track_keys.lock() {
            keys.insert(entry, key);
        }
    }

    /// (frames_played, pos_ms) of the live pipeline, for observability.
    fn record_resolved(&self, entry: EntryId, media: ResolvedMedia) {
        if let Ok(mut m) = self.last_resolved.lock() {
            m.insert(entry.0, media);
        }
    }

    /// F5: the read-only stream-info query for one entry.
    pub fn stream_info(&self, entry: u64) -> Option<StreamInfo> {
        let media = self.last_resolved.lock().ok()?.get(&entry).cloned()?;
        let requested = self.track_keys.lock().ok().and_then(|k| k.get(&entry).map(|t| t.source));
        let is_fallback = matches!(requested, Some(req) if req != media.source);
        Some(StreamInfo {
            source: format!("{:?}", media.source).to_lowercase(),
            codec: media.format.as_ref().map(|f| f.codec.clone()).filter(|c| !c.is_empty()),
            kbps: media.format.as_ref().and_then(|f| f.kbps),
            is_lossless: media.format.as_ref().and_then(|f| f.is_lossless),
            below_request: media.below_request,
            is_fallback,
        })
    }

    fn quality(&self) -> StreamRequest {
        self.quality.lock().map(|q| q.clone()).unwrap_or(StreamRequest::Best)
    }

    /// F5: the quality ceiling for future resolves. The playing track keeps
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
        // S6: a source-backed entry registers its key with the resolver.
        if let Some(key) = key {
            if let Ok(mut keys) = self.track_keys.lock() {
                keys.insert(entry.0, key.clone());
            }
        }
        // S3: source-keyed entries resolve through the resolver (singleflight,
        // cached, pinned per track); everything else keeps the library/bench
        // closure.
        let media = {
            let key = self
                .track_keys
                .lock()
                .ok()
                .and_then(|k| k.get(&entry.0).cloned());
            match (key, self.resolver.as_ref()) {
                // F9: the engine task runs inside the tokio runtime —
                // resolve_blocking's nested runtime would panic there. The
                // sanctioned pattern: block_in_place + block_on the async
                // resolve (prepare is sync and off the audio thread; the
                // blocking-pool swap keeps the worker responsive).
                (Some(key), Some(resolver)) => {
                    let resolver = resolver.clone();
                    let key = key.clone();
                    let quality = self.quality();
                    let media = match tokio::runtime::Handle::try_current() {
                        Ok(handle) => tokio::task::block_in_place(|| {
                            handle.block_on(async move {
                                resolver.resolve(&key, &quality).await
                            })
                        }),
                        // Plain thread (engine_demo, tests): the one-shot
                        // runtime inside resolve_blocking is fine.
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
        // M16: a resolution that is already past its expiry is dead upfront —
        // surface Resolve so the engine's one-retry flow runs, not a doomed
        // load.
        if let Some(expiry) = media.expiry {
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs() as i64)
                .unwrap_or(i64::MAX);
            if now >= expiry {
                tracing::info!(entry = entry.0, "resolved media past expiry; refusing to load");
                return Err(ErrorKind::Resolve);
            }
        }
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
        let mut guard = self.pipeline.lock().map_err(|_| ErrorKind::Device)?;
        if guard.is_none() {
            *guard = Some(
                AudioPipeline::spawn(self.cache.clone(), self.status_tx.clone())
                    .map_err(|_| ErrorKind::Device)?,
            );
        }
        let pipeline = guard.as_ref().ok_or(ErrorKind::Device)?;
        let loaded = pipeline.load(&media.url).map_err(|e| {
            if e.contains("no audio output device") {
                ErrorKind::Device
            } else {
                ErrorKind::Decode
            }
        })?;
        // F5: record only after a successful load (mid-play re-resolves
        // overwrite; a failed load never poisons the query).
        self.record_resolved(entry, media);
        Ok(loaded)
    }


    fn release(&self, entry: EntryId) {
        // F5 (audit): evict the stream-info record when the engine releases
        // an entry — bounds the map to the current entry + stragglers.
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

    /// S8: resolve-ahead — warm the S3 resolver cache for a future remote
    /// entry off the audio path. A later `prepare` for the same key hits the
    /// URL cache instead of the network. Local entries (no key) and the
    /// no-resolver case are no-ops.
    fn prefetch_resolve(
        &self,
        _entry: EntryId,
        key: Option<&sources::TrackKey>,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = ()> + Send>> {
        let Some(key) = key.cloned() else { return Box::pin(std::future::ready(())) };
        let Some(resolver) = self.resolver.clone() else { return Box::pin(std::future::ready(())) };
        let request = self.quality();
        Box::pin(async move {
            // F9: async context — resolve directly (no nested runtime).
            let _ = resolver.resolve(&key, &request).await;
        })
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
            k.insert(2, sources::TrackKey::new(SourceKind::YouTube, "v1").unwrap());
        }
        let info = factory.stream_info(2).unwrap();
        assert!(info.is_fallback, "keyed YouTube answered by Local = fallback");

        // Same source => not a fallback.
        factory.record_resolved(EntryId(3), media(SourceKind::YouTube, "AAC", Some(false)));
        if let Ok(mut k) = factory.track_keys.lock() {
            k.insert(3, sources::TrackKey::new(SourceKind::YouTube, "v2").unwrap());
        }
        assert!(!factory.stream_info(3).unwrap().is_fallback);

        // Unknown entry => None; invalidate clears.
        assert!(factory.stream_info(99).is_none());
        factory.invalidate(EntryId(1));
        assert!(factory.stream_info(1).is_none(), "invalidate clears the record");
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
