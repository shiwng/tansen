//! The resolver — one place a `TrackKey` becomes a pinned
//! `ResolvedMedia`. Sequential only (no racing); single-flight per key;
//! per-source timeout; URL cache TTL = min(media expiry, const); negative
//! cache for permanent errors. Resolution never runs on the audio thread:
//! callers use [`Resolver::resolve_blocking`] from engine/factory code.

use crate::matcher::TrackMeta;
use crate::search::SEARCH_PER_SOURCE_LIMIT;
use crate::{
    decodable, MusicSource, ResolvedMedia, SourceError, SourceKind, StreamFormat, StreamRequest,
    TrackKey,
};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::OnceCell;

/// Per-source resolve timeout.
pub const RESOLVE_TIMEOUT: Duration = Duration::from_secs(15);
/// URL cache TTL ceiling; effective TTL = min(media.expiry, this).
pub const URL_CACHE_TTL: Duration = Duration::from_secs(60 * 60);
/// Negative-cache TTL for permanent failures.
pub const NEGATIVE_CACHE_TTL: Duration = Duration::from_secs(10 * 60);
/// One sequential retry for transient/timeout failures.
const TRANSIENT_RETRIES: u32 = 1;

pub type SourceLookup = Arc<dyn Fn(SourceKind) -> Option<Arc<dyn MusicSource>> + Send + Sync>;

/// The enabled sources in rank order (kind rank, then user rank). The
/// resolver consults this only when the pinned source fails; the provider
/// decides what "enabled" means (shell builds it from the registry).
pub type FallbackProvider = Arc<dyn Fn() -> Vec<Arc<dyn MusicSource>> + Send + Sync>;

/// Target metadata for a key, so fallback candidates can be matched.
/// `None` = no metadata known → no fallback is attempted.
pub type MetaLookup = Arc<dyn Fn(&TrackKey) -> Option<TrackMeta> + Send + Sync>;

/// An optional local-library match, injected by the shell as a plain
/// closure (read-only; no trait, no new dependency edge). Returns a
/// `ResolvedMedia` whose url is the local file path — the pipeline's
/// `open_source` plays non-HTTP paths from disk directly. The closure owns
/// the existence/readability check; a stale file simply yields `None` and
/// resolution proceeds remotely.
pub type LocalMatch = Arc<dyn Fn(&TrackMeta) -> Option<ResolvedMedia> + Send + Sync>;

enum CacheEntry {
    Ok {
        media: ResolvedMedia,
        fetched_at: std::time::Instant,
    },
    Err {
        error: SourceError,
        at: std::time::Instant,
    },
}

pub struct Resolver {
    sources: SourceLookup,
    fallbacks: FallbackProvider,
    meta: MetaLookup,
    /// The injected local-library matcher, when the shell provides one.
    local_match: Option<LocalMatch>,
    cache: Mutex<HashMap<(TrackKey, StreamRequest), CacheEntry>>,
    flights: Mutex<HashMap<TrackKey, Arc<OnceCell<Result<ResolvedMedia, SourceError>>>>>,
}

impl Resolver {
    /// No fallback (behavior preserved for tests/leaf callers).
    pub fn new(sources: SourceLookup) -> Self {
        Self::with_fallbacks(sources, Arc::new(Vec::new), Arc::new(|_| None))
    }

    /// Add the fallback ladder — other enabled sources tried in rank
    /// order when the pinned source fails, using `meta` for matching.
    pub fn with_fallbacks(
        sources: SourceLookup,
        fallbacks: FallbackProvider,
        meta: MetaLookup,
    ) -> Self {
        Self {
            sources,
            fallbacks,
            meta,
            local_match: None,
            cache: Mutex::new(HashMap::new()),
            flights: Mutex::new(HashMap::new()),
        }
    }

    /// Prefer a local file (Lossless/Best only) before any remote
    /// resolution. Builder method — existing constructors are unchanged.
    pub fn with_local_match(mut self, f: LocalMatch) -> Self {
        self.local_match = Some(f);
        self
    }

    /// Drops every cached resolution for `key` (URL + negative).
    pub fn invalidate(&self, key: &TrackKey) {
        let mut cache = self.cache.lock().unwrap_or_else(|p| p.into_inner());
        cache.retain(|(k, _), _| k != key);
    }

    /// The async resolve. Single flight per key: concurrent callers await the
    /// same in-flight future, so exactly one upstream call happens.
    pub async fn resolve(
        &self,
        key: &TrackKey,
        request: &StreamRequest,
    ) -> Result<ResolvedMedia, SourceError> {
        if let Some(hit) = self.cache_hit(key, request) {
            crate::metrics::source::resolve_warm_hit();
            return hit;
        }
        let cell = {
            let mut flights = self.flights.lock().unwrap_or_else(|p| p.into_inner());
            match flights.get(key) {
                Some(cell) => cell.clone(),
                None => {
                    let cell = Arc::new(OnceCell::new());
                    flights.insert(key.clone(), cell.clone());
                    cell
                }
            }
        };
        let t0 = std::time::Instant::now();
        let mut result = cell
            .get_or_init(|| self.resolve_upstream(key, request))
            .await
            .clone();
        // A cold resolve (including waiting on someone else's flight).
        crate::metrics::source::resolve_cold(t0.elapsed().as_secs_f64() * 1000.0);
        self.flights
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .remove(key);
        // Conformance: a Lossless request answered with anything not
        // codec-lossless (any source: local hit, pinned, ladder) is marked
        // below-request before it is cached, so every later consumer sees
        // the settle-for flag too.
        if let Ok(media) = &mut result {
            if matches!(request, StreamRequest::Lossless)
                && media.format.as_ref().and_then(|f| f.is_lossless) != Some(true)
            {
                media.below_request = true;
            }
        }
        // Store (or refresh) the cache entry for later seeks/pins.
        match &result {
            Ok(media) => self.cache_ok(key, request, media.clone()),
            Err(e) => self.cache_err(key, request, e.clone()),
        }
        result.clone()
    }

    /// Blocking facade for the engine/factory (never the audio thread).
    pub fn resolve_blocking(
        &self,
        key: &TrackKey,
        request: &StreamRequest,
    ) -> Result<ResolvedMedia, SourceError> {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|e| SourceError::Transient(e.to_string()))?
            .block_on(self.resolve(key, request))
    }

    fn cache_hit(
        &self,
        key: &TrackKey,
        request: &StreamRequest,
    ) -> Option<Result<ResolvedMedia, SourceError>> {
        let cache = self.cache.lock().unwrap_or_else(|p| p.into_inner());
        let entry = cache.get(&(key.clone(), request.clone()))?;
        match entry {
            CacheEntry::Ok { media, fetched_at } => {
                let ttl = media.expiry.map(|e| {
                    let now = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .map(|d| d.as_secs() as i64)
                        .unwrap_or(i64::MAX);
                    (e - now).max(0) as u64
                });
                let ttl = ttl
                    .map(|s| Duration::from_secs(s))
                    .unwrap_or(URL_CACHE_TTL)
                    .min(URL_CACHE_TTL);
                if fetched_at.elapsed() < ttl {
                    Some(Ok(media.clone()))
                } else {
                    None
                }
            }
            CacheEntry::Err { error, at } => {
                if at.elapsed() < NEGATIVE_CACHE_TTL {
                    Some(Err(error.clone()))
                } else {
                    None
                }
            }
        }
    }

    fn cache_ok(&self, key: &TrackKey, request: &StreamRequest, media: ResolvedMedia) {
        self.cache.lock().unwrap_or_else(|p| p.into_inner()).insert(
            (key.clone(), request.clone()),
            CacheEntry::Ok {
                media,
                fetched_at: std::time::Instant::now(),
            },
        );
    }

    fn cache_err(&self, key: &TrackKey, request: &StreamRequest, error: SourceError) {
        self.cache.lock().unwrap_or_else(|p| p.into_inner()).insert(
            (key.clone(), request.clone()),
            CacheEntry::Err {
                error,
                at: std::time::Instant::now(),
            },
        );
    }

    /// Pick the best decodable candidate for the request. Candidates are
    /// ordered by the source (best first → the resolve contract returns the
    /// best it has); if the best candidate is undecodable we still scan the
    /// rest so HLS/DASH/Opus answers don't hide a plain AAC sibling.
    fn pick_best_decodable(
        &self,
        candidates: Vec<ResolvedMedia>,
    ) -> Result<ResolvedMedia, SourceError> {
        // Prefer the first decodable candidate (source order = quality
        // order). All-undecodable → Unsupported.
        candidates
            .into_iter()
            .find(|m| {
                decodable(
                    m.format.as_ref().unwrap_or(&StreamFormat::default()),
                    m.container,
                )
            })
            .ok_or(SourceError::Unsupported)
    }

    async fn resolve_upstream(
        &self,
        key: &TrackKey,
        request: &StreamRequest,
    ) -> Result<ResolvedMedia, SourceError> {
        // Prefer a local file for Lossless/Best before any remote call.
        // The closure's local media caches like any other answer; a miss
        // (no match, or a stale/unreadable path) falls through to remote.
        if matches!(request, StreamRequest::Lossless | StreamRequest::Best) {
            if let Some(local) = self.local_match.as_ref() {
                if let Some(meta) = (self.meta)(key) {
                    if let Some(media) = local(&meta) {
                        return Ok(media);
                    }
                }
            }
        }
        // Pinned source only: the lookup returns None when the kind is
        // disabled or unknown → Disabled.
        let source = (self.sources)(key.source).ok_or(SourceError::Disabled)?;
        match self.resolve_pinned(source.as_ref(), key, request).await {
            Ok(media) => Ok(media),
            Err(original) => {
                // Pinned failed (Permanent, or transient/timeout with
                // retries exhausted) → try other enabled sources in rank
                // order. No match anywhere → the ORIGINAL error.
                let fell_back = self.try_fallback(source.as_ref(), key, request).await;
                crate::metrics::source::ladder_outcome(fell_back.is_some());
                match fell_back {
                    Some(media) => Ok(media),
                    None => Err(original),
                }
            }
        }
    }

    /// The pinned source, with its one transient retry.
    async fn resolve_pinned(
        &self,
        source: &dyn MusicSource,
        key: &TrackKey,
        request: &StreamRequest,
    ) -> Result<ResolvedMedia, SourceError> {
        let mut last = SourceError::Transient("no attempt".into());
        for _ in 0..=TRANSIENT_RETRIES {
            let attempt = tokio::time::timeout(RESOLVE_TIMEOUT, source.resolve(key, request)).await;
            match attempt {
                Ok(Ok(Some(media))) => return Ok(media),
                Ok(Ok(None)) => return Err(SourceError::Permanent("no media for track".into())),
                Ok(Err(e)) => {
                    match e {
                        SourceError::Transient(_) | SourceError::Timeout(_) => {
                            last = e;
                            continue;
                        }
                        other => return Err(other), // permanent/other: no retry
                    }
                }
                Err(_elapsed) => {
                    last = SourceError::Timeout("resolve deadline elapsed".into());
                }
            }
        }
        Err(last)
    }

    /// Sequential fallback ladder. Skips the pinned source itself (by
    /// config id) and `Local` (no library lookup seam yet). Each
    /// candidate is searched with the matcher's queries; the first candidate
    /// that matches AND streams successfully wins. No racing: strictly one
    /// source at a time.
    async fn try_fallback(
        &self,
        pinned: &dyn MusicSource,
        key: &TrackKey,
        request: &StreamRequest,
    ) -> Option<ResolvedMedia> {
        let target = (self.meta)(key)?;
        for candidate in (self.fallbacks)() {
            if candidate.config_id() == pinned.config_id() {
                continue;
            }
            if candidate.kind() == SourceKind::Local {
                continue; // No local lookup seam in the resolver
            }
            if let Some(mut media) = self
                .match_on_source(candidate.as_ref(), &target, request)
                .await
            {
                // The fallback source is the recorded source.
                media.source = candidate.kind();
                return Some(media);
            }
        }
        None
    }

    /// Search one source for a matching candidate and open its stream.
    async fn match_on_source(
        &self,
        source: &dyn MusicSource,
        target: &TrackMeta,
        request: &StreamRequest,
    ) -> Option<ResolvedMedia> {
        for query in crate::matcher::queries(target) {
            let rows = tokio::time::timeout(
                RESOLVE_TIMEOUT,
                source.search(&query, SEARCH_PER_SOURCE_LIMIT),
            )
            .await
            .ok()?
            .ok()?;
            for row in rows {
                let candidate = TrackMeta {
                    title: row.title.clone(),
                    artist: row.artist.clone(),
                    duration_sec: row.duration_sec.map(f64::from),
                };
                if !crate::matcher::matches(target, &candidate) {
                    continue;
                }
                let candidate_key = TrackKey::new(source.kind(), row.track_id).ok()?;
                if let Ok(Ok(Some(media))) =
                    tokio::time::timeout(RESOLVE_TIMEOUT, source.resolve(&candidate_key, request))
                        .await
                {
                    return Some(media);
                }
            }
        }
        None
    }

    /// Resolve + decodability gate. A source may return multiple
    /// candidate formats (future contract); the best decodable one wins.
    /// If the sidecar returns only one format, `candidates` is that one.
    pub async fn resolve_decodable(
        &self,
        _key: &TrackKey,
        _request: &StreamRequest,
        candidates: Vec<ResolvedMedia>,
    ) -> Result<ResolvedMedia, SourceError> {
        self.pick_best_decodable(candidates)
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, reason = "test code")]
mod tests {
    use super::*;
    use crate::{CatalogTrack, SourceHealth, SourceStream, StreamFormat};
    use std::sync::atomic::{AtomicUsize, Ordering};

    /// Fake source with scripted search/resolve outcomes + call counters.
    struct FakeSource {
        calls: Arc<AtomicUsize>,
        search_calls: Arc<AtomicUsize>,
        config: String,
        kind: SourceKind,
        outcomes: Mutex<std::collections::VecDeque<Result<Option<ResolvedMedia>, SourceError>>>,
        search_outcomes: Mutex<std::collections::VecDeque<Result<Vec<CatalogTrack>, SourceError>>>,
    }
    impl FakeSource {
        fn new() -> (Arc<Self>, Arc<AtomicUsize>) {
            let (s, c, _sc) = Self::new_full(SourceKind::YouTube, "fake");
            (s, c)
        }
        fn new_full(
            kind: SourceKind,
            config: &str,
        ) -> (Arc<Self>, Arc<AtomicUsize>, Arc<AtomicUsize>) {
            let calls = Arc::new(AtomicUsize::new(0));
            let search_calls = Arc::new(AtomicUsize::new(0));
            let this = Arc::new(Self {
                calls: calls.clone(),
                search_calls: search_calls.clone(),
                config: config.into(),
                kind,
                outcomes: Mutex::new(std::collections::VecDeque::new()),
                search_outcomes: Mutex::new(std::collections::VecDeque::new()),
            });
            (this, calls, search_calls)
        }
        fn push(&self, outcome: Result<Option<ResolvedMedia>, SourceError>) {
            self.outcomes.lock().unwrap().push_back(outcome);
        }
        fn push_search(&self, outcome: Result<Vec<CatalogTrack>, SourceError>) {
            self.search_outcomes.lock().unwrap().push_back(outcome);
        }
    }
    #[async_trait::async_trait]
    impl MusicSource for FakeSource {
        fn config_id(&self) -> &str {
            &self.config
        }
        fn kind(&self) -> SourceKind {
            self.kind
        }
        fn display_name(&self) -> &str {
            "Fake"
        }
        async fn health(&self) -> Result<SourceHealth, SourceError> {
            Ok(SourceHealth::Ok { detail: None })
        }
        async fn search(&self, _q: &str, _l: usize) -> Result<Vec<CatalogTrack>, SourceError> {
            self.search_calls.fetch_add(1, Ordering::SeqCst);
            self.search_outcomes
                .lock()
                .unwrap()
                .pop_front()
                .unwrap_or(Ok(vec![]))
        }
        async fn stream(
            &self,
            _k: &TrackKey,
            _r: &StreamRequest,
        ) -> Result<Option<SourceStream>, SourceError> {
            Err(SourceError::Unsupported)
        }
        async fn resolve(
            &self,
            _k: &TrackKey,
            _r: &StreamRequest,
        ) -> Result<Option<ResolvedMedia>, SourceError> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            self.outcomes
                .lock()
                .unwrap()
                .pop_front()
                .unwrap_or(Ok(None))
        }
    }

    fn media(url: &str, expiry_s: Option<i64>) -> ResolvedMedia {
        ResolvedMedia {
            url: url.into(),
            headers: vec![],
            expiry: expiry_s,
            length: None,
            mime: None,
            format: Some(StreamFormat {
                codec: "AAC".into(),
                ..Default::default()
            }),
            container: None,
            source: SourceKind::YouTube,
            below_request: false,
        }
    }

    fn resolver_with(fake: Arc<FakeSource>) -> Resolver {
        let lookup: SourceLookup = Arc::new(move |kind| {
            (kind == SourceKind::YouTube).then(|| fake.clone() as Arc<dyn MusicSource>)
        });
        Resolver::new(lookup)
    }

    fn key() -> TrackKey {
        TrackKey {
            source: SourceKind::YouTube,
            id: "t".into(),
        }
    }

    #[tokio::test(start_paused = true)]
    async fn success_then_cache_no_second_upstream_call() {
        let (fake, calls) = FakeSource::new();
        fake.push(Ok(Some(media("http://a", None))));
        let r = resolver_with(fake.clone());
        let m1 = r.resolve(&key(), &StreamRequest::Best).await.unwrap();
        assert_eq!(m1.url, "http://a");
        let m2 = r.resolve(&key(), &StreamRequest::Best).await.unwrap();
        assert_eq!(m2.url, "http://a");
        assert_eq!(
            calls.load(Ordering::SeqCst),
            1,
            "single-flight + cache = 1 call"
        );
    }

    #[tokio::test(start_paused = true)]
    async fn parallel_requests_make_one_upstream_call() {
        let (fake, calls) = FakeSource::new();
        fake.push(Ok(Some(media("http://a", None))));
        let r = Arc::new(resolver_with(fake));
        let mut futs = Vec::new();
        for _ in 0..8 {
            let r = r.clone();
            futs.push(tokio::spawn(async move {
                r.resolve(&key(), &StreamRequest::Best).await
            }));
        }
        for f in futs {
            assert!(f.await.unwrap().is_ok());
        }
        assert_eq!(calls.load(Ordering::SeqCst), 1, "concurrent => 1 upstream");
    }

    #[tokio::test(start_paused = true)]
    async fn timeout_is_classified_and_not_cached() {
        let (_fake, _calls) = FakeSource::new();
        // A source that never answers: timeout after RESOLVE_TIMEOUT.
        let slow: SourceLookup = Arc::new(move |kind| {
            (kind == SourceKind::YouTube).then(|| Arc::new(SlowSource) as Arc<dyn MusicSource>)
        });
        let r = Resolver::new(slow);
        let err = r.resolve(&key(), &StreamRequest::Best).await.unwrap_err();
        assert!(matches!(err, SourceError::Timeout(_)), "{err:?}");
    }

    struct SlowSource;
    #[async_trait::async_trait]
    impl MusicSource for SlowSource {
        fn config_id(&self) -> &str {
            "slow"
        }
        fn kind(&self) -> SourceKind {
            SourceKind::YouTube
        }
        fn display_name(&self) -> &str {
            "Slow"
        }
        async fn health(&self) -> Result<SourceHealth, SourceError> {
            Ok(SourceHealth::Ok { detail: None })
        }
        async fn search(&self, _q: &str, _l: usize) -> Result<Vec<CatalogTrack>, SourceError> {
            Ok(vec![])
        }
        async fn stream(
            &self,
            _k: &TrackKey,
            _r: &StreamRequest,
        ) -> Result<Option<SourceStream>, SourceError> {
            Err(SourceError::Unsupported)
        }
        async fn resolve(
            &self,
            _k: &TrackKey,
            _r: &StreamRequest,
        ) -> Result<Option<ResolvedMedia>, SourceError> {
            tokio::time::sleep(Duration::from_secs(60)).await;
            Ok(Some(media("http://late", None)))
        }
    }

    #[tokio::test(start_paused = true)]
    async fn transient_retries_once_then_succeeds() {
        let (fake, calls) = FakeSource::new();
        fake.push(Err(SourceError::Transient("flaky".into())));
        fake.push(Ok(Some(media("http://b", None))));
        let r = resolver_with(fake.clone());
        let m = r.resolve(&key(), &StreamRequest::Best).await.unwrap();
        assert_eq!(m.url, "http://b");
        assert_eq!(calls.load(Ordering::SeqCst), 2, "one retry");
    }

    #[tokio::test(start_paused = true)]
    async fn permanent_error_is_negatively_cached() {
        let (fake, calls) = FakeSource::new();
        fake.push(Err(SourceError::Permanent("nope".into())));
        let r = resolver_with(fake.clone());
        assert!(matches!(
            r.resolve(&key(), &StreamRequest::Best).await.unwrap_err(),
            SourceError::Permanent(_)
        ));
        // Second resolve: no upstream call (negative cache).
        assert!(r.resolve(&key(), &StreamRequest::Best).await.is_err());
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        // Invalidate clears the negative cache.
        r.invalidate(&key());
        assert!(r.resolve(&key(), &StreamRequest::Best).await.is_err());
        assert_eq!(calls.load(Ordering::SeqCst), 2);
    }

    #[tokio::test(start_paused = true)]
    async fn expiry_forces_resolve() {
        let (fake, calls) = FakeSource::new();
        let past = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64
            - 10;
        fake.push(Ok(Some(media("http://e", Some(past)))));
        let r = resolver_with(fake.clone());
        let _ = r.resolve(&key(), &StreamRequest::Best).await.unwrap();
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        // Past expiry: cache miss → re-resolve (fresh media now).
        fake.push(Ok(Some(media("http://fresh", None))));
        let m = r.resolve(&key(), &StreamRequest::Best).await.unwrap();
        assert_eq!(m.url, "http://fresh");
        assert_eq!(calls.load(Ordering::SeqCst), 2);
    }

    #[tokio::test(start_paused = true)]
    async fn disabled_source_returns_disabled() {
        let lookup: SourceLookup = Arc::new(|_| None);
        let r = Resolver::new(lookup);
        let err = r.resolve(&key(), &StreamRequest::Best).await.unwrap_err();
        assert!(matches!(err, SourceError::Disabled));
    }

    // Fallback ladder

    fn catalog_row(track_id: &str, title: &str, artist: &str, dur: Option<u32>) -> CatalogTrack {
        CatalogTrack {
            track_id: track_id.into(),
            config_id: "fake".into(),
            title: title.into(),
            artist: artist.into(),
            album: None,
            duration_sec: dur,
            artwork_url: None,
            format: None,
            embedded_stream_url: None,
        }
    }

    fn target_meta() -> TrackMeta {
        TrackMeta {
            title: "Song".into(),
            artist: "Artist".into(),
            duration_sec: Some(200.0),
        }
    }

    fn ladder_resolver(pinned: Arc<FakeSource>, fallbacks: Vec<Arc<FakeSource>>) -> Resolver {
        let lookup: SourceLookup = Arc::new(move |kind| {
            (kind == SourceKind::YouTube).then(|| pinned.clone() as Arc<dyn MusicSource>)
        });
        let provider: FallbackProvider = Arc::new(move || {
            fallbacks
                .iter()
                .map(|f| f.clone() as Arc<dyn MusicSource>)
                .collect()
        });
        let meta: MetaLookup = Arc::new(|_| Some(target_meta()));
        Resolver::with_fallbacks(lookup, provider, meta)
    }

    #[tokio::test(start_paused = true)]
    async fn fallback_hit_returns_fallback_media_with_source_recorded() {
        let (pinned, pinned_calls) = FakeSource::new();
        pinned.push(Err(SourceError::Permanent("nope".into())));
        let (fa, fa_calls, fa_search_calls) = FakeSource::new_full(SourceKind::Addon, "addon-a");
        fa.push_search(Ok(vec![catalog_row("id-a", "Song", "Artist", Some(200))]));
        fa.push(Ok(Some(media("http://fallback", None))));
        let r = ladder_resolver(pinned.clone(), vec![fa.clone()]);
        let m = r.resolve(&key(), &StreamRequest::Best).await.unwrap();
        assert_eq!(m.url, "http://fallback");
        assert_eq!(m.source, SourceKind::Addon, "fallback source recorded");
        assert_eq!(pinned_calls.load(Ordering::SeqCst), 1, "pinned tried once");
        assert_eq!(
            fa_search_calls.load(Ordering::SeqCst),
            1,
            "fallback searched once"
        );
        assert_eq!(fa_calls.load(Ordering::SeqCst), 1, "fallback streamed once");
    }

    #[tokio::test(start_paused = true)]
    async fn no_match_returns_the_original_error() {
        let (pinned, _) = FakeSource::new();
        pinned.push(Err(SourceError::Permanent("nope".into())));
        let (fa, fa_calls, _) = FakeSource::new_full(SourceKind::Addon, "addon-a");
        fa.push_search(Ok(vec![catalog_row(
            "id-a",
            "Different Song",
            "Artist",
            Some(200),
        )]));
        let r = ladder_resolver(pinned.clone(), vec![fa.clone()]);
        let err = r.resolve(&key(), &StreamRequest::Best).await.unwrap_err();
        assert!(matches!(err, SourceError::Permanent(_)), "{err:?}");
        assert_eq!(
            fa_calls.load(Ordering::SeqCst),
            0,
            "non-matching candidate never streamed"
        );
    }

    #[tokio::test(start_paused = true)]
    async fn all_fallbacks_fail_returns_the_original_error() {
        let (pinned, _) = FakeSource::new();
        pinned.push(Err(SourceError::Permanent("nope".into())));
        let (fa, _, _) = FakeSource::new_full(SourceKind::Addon, "addon-a");
        fa.push_search(Err(SourceError::Network("down".into())));
        let r = ladder_resolver(pinned.clone(), vec![fa.clone()]);
        let err = r.resolve(&key(), &StreamRequest::Best).await.unwrap_err();
        assert!(
            matches!(err, SourceError::Permanent(_)),
            "original error preserved: {err:?}"
        );
    }

    #[tokio::test(start_paused = true)]
    async fn pinned_source_in_fallback_list_is_skipped() {
        let (pinned, pinned_calls) = FakeSource::new();
        pinned.push(Err(SourceError::Permanent("nope".into())));
        let (fa, _, _) = FakeSource::new_full(SourceKind::Addon, "addon-a");
        fa.push_search(Ok(vec![catalog_row("id-a", "Song", "Artist", Some(200))]));
        fa.push(Ok(Some(media("http://fallback", None))));
        // Provider includes the pinned source itself (same config id).
        let pinned_provider = pinned.clone();
        let provider: FallbackProvider = Arc::new(move || {
            vec![
                pinned_provider.clone() as Arc<dyn MusicSource>,
                fa.clone() as Arc<dyn MusicSource>,
            ]
        });
        let meta: MetaLookup = Arc::new(|_| Some(target_meta()));
        let lookup: SourceLookup = Arc::new(move |kind| {
            (kind == SourceKind::YouTube).then(|| pinned.clone() as Arc<dyn MusicSource>)
        });
        let r = Resolver::with_fallbacks(lookup, provider, meta);
        let m = r.resolve(&key(), &StreamRequest::Best).await.unwrap();
        assert_eq!(m.url, "http://fallback");
        assert_eq!(
            pinned_calls.load(Ordering::SeqCst),
            1,
            "pinned not re-tried as fallback"
        );
    }

    #[tokio::test(start_paused = true)]
    async fn local_source_is_never_called_as_fallback() {
        let (pinned, _) = FakeSource::new();
        pinned.push(Err(SourceError::Permanent("nope".into())));
        let (local, _, local_search) = FakeSource::new_full(SourceKind::Local, "local");
        let (fa, _, _) = FakeSource::new_full(SourceKind::Addon, "addon-a");
        fa.push_search(Ok(vec![catalog_row("id-a", "Song", "Artist", Some(200))]));
        fa.push(Ok(Some(media("http://fallback", None))));
        let provider: FallbackProvider = Arc::new(move || {
            vec![
                local.clone() as Arc<dyn MusicSource>,
                fa.clone() as Arc<dyn MusicSource>,
            ]
        });
        let meta: MetaLookup = Arc::new(|_| Some(target_meta()));
        let lookup: SourceLookup = Arc::new(move |kind| {
            (kind == SourceKind::YouTube).then(|| pinned.clone() as Arc<dyn MusicSource>)
        });
        let r = Resolver::with_fallbacks(lookup, provider, meta);
        let m = r.resolve(&key(), &StreamRequest::Best).await.unwrap();
        assert_eq!(m.url, "http://fallback");
        assert_eq!(
            local_search.load(Ordering::SeqCst),
            0,
            "Local never searched"
        );
    }

    #[tokio::test(start_paused = true)]
    async fn fallbacks_are_tried_sequentially_until_one_hits() {
        let (pinned, _) = FakeSource::new();
        pinned.push(Err(SourceError::Permanent("nope".into())));
        let (fa1, _, fa1_search) = FakeSource::new_full(SourceKind::Addon, "addon-one");
        fa1.push_search(Err(SourceError::Network("down".into())));
        let (fa2, fa2_calls, fa2_search) = FakeSource::new_full(SourceKind::Addon, "addon-two");
        fa2.push_search(Ok(vec![catalog_row("id-2", "Song", "Artist", Some(200))]));
        fa2.push(Ok(Some(media("http://second", None))));
        let r = ladder_resolver(pinned.clone(), vec![fa1.clone(), fa2.clone()]);
        let m = r.resolve(&key(), &StreamRequest::Best).await.unwrap();
        assert_eq!(
            m.url, "http://second",
            "second fallback wins after first fails"
        );
        assert_eq!(m.source, SourceKind::Addon);
        assert_eq!(
            fa1_search.load(Ordering::SeqCst),
            1,
            "first fallback attempted"
        );
        assert_eq!(fa2_search.load(Ordering::SeqCst), 1);
        assert_eq!(fa2_calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test(start_paused = true)]
    async fn missing_metadata_skips_fallback_entirely() {
        let (pinned, pinned_calls) = FakeSource::new();
        pinned.push(Err(SourceError::Permanent("nope".into())));
        let (fa, _, _) = FakeSource::new_full(SourceKind::Addon, "addon-a");
        let lookup: SourceLookup = Arc::new(move |kind| {
            (kind == SourceKind::YouTube).then(|| pinned.clone() as Arc<dyn MusicSource>)
        });
        let provider: FallbackProvider = Arc::new(move || vec![fa.clone() as Arc<dyn MusicSource>]);
        let meta: MetaLookup = Arc::new(|_| None);
        let r = Resolver::with_fallbacks(lookup, provider, meta);
        let err = r.resolve(&key(), &StreamRequest::Best).await.unwrap_err();
        assert!(matches!(err, SourceError::Permanent(_)));
        assert_eq!(pinned_calls.load(Ordering::SeqCst), 1);
    }
    // Prefer-local — a local hit answers before any remote call
    // for Lossless/Best; a stale/unreadable path or no match falls through
    // to the remote flow; Capped (DataSaver) never consults local.

    fn local_meta_lookup() -> MetaLookup {
        Arc::new(|_| {
            Some(TrackMeta {
                title: "Song".into(),
                artist: "Artist".into(),
                duration_sec: Some(180.0),
            })
        })
    }

    fn sample_key() -> TrackKey {
        TrackKey {
            source: SourceKind::YouTube,
            id: "t1".into(),
        }
    }

    fn local_media(url: &str) -> ResolvedMedia {
        ResolvedMedia {
            url: url.into(),
            headers: vec![],
            expiry: None,
            length: None,
            mime: None,
            format: Some(StreamFormat {
                codec: "flac".into(),
                ..Default::default()
            }),
            container: None,
            source: SourceKind::Local,
            below_request: false,
        }
    }

    #[tokio::test]
    async fn local_hit_preferred_for_best_and_lossless() {
        // No remote sources at all: a remote path would answer Disabled, so
        // success with a local url proves the local seam answered first.
        let local: LocalMatch =
            Arc::new(|m| (m.title == "Song").then(|| local_media("/tmp/library/song.flac")));
        let r =
            Resolver::with_fallbacks(Arc::new(|_| None), Arc::new(Vec::new), local_meta_lookup())
                .with_local_match(local);
        for request in [StreamRequest::Best, StreamRequest::Lossless] {
            let media = r.resolve(&sample_key(), &request).await.unwrap();
            assert_eq!(media.source, SourceKind::Local);
            assert_eq!(media.url, "/tmp/library/song.flac");
        }
        // Capped (DataSaver tier) skips the local seam → Disabled (no remotes).
        let err = r
            .resolve(&sample_key(), &StreamRequest::Capped { max_kbps: 128 })
            .await
            .unwrap_err();
        assert!(matches!(err, SourceError::Disabled), "{err:?}");
    }

    #[tokio::test]
    async fn stale_local_path_falls_back_to_remote() {
        // The shell's closure yields None for a stale path: remote answers.
        let stale: LocalMatch = Arc::new(|_| None);
        let (fake, calls, _search) = FakeSource::new_full(SourceKind::YouTube, "yt");
        fake.push(Ok(Some(media("http://remote/1", None))));
        let fake_dyn: Arc<dyn MusicSource> = fake.clone();
        let lookup: SourceLookup = Arc::new(move |kind: SourceKind| {
            (kind == SourceKind::YouTube).then(|| fake_dyn.clone())
        });
        let r = Resolver::with_fallbacks(lookup, Arc::new(Vec::new), local_meta_lookup())
            .with_local_match(stale);
        let resolved = r
            .resolve(&sample_key(), &StreamRequest::Best)
            .await
            .unwrap();
        assert_eq!(resolved.url, "http://remote/1", "stale local → remote");
        assert_eq!(resolved.source, SourceKind::YouTube);
        assert_eq!(calls.load(Ordering::SeqCst), 1, "remote was consulted");
    }

    #[tokio::test]
    async fn no_local_match_streams_remote() {
        // No local seam injected at all — the behavior is unchanged.
        let (fake, calls, _search) = FakeSource::new_full(SourceKind::YouTube, "yt");
        fake.push(Ok(Some(media("http://remote/2", None))));
        let fake_dyn: Arc<dyn MusicSource> = fake.clone();
        let lookup: SourceLookup = Arc::new(move |kind: SourceKind| {
            (kind == SourceKind::YouTube).then(|| fake_dyn.clone())
        });
        let r = Resolver::with_fallbacks(lookup, Arc::new(Vec::new), local_meta_lookup());
        let resolved = r
            .resolve(&sample_key(), &StreamRequest::Best)
            .await
            .unwrap();
        assert_eq!(resolved.url, "http://remote/2");
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn local_hit_is_cached_like_any_answer() {
        let local: LocalMatch =
            Arc::new(|m| (m.title == "Song").then(|| local_media("/tmp/library/song.flac")));
        let r =
            Resolver::with_fallbacks(Arc::new(|_| None), Arc::new(Vec::new), local_meta_lookup())
                .with_local_match(local);
        assert!(r.resolve(&sample_key(), &StreamRequest::Best).await.is_ok());
        assert!(
            r.resolve(&sample_key(), &StreamRequest::Best).await.is_ok(),
            "cache hit"
        );
    }

    // Conformance: G2 quality tiers + G4 cache semantics

    #[tokio::test]
    async fn lossless_with_only_lossy_marks_below_request() {
        // The pinned source answers AAC (lossy) for a Lossless request:
        // the media comes back flagged below-request, and the CACHED copy
        // carries the flag too.
        let (fake, calls, _search) = FakeSource::new_full(SourceKind::YouTube, "yt");
        fake.push(Ok(Some(media("http://remote/aac", None)))); // media() builds AAC
        fake.push(Ok(Some(media("http://remote/aac", None)))); // one more for the Best resolve
        let fake_dyn: Arc<dyn MusicSource> = fake.clone();
        let lookup: SourceLookup = Arc::new(move |kind: SourceKind| {
            (kind == SourceKind::YouTube).then(|| fake_dyn.clone())
        });
        let r = Resolver::with_fallbacks(lookup, Arc::new(Vec::new), Arc::new(|_| None));
        let key = TrackKey {
            source: SourceKind::YouTube,
            id: "t".into(),
        };
        let m = r.resolve(&key, &StreamRequest::Lossless).await.unwrap();
        assert!(m.below_request, "lossy answer to Lossless is below-request");
        // Second resolve is a cache hit and still carries the flag.
        let m2 = r.resolve(&key, &StreamRequest::Lossless).await.unwrap();
        assert!(m2.below_request);
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        // Best is NOT flagged lossy or otherwise.
        let m3 = r.resolve(&key, &StreamRequest::Best).await.unwrap();
        assert!(!m3.below_request);
    }

    #[tokio::test]
    async fn url_cache_ttl_expiry_triggers_re_resolve() {
        // expiry in the past => the cache entry is dead on arrival; every
        // resolve goes upstream.
        let (fake, calls, _search) = FakeSource::new_full(SourceKind::YouTube, "yt");
        fake.push(Ok(Some(media("http://remote/1", Some(-1)))));
        fake.push(Ok(Some(media("http://remote/2", Some(-1)))));
        let fake_dyn: Arc<dyn MusicSource> = fake.clone();
        let lookup: SourceLookup = Arc::new(move |kind: SourceKind| {
            (kind == SourceKind::YouTube).then(|| fake_dyn.clone())
        });
        let r = Resolver::with_fallbacks(lookup, Arc::new(Vec::new), Arc::new(|_| None));
        let key = TrackKey {
            source: SourceKind::YouTube,
            id: "t".into(),
        };
        let m1 = r.resolve(&key, &StreamRequest::Best).await.unwrap();
        let m2 = r.resolve(&key, &StreamRequest::Best).await.unwrap();
        assert_eq!(m1.url, "http://remote/1");
        assert_eq!(m2.url, "http://remote/2", "expired entry re-resolves");
        assert_eq!(calls.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn permanent_failure_is_negative_cached() {
        let (fake, calls, _search) = FakeSource::new_full(SourceKind::YouTube, "yt");
        fake.push(Err(SourceError::Permanent("nope".into())));
        let fake_dyn: Arc<dyn MusicSource> = fake.clone();
        let lookup: SourceLookup = Arc::new(move |kind: SourceKind| {
            (kind == SourceKind::YouTube).then(|| fake_dyn.clone())
        });
        let r = Resolver::with_fallbacks(lookup, Arc::new(Vec::new), Arc::new(|_| None));
        let key = TrackKey {
            source: SourceKind::YouTube,
            id: "t".into(),
        };
        let e1 = r.resolve(&key, &StreamRequest::Best).await.unwrap_err();
        let e2 = r.resolve(&key, &StreamRequest::Best).await.unwrap_err();
        assert_eq!(e1.to_string(), e2.to_string());
        assert_eq!(
            calls.load(Ordering::SeqCst),
            1,
            "second resolve served from the negative cache"
        );
    }

    #[tokio::test]
    async fn resolved_media_reused_across_sequential_resolves() {
        // The "pinned across seek-rebuilt pipelines" property: RebuildOutput
        // re-enters prepare => resolve; the pinned cache answers without a
        // second upstream call.
        let (fake, calls, _search) = FakeSource::new_full(SourceKind::YouTube, "yt");
        fake.push(Ok(Some(media("http://remote/pinned", None))));
        let fake_dyn: Arc<dyn MusicSource> = fake.clone();
        let lookup: SourceLookup = Arc::new(move |kind: SourceKind| {
            (kind == SourceKind::YouTube).then(|| fake_dyn.clone())
        });
        let r = Resolver::with_fallbacks(lookup, Arc::new(Vec::new), Arc::new(|_| None));
        let key = TrackKey {
            source: SourceKind::YouTube,
            id: "t".into(),
        };
        for _ in 0..3 {
            let m = r.resolve(&key, &StreamRequest::Best).await.unwrap();
            assert_eq!(m.url, "http://remote/pinned");
        }
        assert_eq!(calls.load(Ordering::SeqCst), 1, "1 upstream for 3 resolves");
    }
}

#[cfg(test)]
mod decodable_resolver_tests {
    use super::*;
    use crate::{Container, StreamFormat};

    fn cand(url: &str, codec: &str, container: Option<Container>) -> ResolvedMedia {
        ResolvedMedia {
            url: url.into(),
            headers: vec![],
            expiry: None,
            length: None,
            mime: None,
            format: Some(StreamFormat {
                codec: codec.into(),
                ..Default::default()
            }),
            container,
            source: SourceKind::YouTube,
            below_request: false,
        }
    }

    #[tokio::test]
    async fn picks_best_decodable_among_candidates() {
        // Source order: Opus (best, undecodable) then AAC (decodable).
        let candidates = vec![
            cand("http://opus", "opus", None),
            cand("http://aac", "aac", None),
        ];
        let r = Resolver::new(Arc::new(|_| None));
        let picked = r
            .resolve_decodable(
                &TrackKey {
                    source: SourceKind::YouTube,
                    id: "t".into(),
                },
                &StreamRequest::Best,
                candidates,
            )
            .await
            .unwrap();
        assert_eq!(picked.url, "http://aac", "skips undecodable best");
    }

    #[tokio::test]
    async fn all_undecodable_returns_unsupported() {
        let candidates = vec![
            cand("http://hls", "aac", Some(Container::Hls)),
            cand("http://opus", "opus", None),
        ];
        let r = Resolver::new(Arc::new(|_| None));
        let err = r
            .resolve_decodable(
                &TrackKey {
                    source: SourceKind::YouTube,
                    id: "t".into(),
                },
                &StreamRequest::Best,
                candidates,
            )
            .await
            .unwrap_err();
        assert!(matches!(err, SourceError::Unsupported));
    }
}
