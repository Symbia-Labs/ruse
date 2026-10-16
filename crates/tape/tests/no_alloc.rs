use ruse_core::{Plugin, ProcessContext, Setup};
use ruse_tape::BiasTape;
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
fn process_does_not_allocate_at_24_channels_even_when_a_new_config_arrives() {
    let mut p = BiasTape::new();
    p.prepare(&Setup {
        sample_rate: 44100.0,
        max_block: 512,
        channels: 24,
    });
    let mut bufs: Vec<Vec<f32>> = (0..24)
        .map(|c| {
            (0..128)
                .map(|i| 0.2 * (i as f32 * 0.05 * (1.0 + c as f32 * 0.1)).sin())
                .collect()
        })
        .collect();
    let ctx = ProcessContext::offline(44100.0);

    // Warm up, then change a setting on this thread, outside the counted region. The new
    // config is built here; the audio side only picks it up.
    for _ in 0..4 {
        let mut chans: Vec<&mut [f32]> = bufs.iter_mut().map(|b| &mut b[..]).collect();
        p.process(&mut chans, &ctx);
    }
    p.params().set_plain("speed", 3.0).unwrap();
    let mut chans: Vec<&mut [f32]> = bufs.iter_mut().map(|b| &mut b[..]).collect();

    let before = COUNT.with(|c| c.get());
    for _ in 0..50 {
        p.process(&mut chans, &ctx);
    }
    let after = COUNT.with(|c| c.get());

    assert_eq!(after - before, 0, "process allocated");
}
