//! Runtime memory accounting.
//!
//! Two different numbers matter, and they answer different questions:
//!
//! * [`TrackingAllocator`] wraps a host's global allocator and reports what
//!   the process has **live on the heap right now**, plus the high-water
//!   mark. This is the number that says whether a scene leaks.
//! * [`wasm_linear_memory_bytes`] reports how much linear memory the wasm
//!   module has **reserved from the browser**. WebAssembly memory only ever
//!   grows, so this is the peak the tab pays for regardless of how much is
//!   free inside it.
//!
//! A host opts into tracking with one line:
//!
//! ```ignore
//! #[global_allocator]
//! static ALLOC: jengine_core::profiling::memory::TrackingAllocator<std::alloc::System> =
//!     jengine_core::profiling::memory::TrackingAllocator::new(std::alloc::System);
//! ```
//!
//! Without that line the counters read zero and everything else still works
//! — nothing in the engine depends on tracking being installed.

use std::alloc::{GlobalAlloc, Layout};
use std::sync::atomic::{AtomicUsize, Ordering};

static LIVE: AtomicUsize = AtomicUsize::new(0);
static PEAK: AtomicUsize = AtomicUsize::new(0);

/// Bytes currently allocated through a [`TrackingAllocator`], or 0 when no
/// host installed one.
pub fn live_bytes() -> usize {
    LIVE.load(Ordering::Relaxed)
}

/// High-water mark of [`live_bytes`] since process start.
pub fn peak_bytes() -> usize {
    PEAK.load(Ordering::Relaxed)
}

/// Linear memory the wasm module holds, in bytes.
///
/// Returns 0 off wasm. Note this never shrinks: wasm memory grows on demand
/// and is not returned, so it tracks the peak the page is holding.
pub fn wasm_linear_memory_bytes() -> usize {
    #[cfg(target_arch = "wasm32")]
    {
        // One wasm page is 64 KiB.
        core::arch::wasm32::memory_size(0) * 65536
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        0
    }
}

/// The most meaningful "how much RAM is this using" figure for the target:
/// reserved linear memory on wasm, live heap elsewhere.
pub fn resident_bytes() -> usize {
    let wasm = wasm_linear_memory_bytes();
    if wasm > 0 { wasm } else { live_bytes() }
}

/// A `GlobalAlloc` wrapper that counts live bytes.
///
/// Delegates every operation to the inner allocator and adjusts two atomics
/// around it, so the cost is a pair of relaxed atomic ops per allocation.
/// `realloc` is forwarded rather than reimplemented so the inner allocator
/// keeps whatever growth strategy it has.
pub struct TrackingAllocator<A> {
    inner: A,
}

impl<A> TrackingAllocator<A> {
    pub const fn new(inner: A) -> Self {
        Self { inner }
    }
}

fn record_alloc(size: usize) {
    let live = LIVE.fetch_add(size, Ordering::Relaxed) + size;
    // Relaxed CAS loop: an occasional lost update only understates the peak
    // slightly, and this runs on every allocation.
    let mut peak = PEAK.load(Ordering::Relaxed);
    while live > peak {
        match PEAK.compare_exchange_weak(peak, live, Ordering::Relaxed, Ordering::Relaxed) {
            Ok(_) => break,
            Err(current) => peak = current,
        }
    }
}

unsafe impl<A: GlobalAlloc> GlobalAlloc for TrackingAllocator<A> {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let ptr = unsafe { self.inner.alloc(layout) };
        if !ptr.is_null() {
            record_alloc(layout.size());
        }
        ptr
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        LIVE.fetch_sub(layout.size(), Ordering::Relaxed);
        unsafe { self.inner.dealloc(ptr, layout) }
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        let ptr = unsafe { self.inner.alloc_zeroed(layout) };
        if !ptr.is_null() {
            record_alloc(layout.size());
        }
        ptr
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        let new_ptr = unsafe { self.inner.realloc(ptr, layout, new_size) };
        if !new_ptr.is_null() {
            if new_size >= layout.size() {
                record_alloc(new_size - layout.size());
            } else {
                LIVE.fetch_sub(layout.size() - new_size, Ordering::Relaxed);
            }
        }
        new_ptr
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tracks_live_and_peak() {
        // The test binary does not install the allocator, so drive the
        // counters directly - this is the arithmetic, not the hook.
        let start_live = live_bytes();
        let start_peak = peak_bytes();
        record_alloc(1024);
        assert_eq!(live_bytes(), start_live + 1024);
        assert!(peak_bytes() >= start_peak + 1024 || peak_bytes() >= live_bytes());
        LIVE.fetch_sub(1024, Ordering::Relaxed);
        assert_eq!(live_bytes(), start_live);
        // Peak does not come back down.
        assert!(peak_bytes() >= start_live + 1024);
    }

    #[test]
    fn resident_falls_back_to_heap_off_wasm() {
        assert_eq!(wasm_linear_memory_bytes(), 0);
        assert_eq!(resident_bytes(), live_bytes());
    }
}
