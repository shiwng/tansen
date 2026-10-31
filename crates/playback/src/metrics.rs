//! Audio reliability metrics (M4). Debug builds only — release compiles the
//! no-op half of this module and the engine takes the un-instrumented path.
//!
//! What is measurable through rodio 0.21:
//! - **Stream errors**: rodio forwards cpal's error callback through
//!   `OutputStreamBuilder::with_error_callback`. Counted here. Note: CoreAudio
//!   does not report buffer underruns as cpal stream errors, so a zero here
//!   means "no device-level failure", not "provably glitchless".
//! - **Source-read latency**: rodio owns the cpal data callback and exposes
//!   no timing for it. The nearest measurable proxy is how long our decoders
//!   take per `next()` call on rodio's feeder thread — that is the latency
//!   we control and the thing that, if it exceeds the buffer, causes real
//!   underruns. Recorded as a coarse ns histogram.

use std::time::Duration;

#[cfg(debug_assertions)]
pub mod audio {
    use super::Duration;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::sync::{Mutex, OnceLock};

    /// Histogram bucket upper bounds in nanoseconds. The last bucket is open.
    const BUCKETS_NS: [u64; 6] =
        [1_000, 10_000, 100_000, 1_000_000, 10_000_000, u64::MAX];
    const BUCKET_LABELS: [&str; 6] =
        ["<1µs", "1–10µs", "10–100µs", "0.1–1ms", "1–10ms", ">10ms"];

    pub struct AudioMetrics {
        pub stream_errors: AtomicU64,
        pub last_error: Mutex<String>,
        pub reads: AtomicU64,
        pub buckets: [AtomicU64; 6],
        pub slowest_ns: AtomicU64,
    }

    static METRICS: OnceLock<AudioMetrics> = OnceLock::new();

    pub fn metrics() -> &'static AudioMetrics {
        METRICS.get_or_init(|| AudioMetrics {
            stream_errors: AtomicU64::new(0),
            last_error: Mutex::new(String::new()),
            reads: AtomicU64::new(0),
            buckets: Default::default(),
            slowest_ns: AtomicU64::new(0),
        })
    }

    /// One cpal stream error (device loss, backend failure). Not an underrun
    /// signal on macOS — CoreAudio does not surface underruns this way.
    pub fn record_stream_error(err: &str) {
        let m = metrics();
        m.stream_errors.fetch_add(1, Ordering::Relaxed);
        if let Ok(mut last) = m.last_error.lock() {
            *last = err.to_owned();
        }
    }

    /// One decoder `next()` call, timed on rodio's feeder thread.
    pub fn record_read(d: Duration) {
        let m = metrics();
        m.reads.fetch_add(1, Ordering::Relaxed);
        let ns = d.as_nanos().min(u128::from(u64::MAX)) as u64;
        for (i, bound) in BUCKETS_NS.iter().enumerate() {
            if ns < *bound {
                m.buckets[i].fetch_add(1, Ordering::Relaxed);
                break;
            }
        }
        m.slowest_ns.fetch_max(ns, Ordering::Relaxed);
    }

    /// One-line summary for the periodic logger. Percentiles are quantized to
    /// histogram buckets — the honest resolution this proxy affords.
    pub fn summary() -> String {
        let m = metrics();
        let errors = m.stream_errors.load(Ordering::Relaxed);
        let reads = m.reads.load(Ordering::Relaxed);
        let counts: Vec<u64> = m.buckets.iter().map(|b| b.load(Ordering::Relaxed)).collect();
        let slowest = m.slowest_ns.load(Ordering::Relaxed);
        let last = m
            .last_error
            .lock()
            .ok()
            .map(|l| l.clone())
            .unwrap_or_default();

        let mut out = format!("stream_errors={errors} reads={reads} slowest={:.1}ms", slowest as f64 / 1e6);
        if errors > 0 && !last.is_empty() {
            out.push_str(&format!(" last_error=“{last}”"));
        }
        if reads > 0 {
            for pct in [50.0, 99.0] {
                let mut cumulative = 0u64;
                let mut found: Option<usize> = None;
                for (i, c) in counts.iter().enumerate() {
                    cumulative += c;
                    let rank = (pct / 100.0 * reads as f64).ceil() as u64;
                    if cumulative >= rank.max(1) {
                        found = Some(i);
                        break;
                    }
                }
                if let Some(i) = found {
                    out.push_str(&format!(" p{}={}", pct as u64, BUCKET_LABELS[i]));
                }
            }
        }
        out.push_str(" [");
        for (i, c) in counts.iter().enumerate() {
            if *c > 0 {
                out.push_str(&format!("{}={c} ", BUCKET_LABELS[i]));
            }
        }
        out.push(']');
        out
    }

    /// Wraps a decoder so every `next()` is timed, and (debug-only stress
    /// hook) optionally sleeps mid-track: `TANSEN_STALL_MS` per every
    /// `TANSEN_STALL_EVERY`-th sample (both env vars, default off).
    /// The sleep lands on rodio's feeder thread, starving the real output
    /// buffer — the closest a wrapper gets to a genuine mid-track stall.
    pub struct MeasuredSource {
        inner: Box<dyn rodio::Source<Item = f32> + Send>,
        stall_ms: u64,
        stall_every: u64,
        count: u64,
    }

    impl MeasuredSource {
        pub fn wrap(inner: Box<dyn rodio::Source<Item = f32> + Send>) -> Self {
            let stall_ms = std::env::var("TANSEN_STALL_MS")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(0);
            let stall_every = std::env::var("TANSEN_STALL_EVERY")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(10_000);
            Self { inner, stall_ms, stall_every, count: 0 }
        }
    }

    impl Iterator for MeasuredSource {
        type Item = f32;

        fn next(&mut self) -> Option<f32> {
            // The stall (if any) is inside the timed window on purpose: a
            // delayed read is exactly what starves the output buffer.
            let t = std::time::Instant::now();
            if self.stall_ms > 0 {
                self.count = self.count.wrapping_add(1);
                if self.count % self.stall_every.max(1) == 0 {
                    std::thread::sleep(Duration::from_millis(self.stall_ms));
                }
            }
            let sample = self.inner.next();
            record_read(t.elapsed());
            sample
        }
    }

    impl rodio::Source for MeasuredSource {
        fn current_span_len(&self) -> Option<usize> {
            self.inner.current_span_len()
        }
        fn channels(&self) -> u16 {
            self.inner.channels()
        }
        fn sample_rate(&self) -> u32 {
            self.inner.sample_rate()
        }
        fn total_duration(&self) -> Option<Duration> {
            self.inner.total_duration()
        }
    }

    /// Engine entry point: instruments a decoder in debug builds.
    pub fn wrap_source(
        source: Box<dyn rodio::Source<Item = f32> + Send>,
    ) -> Box<dyn rodio::Source<Item = f32> + Send> {
        Box::new(MeasuredSource::wrap(source))
    }
}

#[cfg(not(debug_assertions))]
pub mod audio {
    /// Release: pass the decoder through untouched.
    pub fn wrap_source(
        source: Box<dyn rodio::Source<Item = f32> + Send>,
    ) -> Box<dyn rodio::Source<Item = f32> + Send> {
        source
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, reason = "test code")]
mod tests {
    use super::audio;

    #[test]
    fn histogram_buckets_and_percentiles() {
        // Simulated reads: 9 fast, 1 slow (5 ms).
        for _ in 0..9 {
            audio::record_read(std::time::Duration::from_nanos(500));
        }
        audio::record_read(std::time::Duration::from_millis(5));
        let s = audio::summary();
        assert!(s.contains("reads=10"), "{s}");
        assert!(s.contains("slowest=5.0ms"), "{s}");
        assert!(s.contains("<1µs=9"), "{s}");
        assert!(s.contains("1–10ms=1"), "{s}");
        // p50 lands in the fast bucket, p99 (rank 10) in the 1–10ms bucket.
        assert!(s.contains("p50=<1µs"), "{s}");
        assert!(s.contains("p99=1–10ms"), "{s}");
    }
}
