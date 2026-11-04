//! M19: lock-free SPSC sample ring between the decode thread (producer) and
//! the audio callback (consumer). Fixed capacity (power of two), no locks,
//! no allocation after construction — the callback pops with one atomic
//! load/store pair per sample.

use std::cell::UnsafeCell;
use std::sync::atomic::{AtomicUsize, Ordering};

pub struct SpscRing {
    /// Slot storage. Shared mutability is sound because the SPSC cursors
    /// guarantee the producer and consumer never touch the same slot: the
    /// producer owns slots `[tail, head)`, the consumer owns `[head, tail)`.
    buf: UnsafeCell<Box<[f32]>>,
    /// Fixed capacity (power of two); plain data, never mutated.
    capacity: usize,
    mask: usize,
    /// Consumer cursor: number of samples ever popped.
    head: AtomicUsize,
    /// Producer cursor: number of samples ever pushed.
    tail: AtomicUsize,
}

// SAFETY: `SpscRing` is a single-producer/single-consumer queue. The
// `UnsafeCell` slots are written only by the producer at `tail & mask` and
// read only by the consumer at `head & mask`; the Release/Acquire cursor
// protocol guarantees the two never touch the same slot. Cross-thread use
// is the entire point of the type and is covered by the multi-thread test.
unsafe impl Sync for SpscRing {}

impl SpscRing {
    /// `capacity` is rounded up to the next power of two.
    pub fn new(capacity: usize) -> Self {
        let cap = capacity.max(2).next_power_of_two();
        let mut buf = Vec::with_capacity(cap);
        buf.resize(cap, 0.0);
        Self {
            buf: UnsafeCell::new(buf.into_boxed_slice()),
            capacity: cap,
            mask: cap - 1,
            head: AtomicUsize::new(0),
            tail: AtomicUsize::new(0),
        }
    }

    /// Producer: push one sample. `Err(v)` when full (caller decides: park
    /// or drop — the callback is never blocked either way).
    pub fn push(&self, v: f32) -> Result<(), f32> {
        let tail = self.tail.load(Ordering::Relaxed);
        let head = self.head.load(Ordering::Acquire);
        if tail - head >= self.capacity {
            return Err(v);
        }
        // SAFETY: the SPSC protocol gives the producer exclusive write
        // access to the slot at `tail & mask` until `tail` is published
        // (Release); the consumer only reads slots below `head`.
        unsafe {
            (*self.buf.get())[tail & self.mask] = v;
        }
        self.tail.store(tail + 1, Ordering::Release);
        Ok(())
    }

    /// Consumer: pop one sample. `None` when empty.
    pub fn pop(&self) -> Option<f32> {
        let head = self.head.load(Ordering::Relaxed);
        let tail = self.tail.load(Ordering::Acquire);
        if head == tail {
            return None;
        }
        // SAFETY: the consumer owns the slot at `head & mask` — the
        // producer has published it (Acquire) and will not rewrite it.
        let v = unsafe { (*self.buf.get())[head & self.mask] };
        self.head.store(head + 1, Ordering::Release);
        Some(v)
    }

    pub fn len(&self) -> usize {
        self.tail.load(Ordering::Acquire) - self.head.load(Ordering::Relaxed)
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, reason = "test code")]
mod tests {
    use super::*;

    #[test]
    fn push_pop_preserves_order_across_wraparound() {
        let ring = SpscRing::new(64); // rounds to 64
        // Push well past one wraparound; pop interleaved.
        let mut expected = 0u32;
        for i in 0..1000u32 {
            ring.push(i as f32).unwrap(); // 64-slot ring wraps many times
            assert_eq!(ring.pop().unwrap(), expected as f32);
            expected += 1;
        }
        while let Some(v) = ring.pop() {
            assert_eq!(v, expected as f32);
            expected += 1;
        }
        assert_eq!(expected, 1000);
    }

    #[test]
    fn full_ring_rejects_and_empty_ring_returns_none() {
        let ring = SpscRing::new(8);
        for i in 0..8 {
            ring.push(i as f32).unwrap();
        }
        assert!(matches!(ring.push(99.0), Err(99.0)), "full");
        for i in 0..8 {
            assert_eq!(ring.pop().unwrap(), i as f32);
        }
        assert!(ring.pop().is_none(), "empty");
    }

    /// The M19 contract: one producer thread, one consumer thread, every
    /// sample delivered exactly once, in order.
    #[test]
    fn spsc_across_threads_is_exact() {
        let ring = std::sync::Arc::new(SpscRing::new(1024));
        let producer = {
            let ring = ring.clone();
            std::thread::spawn(move || {
                for i in 0..200_000u32 {
                    loop {
                        if ring.push(i as f32).is_ok() {
                            break;
                        }
                        std::thread::yield_now();
                    }
                }
            })
        };
        let mut last = -1i64;
        while last < 199_999 {
            if let Some(v) = ring.pop() {
                assert_eq!(v as i64, last + 1, "in-order, no gaps");
                last = v as i64;
            } else {
                std::thread::yield_now();
            }
        }
        producer.join().unwrap();
    }
}
