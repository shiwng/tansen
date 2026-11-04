//! S13: source-layer measurement counters (debug builds only, cfg-gated
//! no-op twin — never registered as IPC, release compiles the calls to
//! nothing). Mirrors the shell's `metrics.rs` M2 pattern: std statics, no
//! new deps, dumped per 10 s window under the `metrics` log target.

#[cfg(debug_assertions)]
pub mod source {
    use std::collections::HashMap;
    use std::sync::{Mutex, OnceLock};

    /// kind label → completed per-source search latencies (ms).
    fn search_lat() -> &'static Mutex<HashMap<String, Vec<f64>>> {
        static S: OnceLock<Mutex<HashMap<String, Vec<f64>>>> = OnceLock::new();
        S.get_or_init(|| Mutex::new(HashMap::new()))
    }

    #[derive(Default)]
    struct ResolveStats {
        /// Completed cold (upstream) resolve latencies, ms.
        cold_ms: Vec<f64>,
        /// Cache (warm) hits — the latency is a map lookup, counted not timed.
        warm_hits: u64,
        /// Cache misses that went upstream.
        misses: u64,
    }

    fn resolve() -> &'static Mutex<ResolveStats> {
        static R: OnceLock<Mutex<ResolveStats>> = OnceLock::new();
        R.get_or_init(|| Mutex::new(ResolveStats::default()))
    }

    /// Ladder outcome counts, by name.
    fn ladder() -> &'static Mutex<HashMap<String, u64>> {
        static L: OnceLock<Mutex<HashMap<String, u64>>> = OnceLock::new();
        L.get_or_init(|| Mutex::new(HashMap::new()))
    }

    /// One completed per-source search (ok or failed — the elapsed time is
    /// the measurement; timeouts land here too).
    pub fn search_sample(kind: &str, ms: f64) {
        if let Ok(mut map) = search_lat().lock() {
            map.entry(kind.to_string()).or_default().push(ms);
        }
    }

    /// One cold (upstream) resolve.
    pub fn resolve_cold(ms: f64) {
        if let Ok(mut s) = resolve().lock() {
            s.cold_ms.push(ms);
            s.misses += 1;
        }
    }

    /// One warm (cache) resolve hit.
    pub fn resolve_warm_hit() {
        if let Ok(mut s) = resolve().lock() {
            s.warm_hits += 1;
        }
    }

    /// One S7 fallback-ladder outcome.
    pub fn ladder_outcome(hit: bool) {
        if let Ok(mut map) = ladder().lock() {
            let key = if hit { "fallback_hit" } else { "fallback_miss" };
            *map.entry(key.to_string()).or_insert(0) += 1;
        }
    }

    /// Nearest-rank percentile of a sample list (ms), caller-sorted.
    fn pct(sorted: &[f64], p: f64) -> f64 {
        match sorted.len() {
            0 => 0.0,
            1 => sorted[0],
            n => {
                let idx = (((n as f64) * p).ceil() as usize).clamp(1, n);
                sorted[idx - 1]
            }
        }
    }

    fn fmt_lat(samples: &[f64]) -> String {
        let mut s = samples.to_vec();
        s.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        format!("n={} p50={:.0}ms p95={:.0}ms", s.len(), pct(&s, 0.50), pct(&s, 0.95))
    }

    /// Logs one line for everything that fired since the previous call.
    /// Driven by the shell's 10 s metrics thread (debug builds only).
    pub fn log_window() {
        let mut parts: Vec<String> = Vec::new();
        if let Ok(map) = search_lat().lock() {
            for (kind, samples) in map.iter() {
                if !samples.is_empty() {
                    parts.push(format!("search[{kind}] {}", fmt_lat(samples)));
                }
            }
        }
        if let Ok(s) = resolve().lock() {
            if !s.cold_ms.is_empty() || s.warm_hits > 0 {
                let total = s.cold_ms.len() as u64 + s.warm_hits;
                let rate = if total > 0 {
                    format!("{:.1}%", (s.warm_hits as f64 / total as f64) * 100.0)
                } else {
                    "n/a".into()
                };
                parts.push(format!(
                    "resolve cold:{} warm_hits:{} hit_rate:{}",
                    fmt_lat(&s.cold_ms),
                    s.warm_hits,
                    rate
                ));
            }
        }
        if let Ok(map) = ladder().lock() {
            for (k, v) in map.iter() {
                parts.push(format!("ladder {k}={v}"));
            }
        }
        if !parts.is_empty() {
            tracing::info!(target: "metrics", "{}", parts.join("; "));
        }
    }

    #[cfg(test)]
    #[allow(clippy::unwrap_used, reason = "test code")]
    mod tests {
        use super::*;

        #[test]
        fn percentiles_are_nearest_rank() {
            assert_eq!(pct(&[], 0.5), 0.0);
            assert_eq!(pct(&[5.0], 0.95), 5.0);
            let s = vec![1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0, 9.0, 10.0];
            assert_eq!(pct(&s, 0.5), 5.0, "p50 nearest-rank");
            assert_eq!(pct(&s, 0.95), 10.0, "p95 nearest-rank");
        }

        #[test]
        fn counters_accumulate_and_dump() {
            search_sample("YouTube", 100.0);
            search_sample("YouTube", 200.0);
            resolve_cold(1500.0);
            resolve_warm_hit();
            ladder_outcome(true);
            // No panic; the values land in the next log_window (checked by
            // the bench run parsing the metrics target, not here).
            log_window();
        }
    }
}

/// S13 bench helper (debug only): a permanently-failing pinned source for
/// driving fallback-ladder outcome counters.
#[cfg(debug_assertions)]
pub mod bench {
    use crate::{CatalogTrack, MusicSource, SourceError, SourceHealth, SourceKind, SourceStream, StreamRequest, TrackKey};

    pub struct BenchFailSource;
    #[async_trait::async_trait]
    impl MusicSource for BenchFailSource {
        fn config_id(&self) -> &str { "bench-fail" }
        fn kind(&self) -> SourceKind { SourceKind::YouTube }
        fn display_name(&self) -> &str { "BenchFail" }
        async fn health(&self) -> Result<SourceHealth, SourceError> {
            Ok(SourceHealth::Ok { detail: None })
        }
        async fn search(&self, _q: &str, _l: usize) -> Result<Vec<CatalogTrack>, SourceError> {
            Err(SourceError::Permanent("bench pinned failure".into()))
        }
        async fn stream(&self, _k: &TrackKey, _r: &StreamRequest) -> Result<Option<SourceStream>, SourceError> {
            Err(SourceError::Unsupported)
        }
        async fn resolve(&self, _k: &TrackKey, _r: &StreamRequest) -> Result<Option<crate::ResolvedMedia>, SourceError> {
            Err(SourceError::Permanent("bench pinned failure".into()))
        }
    }
}

#[cfg(not(debug_assertions))]
pub mod source {
    /// No-op in release builds; call sites inline to nothing.
    #[inline]
    pub fn search_sample(_kind: &str, _ms: f64) {}
    #[inline]
    pub fn resolve_cold(_ms: f64) {}
    #[inline]
    pub fn resolve_warm_hit() {}
    #[inline]
    pub fn ladder_outcome(_hit: bool) {}
    #[inline]
    pub fn log_window() {}
}
