use muse_core::{Plugin, ProcessContext, Setup};
use muse_gain::Gain;
use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;

thread_local! {
    static COUNT: Cell<usize> = const { Cell::new(0) };
}

struct Counting;

unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        COUNT.with(|c| c.set(c.get() + 1));
        System.alloc(layout)
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        System.dealloc(ptr, layout)
    }
}

#[global_allocator]
static ALLOC: Counting = Counting;

#[test]
fn process_does_not_allocate_at_24_channels() {
    let mut p = Gain::new();
    p.prepare(&Setup {
        sample_rate: 48000.0,
        max_block: 512,
        channels: 24,
    });
    let mut bufs: Vec<Vec<f32>> = (0..24).map(|_| vec![0.25_f32; 512]).collect();
    let mut chans: Vec<&mut [f32]> = bufs.iter_mut().map(|b| &mut b[..]).collect();
    let ctx = ProcessContext::offline(48000.0);

    let before = COUNT.with(|c| c.get());
    for _ in 0..100 {
        p.process(&mut chans, &ctx);
    }
    let after = COUNT.with(|c| c.get());

    assert_eq!(after - before, 0, "process allocated");
}
