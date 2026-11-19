//! Per-command IPC call counters (instrumentation).
//!
//! Debug builds only: `hit()` compiles to a no-op in release, so the invoke
//! wrapper costs nothing there. std atomics + a mutex'd map — no new deps.

#[cfg(debug_assertions)]
pub mod cmd {
    use std::collections::HashMap;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::sync::{Arc, Mutex, OnceLock};

    fn counts() -> &'static Mutex<HashMap<String, Arc<AtomicU64>>> {
        static COUNTS: OnceLock<Mutex<HashMap<String, Arc<AtomicU64>>>> = OnceLock::new();
        COUNTS.get_or_init(|| Mutex::new(HashMap::new()))
    }

    /// Counts one invocation of the named command. Called once per IPC invoke
    /// from the `invoke_handler` wrapper in main.rs.
    pub fn hit(name: &str) {
        if let Ok(mut map) = counts().lock() {
            if let Some(c) = map.get(name) {
                c.fetch_add(1, Ordering::Relaxed);
            } else {
                map.insert(name.to_owned(), Arc::new(AtomicU64::new(1)));
            }
        }
        // A poisoned or contended mutex here costs one counter tick, nothing else.
    }

    /// Logs one line for every command that fired since the previous call.
    /// Driven by a 10 s thread spawned in main() (debug builds only).
    pub fn log_window() {
        static LAST: OnceLock<Mutex<HashMap<String, u64>>> = OnceLock::new();
        let last = LAST.get_or_init(|| Mutex::new(HashMap::new()));
        let mut line = String::new();
        if let Ok(map) = counts().lock() {
            let mut names: Vec<&String> = map.keys().collect();
            names.sort();
            for name in names {
                let total = map
                    .get(name)
                    .map(|c| c.load(Ordering::Relaxed))
                    .unwrap_or(0);
                let prev = last
                    .lock()
                    .ok()
                    .and_then(|l| l.get(name).copied())
                    .unwrap_or(0);
                if total > prev {
                    let delta = total.saturating_sub(prev);
                    if !line.is_empty() {
                        line.push_str("; ");
                    }
                    line.push_str(&format!("{name} +{delta} (total {total})"));
                }
            }
            if let Ok(mut l) = last.lock() {
                for (name, total) in map.iter() {
                    l.insert(name.clone(), total.load(Ordering::Relaxed));
                }
            }
        }
        if !line.is_empty() {
            tracing::info!(target: "metrics", "{line}");
        }
    }
}

#[cfg(not(debug_assertions))]
pub mod cmd {
    /// No-op in release builds; the invoke wrapper call inlines to nothing.
    #[inline]
    pub fn hit(_name: &str) {}
}
