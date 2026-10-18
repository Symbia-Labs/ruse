//! Drives the wrapper through its interface from inside one process, the way a host does.
#![cfg(target_os = "macos")]

use ruse_au::check::{description, run, Report};
use ruse_au::sys::*;
use ruse_core::{BusInfo, Channels, Layout, ParamInfo, ParamSet, Plugin, ProcessContext, Setup};
use ruse_host::alloc::{count, CountingAlloc};
use std::ffi::c_void;
use std::mem::size_of;
use std::ptr::null_mut;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};

#[global_allocator]
static ALLOC: CountingAlloc = CountingAlloc::new();

ruse_au::export_au! {
    plugin: ruse_gain::Gain,
    factory: GainFactory,
}

// A plugin with a per-channel parameter, a choice and a toggle, that reports what the host told it.

static LAST_TIME: AtomicU64 = AtomicU64::new(0);
static LAST_PLAYING: AtomicBool = AtomicBool::new(false);
static LAST_TEMPO: AtomicU64 = AtomicU64::new(0);
static PREPARED_CHANNELS: AtomicU32 = AtomicU32::new(0);
static PROBE_DENORMAL: AtomicBool = AtomicBool::new(false);

/// Every test that runs the probe takes a turn, because it reports through the statics above and tests run in
/// parallel. Without this, one test's render can overwrite the sample time another is about to check.
static PROBE_TURN: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn probe_turn() -> std::sync::MutexGuard<'static, ()> {
    PROBE_TURN.lock().unwrap_or_else(|e| e.into_inner())
}

const PROBE_PARAMS: [ParamInfo; 3] = [
    ParamInfo {
        description: "Level of one channel.",
        group: "Mix",
        ..ParamInfo::knob("trim", "Trim", "dB", -24.0, 24.0, 0.0).per_channel()
    },
    ParamInfo {
        description: "Picks a flavour.",
        group: "Mix",
        ..ParamInfo::choice("flavour", "Flavour", &["Plain", "Warm", "Bright"], 1)
    },
    ParamInfo {
        description: "Passes the audio through.",
        group: "",
        ..ParamInfo::toggle("bypass", "Bypass", false)
    },
];

const PROBE_LAYOUT: Layout = Layout {
    inputs: &[BusInfo {
        name: "Main",
        channels: Channels::Range { min: 1, max: 4 },
    }],
    outputs: &[BusInfo {
        name: "Main",
        channels: Channels::SameAsInput,
    }],
};

struct Probe {
    params: ParamSet,
}

impl Default for Probe {
    fn default() -> Self {
        Probe {
            params: ParamSet::with_channels(&PROBE_PARAMS, 4),
        }
    }
}

impl Plugin for Probe {
    fn layout(&self) -> Layout {
        PROBE_LAYOUT
    }

    fn params(&self) -> &ParamSet {
        &self.params
    }

    fn prepare(&mut self, setup: &Setup) {
        PREPARED_CHANNELS.store(setup.channels, Ordering::Relaxed);
    }

    fn process(&mut self, channels: &mut [&mut [f32]], ctx: &ProcessContext) {
        LAST_TIME.store(ctx.sample_time.unwrap_or(-1.0).to_bits(), Ordering::Relaxed);
        LAST_PLAYING.store(ctx.playing, Ordering::Relaxed);
        LAST_TEMPO.store(ctx.tempo_bpm.unwrap_or(-1.0).to_bits(), Ordering::Relaxed);
        for (c, ch) in channels.iter_mut().enumerate() {
            let g = 10f32.powf(self.params.at(0).get_ch(c) / 20.0);
            for s in ch.iter_mut() {
                *s *= g;
                if PROBE_DENORMAL.load(Ordering::Relaxed) {
                    *s = std::hint::black_box(1e-38f32) * std::hint::black_box(0.1f32);
                }
            }
        }
    }
}

unsafe extern "C" fn probe_factory(desc: *const AudioComponentDescription) -> *mut PlugInInterface {
    ruse_au::wrapper::create::<Probe>(desc)
}

fn desc(subtype: &[u8; 4]) -> AudioComponentDescription {
    AudioComponentDescription {
        component_type: TYPE_EFFECT,
        component_sub_type: fourcc(subtype),
        component_manufacturer: fourcc(b"Test"),
        component_flags: 0,
        component_flags_mask: 0,
    }
}

fn failures(r: &Report) -> String {
    r.lines.iter().filter(|l| !l.1).map(|l| format!("{}: {}", l.0, l.2)).collect::<Vec<_>>().join("\n")
}

#[test]
fn the_gain_plugin_passes_every_check() {
    let mut r = Report::new(false);
    unsafe { run(GainFactory, desc(b"Gain"), &mut r) };
    assert!(r.all_passed(), "{}", failures(&r));
    assert!(r.lines.len() >= 17, "{} checks ran", r.lines.len());
}

#[test]
fn a_per_channel_plugin_with_a_choice_and_a_toggle_passes_every_check() {
    let _turn = probe_turn();
    let mut r = Report::new(false);
    unsafe { run(probe_factory, desc(b"Prob"), &mut r) };
    assert!(r.all_passed(), "{}", failures(&r));
}

#[test]
fn the_codes_from_text_make_a_description() {
    let d = description("Wsat", "Syml").unwrap();
    assert_eq!(d.component_type, TYPE_EFFECT);
    assert_eq!(fourcc_string(d.component_sub_type), "Wsat");
    assert!(description("toolong", "Syml").is_none());
    assert!(description("Wsat", "Sy").is_none());
}

// Direct calls through the interface, for what the checks do not measure.

type SetPropFn = unsafe extern "C" fn(*mut c_void, u32, u32, u32, *const c_void, u32) -> OSStatus;
type InitializeFn = unsafe extern "C" fn(*mut c_void) -> OSStatus;
type RenderFn = unsafe extern "C" fn(*mut c_void, *mut u32, *const AudioTimeStamp, u32, u32, *mut AudioBufferList) -> OSStatus;

struct Direct {
    iface: *mut PlugInInterface,
    set_prop: SetPropFn,
    initialize: InitializeFn,
    render: RenderFn,
}

static HANDLE: u8 = 0;

unsafe fn direct(factory: FactoryFunction, d: &AudioComponentDescription) -> Direct {
    let iface = factory(d);
    assert!(!iface.is_null());
    assert_eq!(((*iface).open)(iface as *mut c_void, &HANDLE as *const u8 as *mut c_void), NO_ERR);
    let find = |s: i16| ((*iface).lookup)(s).unwrap();
    Direct {
        iface,
        set_prop: std::mem::transmute::<unsafe extern "C" fn(), SetPropFn>(find(SELECT_SET_PROPERTY)),
        initialize: std::mem::transmute::<unsafe extern "C" fn(), InitializeFn>(find(SELECT_INITIALIZE)),
        render: std::mem::transmute::<unsafe extern "C" fn(), RenderFn>(find(SELECT_RENDER)),
    }
}

impl Direct {
    unsafe fn this(&self) -> *mut c_void {
        self.iface as *mut c_void
    }

    unsafe fn set<T>(&self, id: u32, scope: u32, value: &T) {
        assert_eq!(self.try_set(id, scope, 0, value), NO_ERR, "setting property {}", id);
    }

    unsafe fn try_set<T>(&self, id: u32, scope: u32, element: u32, value: &T) -> OSStatus {
        (self.set_prop)(self.this(), id, scope, element, value as *const T as *const c_void, size_of::<T>() as u32)
    }

    unsafe fn close(self) {
        ((*self.iface).close)(self.iface as *mut c_void);
    }
}

unsafe extern "C" fn constant_source(
    _ref_con: *mut c_void,
    _flags: *mut u32,
    _ts: *const AudioTimeStamp,
    _bus: u32,
    frames: u32,
    io: *mut AudioBufferList,
) -> OSStatus {
    let buffers = std::ptr::addr_of_mut!((*io).buffers) as *mut AudioBuffer;
    for c in 0..(*io).number_buffers as usize {
        let b = &*buffers.add(c);
        for i in 0..frames as usize {
            *(b.data as *mut f32).add(i) = 0.25;
        }
    }
    NO_ERR
}

struct Block {
    memory: Vec<u64>,
    data: Vec<Vec<f32>>,
}

impl Block {
    fn new(channels: usize, frames: usize) -> Block {
        let bytes = size_of::<AudioBufferList>() + (channels - 1) * size_of::<AudioBuffer>();
        let mut b = Block {
            memory: vec![0u64; bytes.div_ceil(8)],
            data: (0..channels).map(|_| vec![0.0f32; frames]).collect(),
        };
        unsafe {
            let list = b.memory.as_mut_ptr() as *mut AudioBufferList;
            (*list).number_buffers = channels as u32;
            let buffers = std::ptr::addr_of_mut!((*list).buffers) as *mut AudioBuffer;
            for (c, d) in b.data.iter_mut().enumerate() {
                *buffers.add(c) = AudioBuffer {
                    number_channels: 1,
                    data_byte_size: (frames * 4) as u32,
                    data: d.as_mut_ptr() as *mut c_void,
                };
            }
        }
        b
    }

    fn list(&mut self) -> *mut AudioBufferList {
        self.memory.as_mut_ptr() as *mut AudioBufferList
    }
}

unsafe fn ready(factory: FactoryFunction, d: &AudioComponentDescription, channels: u32) -> Direct {
    let u = direct(factory, d);
    u.set(PROP_STREAM_FORMAT, SCOPE_INPUT, &AudioStreamBasicDescription::float_planar(48000.0, channels));
    u.set(
        PROP_SET_RENDER_CALLBACK,
        SCOPE_INPUT,
        &AURenderCallbackStruct {
            input_proc: Some(constant_source),
            input_proc_ref_con: null_mut(),
        },
    );
    assert_eq!((u.initialize)(u.this()), NO_ERR);
    u
}

#[test]
fn render_does_not_allocate() {
    unsafe {
        let d = desc(b"Gain");
        let u = ready(GainFactory, &d, 2);
        let mut block = Block::new(2, 512);
        let ts = AudioTimeStamp {
            flags: TIMESTAMP_SAMPLE_TIME_VALID,
            ..Default::default()
        };
        let mut flags = 0u32;
        // One render first, so anything the first call sets up is not counted.
        assert_eq!((u.render)(u.this(), &mut flags, &ts, 0, 512, block.list()), NO_ERR);
        let before = count();
        for _ in 0..200 {
            assert_eq!((u.render)(u.this(), &mut flags, &ts, 0, 512, block.list()), NO_ERR);
        }
        assert_eq!(count() - before, 0, "render allocated");
        assert!((block.data[0][10] - 0.25).abs() < 1e-6, "unity gain by default");
        u.close();
    }
}

unsafe extern "C" fn host_transport(
    _user: *mut c_void,
    playing: *mut u8,
    changed: *mut u8,
    position: *mut f64,
    cycling: *mut u8,
    cycle_start: *mut f64,
    cycle_end: *mut f64,
) -> OSStatus {
    *playing = 1;
    *changed = 0;
    *position = 96000.0;
    *cycling = 0;
    *cycle_start = 0.0;
    *cycle_end = 0.0;
    NO_ERR
}

unsafe extern "C" fn host_tempo(_user: *mut c_void, beat: *mut f64, tempo: *mut f64) -> OSStatus {
    *beat = 4.0;
    *tempo = 123.0;
    NO_ERR
}

#[test]
fn the_hosts_transport_and_tempo_reach_the_plugin() {
    let _turn = probe_turn();
    unsafe {
        let d = desc(b"Prob");
        let u = direct(probe_factory, &d);
        u.set(PROP_STREAM_FORMAT, SCOPE_INPUT, &AudioStreamBasicDescription::float_planar(48000.0, 2));
        u.set(
            PROP_SET_RENDER_CALLBACK,
            SCOPE_INPUT,
            &AURenderCallbackStruct {
                input_proc: Some(constant_source),
                input_proc_ref_con: null_mut(),
            },
        );
        assert_eq!((u.initialize)(u.this()), NO_ERR);
        assert_eq!(PREPARED_CHANNELS.load(Ordering::Relaxed), 2);
        let mut block = Block::new(2, 128);
        let ts = AudioTimeStamp {
            sample_time: 777.0,
            flags: TIMESTAMP_SAMPLE_TIME_VALID,
            ..Default::default()
        };
        let mut flags = 0u32;

        // Without host callbacks the render clock stands in.
        assert_eq!((u.render)(u.this(), &mut flags, &ts, 0, 128, block.list()), NO_ERR);
        assert_eq!(f64::from_bits(LAST_TIME.load(Ordering::Relaxed)), 777.0);
        assert!(!LAST_PLAYING.load(Ordering::Relaxed));

        u.set(
            PROP_HOST_CALLBACKS,
            SCOPE_GLOBAL,
            &HostCallbackInfo {
                host_user_data: null_mut(),
                beat_and_tempo_proc: Some(host_tempo),
                musical_time_location_proc: None,
                transport_state_proc: Some(host_transport),
                transport_state_proc2: None,
            },
        );
        assert_eq!((u.render)(u.this(), &mut flags, &ts, 0, 128, block.list()), NO_ERR);
        assert_eq!(f64::from_bits(LAST_TIME.load(Ordering::Relaxed)), 96000.0);
        assert!(LAST_PLAYING.load(Ordering::Relaxed));
        assert_eq!(f64::from_bits(LAST_TEMPO.load(Ordering::Relaxed)), 123.0);
        u.close();
    }
}

#[test]
fn subnormal_output_is_flushed_to_zero_and_the_mode_comes_back() {
    let _turn = probe_turn();
    unsafe {
        let d = desc(b"Prob");
        let u = ready(probe_factory, &d, 2);
        let mut block = Block::new(2, 64);
        let ts = AudioTimeStamp::default();
        let mut flags = 0u32;
        PROBE_DENORMAL.store(true, Ordering::Relaxed);
        assert_eq!((u.render)(u.this(), &mut flags, &ts, 0, 64, block.list()), NO_ERR);
        PROBE_DENORMAL.store(false, Ordering::Relaxed);
        assert!(block.data[0].iter().all(|v| *v == 0.0), "the tail came out as exact zeros");
        let outside = std::hint::black_box(1e-38f32) * std::hint::black_box(0.1f32);
        assert!(outside.is_subnormal(), "the thread's own mode is unchanged");
        u.close();
    }
}

// A plugin with a sidechain that turns its input down by the sidechain's level, sample by sample.

static KEY_CHANNELS_SEEN: AtomicU32 = AtomicU32::new(u32::MAX);
static KEY_BUS_PULLED: AtomicU32 = AtomicU32::new(u32::MAX);

const DUCKER_LAYOUT: Layout = Layout {
    inputs: &[
        BusInfo {
            name: "Main",
            channels: Channels::Range { min: 1, max: 2 },
        },
        BusInfo {
            name: "Key",
            channels: Channels::Range { min: 1, max: 2 },
        },
    ],
    outputs: &[BusInfo {
        name: "Main",
        channels: Channels::SameAsInput,
    }],
};

const AMOUNT: ParamInfo = ParamInfo {
    description: "How far the sidechain ducks the input.",
    ..ParamInfo::knob("amount", "Amount", "", 0.0, 1.0, 1.0)
};

struct Ducker {
    params: ParamSet,
}

impl Default for Ducker {
    fn default() -> Self {
        Ducker {
            params: ParamSet::with_channels(&[AMOUNT], 2),
        }
    }
}

impl Plugin for Ducker {
    fn layout(&self) -> Layout {
        DUCKER_LAYOUT
    }

    fn params(&self) -> &ParamSet {
        &self.params
    }

    fn prepare(&mut self, _setup: &Setup) {}

    fn process(&mut self, _channels: &mut [&mut [f32]], _ctx: &ProcessContext) {
        KEY_CHANNELS_SEEN.store(1000, Ordering::Relaxed);
    }

    fn process_with_sidechain(&mut self, channels: &mut [&mut [f32]], sidechain: &[&[f32]], _ctx: &ProcessContext) {
        KEY_CHANNELS_SEEN.store(sidechain.len() as u32, Ordering::Relaxed);
        let amount = self.params.at(0).get();
        if let Some(key) = sidechain.first() {
            for ch in channels.iter_mut() {
                for (s, k) in ch.iter_mut().zip(key.iter()) {
                    *s *= 1.0 - amount * k.abs().min(1.0);
                }
            }
        }
    }
}

unsafe extern "C" fn ducker_factory(desc: *const AudioComponentDescription) -> *mut PlugInInterface {
    ruse_au::wrapper::create::<Ducker>(desc)
}

unsafe extern "C" fn key_source(
    _ref_con: *mut c_void,
    _flags: *mut u32,
    _ts: *const AudioTimeStamp,
    bus: u32,
    frames: u32,
    io: *mut AudioBufferList,
) -> OSStatus {
    KEY_BUS_PULLED.store(bus, Ordering::Relaxed);
    let buffers = std::ptr::addr_of_mut!((*io).buffers) as *mut AudioBuffer;
    for c in 0..(*io).number_buffers as usize {
        let b = &*buffers.add(c);
        for i in 0..frames as usize {
            *(b.data as *mut f32).add(i) = 0.5;
        }
    }
    NO_ERR
}

#[test]
fn a_sidechain_is_a_second_input_element_with_its_own_source() {
    let mut r = Report::new(false);
    unsafe { run(ducker_factory, desc(b"Duck"), &mut r) };
    assert!(r.all_passed(), "{}", failures(&r));
    unsafe {
        let d = desc(b"Duck");
        let u = direct(ducker_factory, &d);
        u.set(PROP_STREAM_FORMAT, SCOPE_INPUT, &AudioStreamBasicDescription::float_planar(48000.0, 2));
        assert_eq!(u.try_set(PROP_STREAM_FORMAT, SCOPE_INPUT, 1, &AudioStreamBasicDescription::float_planar(48000.0, 1)), NO_ERR);
        assert_eq!(
            u.try_set(PROP_STREAM_FORMAT, SCOPE_INPUT, 1, &AudioStreamBasicDescription::float_planar(48000.0, 3)),
            ERR_FORMAT_NOT_SUPPORTED,
            "a sidechain wider than the bus allows is refused"
        );
        assert_eq!(
            u.try_set(PROP_STREAM_FORMAT, SCOPE_INPUT, 2, &AudioStreamBasicDescription::float_planar(48000.0, 1)),
            ERR_INVALID_ELEMENT,
            "there is no third input"
        );
        let main = AURenderCallbackStruct {
            input_proc: Some(constant_source),
            input_proc_ref_con: null_mut(),
        };
        u.set(PROP_SET_RENDER_CALLBACK, SCOPE_INPUT, &main);
        assert_eq!((u.initialize)(u.this()), NO_ERR);
        let mut block = Block::new(2, 256);
        let ts = AudioTimeStamp::default();
        let mut flags = 0u32;

        // With nothing on the sidechain, the plugin gets none and nothing is ducked.
        assert_eq!((u.render)(u.this(), &mut flags, &ts, 0, 256, block.list()), NO_ERR);
        assert_eq!(KEY_CHANNELS_SEEN.load(Ordering::Relaxed), 0);
        assert!((block.data[0][100] - 0.25).abs() < 1e-6);

        let key = AURenderCallbackStruct {
            input_proc: Some(key_source),
            input_proc_ref_con: null_mut(),
        };
        assert_eq!(u.try_set(PROP_SET_RENDER_CALLBACK, SCOPE_INPUT, 1, &key), NO_ERR);
        let before = count();
        assert_eq!((u.render)(u.this(), &mut flags, &ts, 0, 256, block.list()), NO_ERR);
        assert_eq!(count() - before, 0, "pulling the sidechain allocated");
        assert_eq!(KEY_BUS_PULLED.load(Ordering::Relaxed), 1, "the sidechain is pulled as input 1");
        assert_eq!(KEY_CHANNELS_SEEN.load(Ordering::Relaxed), 1);
        assert!(block.data.iter().all(|c| c.iter().all(|v| (*v - 0.125).abs() < 1e-6)), "ducked by half");
        u.close();
    }
}

#[test]
fn a_description_the_plugin_was_not_made_for_still_creates_a_unit_but_not_a_null_one() {
    unsafe {
        assert!(!GainFactory(&desc(b"Anyx")).is_null());
        assert!(GainFactory(std::ptr::null()).is_null());
    }
}
