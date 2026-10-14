//! An allocator that counts, so a check can prove that `process` never allocates.
//!
//! A test binary installs it once:
//!
//! ```ignore
//! #[global_allocator]
//! static ALLOC: muse_host::alloc::CountingAlloc = muse_host::alloc::CountingAlloc::new();
//! ```
//!
//! The count belongs to the calling thread, so other threads in the test run do not disturb it.

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;

thread_local! {
    static COUNT: Cell<usize> = const { Cell::new(0) };
}

pub struct CountingAlloc;

impl CountingAlloc {
    pub const fn new() -> Self {
        CountingAlloc
    }
}

impl Default for CountingAlloc {
    fn default() -> Self {
        Self::new()
    }
}

unsafe impl GlobalAlloc for CountingAlloc {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        COUNT.with(|c| c.set(c.get() + 1));
        System.alloc(layout)
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        System.dealloc(ptr, layout)
    }
}

/// Allocations made so far on this thread.
pub fn count() -> usize {
    COUNT.with(|c| c.get())
}

/// Whether the counting allocator is the one in use.
pub fn installed() -> bool {
    let before = count();
    let probe: Vec<u8> = Vec::with_capacity(16);
    std::hint::black_box(&probe);
    count() > before
}
