//! Drives the wrapper through its C function tables, the way a host does, from inside one process.

use ruse_clap::abi::*;
use ruse_core::{BusInfo, Channels, Layout, ParamInfo as RuseParamInfo, ParamSet, Plugin as RusePlugin, ProcessContext, Setup};
use ruse_host::alloc::{count, CountingAlloc};
use std::ffi::{c_char, c_void, CStr, CString};
use std::mem::size_of;
use std::ptr::{null, null_mut};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};

#[global_allocator]
static ALLOC: CountingAlloc = CountingAlloc::new();

ruse_clap::export_clap! {
    plugin: ruse_gain::Gain,
    id: "test.ruse.gain",
    name: "Gain",
    vendor: "RUSE tests",
    url: "https://example.invalid",
    version: "0.0.1",
    description: "Level change.",
    features: ["audio-effect", "utility"],
}

// A plugin with a per-channel parameter that reports what the host told it.

static LAST_TIME: AtomicU64 = AtomicU64::new(0);
static LAST_PLAYING: AtomicBool = AtomicBool::new(false);
static LAST_TEMPO: AtomicU64 = AtomicU64::new(0);
static PREPARED_CHANNELS: AtomicU32 = AtomicU32::new(0);
static PROBE_LATENCY: AtomicU32 = AtomicU32::new(0);
static PROBE_DENORMAL: AtomicBool = AtomicBool::new(false);

const TRIM: RuseParamInfo = RuseParamInfo {
    description: "Level of one channel.",
    group: "Mix",
    ..RuseParamInfo::knob("trim", "Trim", "dB", -24.0, 24.0, 0.0).per_channel()
};

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
            params: ParamSet::with_channels(&[TRIM], 4),
        }
    }
}

impl RusePlugin for Probe {
    fn layout(&self) -> Layout {
        PROBE_LAYOUT
    }

    fn params(&self) -> &ParamSet {
        &self.params
    }

    fn prepare(&mut self, setup: &Setup) {
        PREPARED_CHANNELS.store(setup.channels, Ordering::Relaxed);
    }

    fn latency_samples(&self) -> u32 {
        PROBE_LATENCY.load(Ordering::Relaxed)
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
                    // A decaying tail that lands just under the smallest normal number.
                    *s = std::hint::black_box(1e-38f32) * std::hint::black_box(0.1f32);
                }
            }
        }
    }
}

static PROBE_CONFIG: ruse_clap::Config = ruse_clap::Config {
    id: "test.ruse.probe",
    name: "Probe",
    vendor: "RUSE tests",
    url: "",
    version: "0.0.1",
    description: "Reports what the host said.",
    features: &["audio-effect"],
};
static PROBE_REGISTRY: ruse_clap::Registry = ruse_clap::Registry::new(&PROBE_CONFIG);

// A host.

static CALLBACKS: AtomicU32 = AtomicU32::new(0);
static LATENCY_CHANGED: AtomicU32 = AtomicU32::new(0);
static RESCANS: AtomicU32 = AtomicU32::new(0);

unsafe extern "C" fn host_changed(_host: *const Host) {
    LATENCY_CHANGED.fetch_add(1, Ordering::Relaxed);
}

unsafe extern "C" fn host_rescan(_host: *const Host, _flags: u32) {
    RESCANS.fetch_add(1, Ordering::Relaxed);
}

unsafe extern "C" fn host_clear(_host: *const Host, _id: u32, _flags: u32) {}

unsafe extern "C" fn host_flush(_host: *const Host) {}

static HOST_LATENCY: HostLatency = HostLatency { changed: host_changed };
static HOST_PARAMS: HostParams = HostParams {
    rescan: host_rescan,
    clear: host_clear,
    request_flush: host_flush,
};

unsafe extern "C" fn host_extension(_host: *const Host, id: *const c_char) -> *const c_void {
    if is_id(id, EXT_LATENCY) {
        &HOST_LATENCY as *const HostLatency as *const c_void
    } else if is_id(id, EXT_PARAMS) {
        &HOST_PARAMS as *const HostParams as *const c_void
    } else {
        null()
    }
}

unsafe extern "C" fn host_noop(_host: *const Host) {}

unsafe extern "C" fn host_callback(_host: *const Host) {
    CALLBACKS.fetch_add(1, Ordering::Relaxed);
}

fn test_host() -> Host {
    Host {
        clap_version: CLAP_VERSION,
        host_data: null_mut(),
        name: c"test host".as_ptr(),
        vendor: c"tests".as_ptr(),
        url: c"".as_ptr(),
        version: c"1".as_ptr(),
        get_extension: host_extension,
        request_restart: host_noop,
        request_process: host_noop,
        request_callback: host_callback,
    }
}

// Events and buffers.

struct Events {
    items: Vec<EventParamValue>,
}

unsafe extern "C" fn events_size(list: *const InputEvents) -> u32 {
    (*((*list).ctx as *const Events)).items.len() as u32
}

unsafe extern "C" fn events_get(list: *const InputEvents, index: u32) -> *const EventHeader {
    let events = &*((*list).ctx as *const Events);
    &events.items[index as usize].header
}

fn input_events(events: &Events) -> InputEvents {
    InputEvents {
        ctx: events as *const Events as *mut c_void,
        size: events_size,
        get: events_get,
    }
}

fn param_event(id: u32, value: f64) -> EventParamValue {
    EventParamValue {
        header: EventHeader {
            size: size_of::<EventParamValue>() as u32,
            time: 0,
            space_id: EVENT_SPACE_CORE,
            type_: EVENT_PARAM_VALUE,
            flags: 0,
        },
        param_id: id,
        cookie: null_mut(),
        note_id: -1,
        port_index: -1,
        channel: -1,
        key: -1,
        value,
    }
}

/// An empty list. The events sit in a box so the list's pointer to them survives the move out of here.
fn no_events() -> (Box<Events>, InputEvents) {
    let e = Box::new(Events { items: Vec::new() });
    let i = input_events(&e);
    (e, i)
}

struct Buffers {
    data: Vec<Vec<f32>>,
    ptrs: Vec<*mut f32>,
}

impl Buffers {
    fn new(channels: usize, frames: usize, value: f32) -> Self {
        let mut data: Vec<Vec<f32>> = (0..channels).map(|_| vec![value; frames]).collect();
        let ptrs = data.iter_mut().map(|d| d.as_mut_ptr()).collect();
        Buffers { data, ptrs }
    }

    fn audio(&mut self) -> AudioBuffer {
        AudioBuffer {
            data32: self.ptrs.as_mut_ptr(),
            data64: null_mut(),
            channel_count: self.data.len() as u32,
            latency: 0,
            constant_mask: 0,
        }
    }
}

unsafe fn process(
    plugin: *const Plugin,
    input: Option<&mut AudioBuffer>,
    output: &mut AudioBuffer,
    frames: usize,
    events: &InputEvents,
    transport: *const EventTransport,
    steady: i64,
) -> i32 {
    let p = Process {
        steady_time: steady,
        frames_count: frames as u32,
        transport,
        audio_inputs: input.as_ref().map_or(null(), |i| *i as *const AudioBuffer),
        audio_outputs: output,
        audio_inputs_count: input.is_some() as u32,
        audio_outputs_count: 1,
        in_events: events,
        out_events: null(),
    };
    ((*plugin).process)(plugin, &p)
}

unsafe fn create_gain(host: &Host) -> *const Plugin {
    let entry = &__ruse_clap_export::clap_entry;
    assert!((entry.init)(c"test".as_ptr()));
    let factory = (entry.get_factory)(PLUGIN_FACTORY_ID.as_ptr() as *const c_char) as *const PluginFactory;
    assert!(!factory.is_null());
    let id = CString::new("test.ruse.gain").unwrap();
    let plugin = ((*factory).create_plugin)(factory, host, id.as_ptr());
    assert!(!plugin.is_null());
    assert!(((*plugin).init)(plugin));
    plugin
}

unsafe fn create_probe(host: &Host) -> *const Plugin {
    let id = CString::new("test.ruse.probe").unwrap();
    let plugin = ruse_clap::wrapper::create_plugin::<Probe>(&PROBE_REGISTRY, host, id.as_ptr());
    assert!(!plugin.is_null());
    assert!(((*plugin).init)(plugin));
    plugin
}

unsafe fn ext<T>(plugin: *const Plugin, id: &[u8]) -> &'static T {
    let p = ((*plugin).get_extension)(plugin, id.as_ptr() as *const c_char) as *const T;
    assert!(!p.is_null(), "missing extension {:?}", CStr::from_bytes_with_nul(id).unwrap());
    &*p
}

unsafe fn param_info(plugin: *const Plugin, index: u32) -> ParamInfo {
    let params: &PluginParams = ext(plugin, EXT_PARAMS);
    let mut info: ParamInfo = std::mem::zeroed();
    assert!((params.get_info)(plugin, index, &mut info));
    info
}

unsafe fn text(buf: &[c_char]) -> String {
    CStr::from_ptr(buf.as_ptr()).to_string_lossy().to_string()
}

fn rms(x: &[f32]) -> f32 {
    (x.iter().map(|v| v * v).sum::<f32>() / x.len() as f32).sqrt()
}

#[test]
fn the_descriptor_and_factory_describe_the_plugin() {
    unsafe {
        let entry = &__ruse_clap_export::clap_entry;
        assert!(entry.clap_version.is_compatible());
        assert!((entry.init)(null()));
        assert!((entry.get_factory)(c"clap.something-else".as_ptr()).is_null());
        let factory = (entry.get_factory)(PLUGIN_FACTORY_ID.as_ptr() as *const c_char) as *const PluginFactory;
        assert_eq!(((*factory).get_plugin_count)(factory), 1);
        assert!(((*factory).get_plugin_descriptor)(factory, 1).is_null());
        let d = &*((*factory).get_plugin_descriptor)(factory, 0);
        assert_eq!(CStr::from_ptr(d.id).to_str().unwrap(), "test.ruse.gain");
        assert_eq!(CStr::from_ptr(d.name).to_str().unwrap(), "Gain");
        assert_eq!(CStr::from_ptr(d.vendor).to_str().unwrap(), "RUSE tests");
        assert_eq!(CStr::from_ptr(d.version).to_str().unwrap(), "0.0.1");
        let mut features = Vec::new();
        let mut f = d.features;
        while !(*f).is_null() {
            features.push(CStr::from_ptr(*f).to_str().unwrap().to_string());
            f = f.add(1);
        }
        assert_eq!(features, ["audio-effect", "utility"]);

        let host = test_host();
        let wrong = CString::new("not.this.one").unwrap();
        assert!(((*factory).create_plugin)(factory, &host, wrong.as_ptr()).is_null());
        let id = CString::new("test.ruse.gain").unwrap();
        assert!(((*factory).create_plugin)(factory, null(), id.as_ptr()).is_null());
    }
}

#[test]
fn the_extensions_a_host_needs_are_there() {
    unsafe {
        let host = test_host();
        let plugin = create_gain(&host);
        for id in [EXT_AUDIO_PORTS, EXT_CONFIGURABLE_AUDIO_PORTS, EXT_PARAMS, EXT_STATE, EXT_LATENCY, EXT_TAIL] {
            let _: &c_void = ext(plugin, id);
        }
        assert!(((*plugin).get_extension)(plugin, c"clap.gui".as_ptr()).is_null());
        ((*plugin).destroy)(plugin);
    }
}

#[test]
fn the_audio_port_is_stereo_by_default_and_follows_the_configuration() {
    unsafe {
        let host = test_host();
        let plugin = create_gain(&host);
        let ports: &PluginAudioPorts = ext(plugin, EXT_AUDIO_PORTS);
        assert_eq!((ports.count)(plugin, true), 1);
        assert_eq!((ports.count)(plugin, false), 1);
        let mut info: AudioPortInfo = std::mem::zeroed();
        assert!((ports.get)(plugin, 0, true, &mut info));
        assert_eq!(info.channel_count, 2);
        assert_eq!(CStr::from_ptr(info.port_type).to_str().unwrap(), "stereo");
        assert_eq!(info.flags & AUDIO_PORT_IS_MAIN, AUDIO_PORT_IS_MAIN);
        assert!(!(ports.get)(plugin, 1, true, &mut info));

        let configurable: &PluginConfigurableAudioPorts = ext(plugin, EXT_CONFIGURABLE_AUDIO_PORTS);
        let ask = |n: u32, index: u32| AudioPortConfigurationRequest {
            is_input: true,
            port_index: index,
            channel_count: n,
            port_type: null(),
            port_details: null(),
        };
        assert!((configurable.can_apply_configuration)(plugin, &ask(6, 0), 1));
        assert!(!(configurable.can_apply_configuration)(plugin, &ask(65, 0), 1));
        assert!(!(configurable.can_apply_configuration)(plugin, &ask(0, 0), 1));
        assert!(!(configurable.can_apply_configuration)(plugin, &ask(2, 1), 1));
        let mixed = [ask(2, 0), ask(4, 0)];
        assert!(!(configurable.can_apply_configuration)(plugin, mixed.as_ptr(), 2));

        assert!((configurable.apply_configuration)(plugin, &ask(6, 0), 1));
        assert!((ports.get)(plugin, 0, false, &mut info));
        assert_eq!(info.channel_count, 6);
        assert!(info.port_type.is_null());

        // Once active, the plugin refuses to change its ports.
        assert!(((*plugin).activate)(plugin, 48000.0, 1, 512));
        assert!(!(configurable.can_apply_configuration)(plugin, &ask(2, 0), 1));
        ((*plugin).deactivate)(plugin);
        ((*plugin).destroy)(plugin);
    }
}

#[test]
fn parameters_describe_themselves_and_convert_text() {
    unsafe {
        let host = test_host();
        let plugin = create_gain(&host);
        let params: &PluginParams = ext(plugin, EXT_PARAMS);
        assert_eq!((params.count)(plugin), 1);
        let info = param_info(plugin, 0);
        assert_eq!(text(&info.name), "Gain");
        assert_eq!(text(&info.module), "Output");
        assert_eq!((info.min_value, info.max_value, info.default_value), (-60.0, 12.0, 0.0));
        assert_eq!(info.flags & PARAM_IS_AUTOMATABLE, PARAM_IS_AUTOMATABLE);
        assert_eq!(info.flags & PARAM_IS_STEPPED, 0);
        let mut out = [0 as c_char; 64];
        assert!((params.value_to_text)(plugin, info.id, 6.0, out.as_mut_ptr(), 64));
        assert_eq!(text(&out), "6.00 dB");
        let mut v = 0.0;
        let s = CString::new("-12.5 dB").unwrap();
        assert!((params.text_to_value)(plugin, info.id, s.as_ptr(), &mut v));
        assert_eq!(v, -12.5);
        let junk = CString::new("loud").unwrap();
        assert!(!(params.text_to_value)(plugin, info.id, junk.as_ptr(), &mut v));
        assert!(!(params.get_info)(plugin, 1, &mut std::mem::zeroed()));
        assert!(!(params.get_value)(plugin, 999, &mut v));
        // A buffer too small for the text gets a cut, zero-terminated string.
        let mut tiny = [0x55 as c_char; 4];
        assert!((params.value_to_text)(plugin, info.id, 6.0, tiny.as_mut_ptr(), 4));
        assert_eq!(text(&tiny), "6.0");
        ((*plugin).destroy)(plugin);
    }
}

#[test]
fn a_parameter_event_changes_the_audio_in_place_and_out_of_place() {
    unsafe {
        let host = test_host();
        let plugin = create_gain(&host);
        assert!(((*plugin).activate)(plugin, 48000.0, 1, 512));
        let id = param_info(plugin, 0).id;
        let events = Events {
            items: vec![param_event(id, 6.0)],
        };
        let list = input_events(&events);
        let want = 0.25 * 10f32.powf(6.0 / 20.0);

        // In place.
        let mut buf = Buffers::new(2, 256, 0.25);
        let mut out = buf.audio();
        let mut same = AudioBuffer {
            data32: out.data32,
            data64: null_mut(),
            channel_count: 2,
            latency: 0,
            constant_mask: 0,
        };
        assert_eq!(process(plugin, Some(&mut same), &mut out, 256, &list, null(), -1), PROCESS_CONTINUE);
        for ch in &buf.data {
            assert!((ch[0] - want).abs() < 1e-6 && (ch[255] - want).abs() < 1e-6);
        }

        // Out of place, with no new events.
        let (_keep, none) = no_events();
        let mut input = Buffers::new(2, 128, 0.5);
        let mut output = Buffers::new(2, 128, 0.0);
        let mut ia = input.audio();
        let mut oa = output.audio();
        assert_eq!(process(plugin, Some(&mut ia), &mut oa, 128, &none, null(), -1), PROCESS_CONTINUE);
        assert!((output.data[1][100] - 0.5 * 10f32.powf(6.0 / 20.0)).abs() < 1e-6);
        assert_eq!(input.data[0][0], 0.5, "the input is left alone");

        // The wrapper asked the host for a main-thread call, so the plugin's listener can run there.
        assert!(CALLBACKS.load(Ordering::Relaxed) > 0);
        ((*plugin).on_main_thread)(plugin);

        let params: &PluginParams = ext(plugin, EXT_PARAMS);
        let mut v = 0.0;
        assert!((params.get_value)(plugin, id, &mut v));
        assert_eq!(v, 6.0);
        ((*plugin).deactivate)(plugin);
        ((*plugin).destroy)(plugin);
    }
}

#[test]
fn a_block_with_the_wrong_channel_count_is_an_error_not_a_crash() {
    unsafe {
        let host = test_host();
        let plugin = create_gain(&host);
        assert!(((*plugin).activate)(plugin, 48000.0, 1, 512));
        let (_k, none) = no_events();
        let mut buf = Buffers::new(3, 64, 0.1);
        let mut out = buf.audio();
        assert_eq!(process(plugin, None, &mut out, 64, &none, null(), -1), PROCESS_ERROR);
        ((*plugin).destroy)(plugin);
    }
}

#[test]
fn process_does_not_allocate_even_with_events_and_a_transport() {
    unsafe {
        let host = test_host();
        let plugin = create_gain(&host);
        assert!(((*plugin).activate)(plugin, 48000.0, 1, 512));
        let id = param_info(plugin, 0).id;
        let events = Events {
            items: vec![param_event(id, -3.0), param_event(id, 3.0)],
        };
        let list = input_events(&events);
        let mut buf = Buffers::new(2, 512, 0.2);
        let mut out = buf.audio();
        let mut input = Buffers::new(2, 512, 0.2);
        let mut ia = input.audio();
        let transport: EventTransport = std::mem::zeroed();

        let before = count();
        for i in 0..200 {
            let status = process(plugin, Some(&mut ia), &mut out, 512, &list, &transport, i * 512);
            assert_eq!(status, PROCESS_CONTINUE);
        }
        let after = count();
        assert_eq!(after - before, 0, "process allocated");
        ((*plugin).destroy)(plugin);
    }
}

#[test]
fn state_saves_and_restores_every_value() {
    unsafe {
        unsafe extern "C" fn write(stream: *const OStream, buffer: *const c_void, size: u64) -> i64 {
            let v = &mut *((*stream).ctx as *mut Vec<u8>);
            // Take at most 5 bytes at a time, as a host with a small buffer would.
            let n = (size as usize).min(5);
            v.extend_from_slice(std::slice::from_raw_parts(buffer as *const u8, n));
            n as i64
        }
        struct Reader {
            bytes: Vec<u8>,
            at: usize,
        }
        unsafe extern "C" fn read(stream: *const IStream, buffer: *mut c_void, size: u64) -> i64 {
            let r = &mut *((*stream).ctx as *mut Reader);
            let n = (size as usize).min(7).min(r.bytes.len() - r.at);
            std::ptr::copy_nonoverlapping(r.bytes.as_ptr().add(r.at), buffer as *mut u8, n);
            r.at += n;
            n as i64
        }

        let host = test_host();
        let a = create_gain(&host);
        let id = param_info(a, 0).id;
        let params: &PluginParams = ext(a, EXT_PARAMS);
        let events = Events {
            items: vec![param_event(id, -9.5)],
        };
        params_flush_once(a, &events);

        let state: &PluginState = ext(a, EXT_STATE);
        let mut saved: Vec<u8> = Vec::new();
        let out = OStream {
            ctx: &mut saved as *mut Vec<u8> as *mut c_void,
            write,
        };
        assert!((state.save)(a, &out));
        assert_eq!(&saved[..4], b"RUSE");

        let b = create_gain(&host);
        let mut reader = Reader { bytes: saved.clone(), at: 0 };
        let input = IStream {
            ctx: &mut reader as *mut Reader as *mut c_void,
            read,
        };
        assert!((state.load)(b, &input));
        let mut v = 0.0;
        assert!((params.get_value)(b, id, &mut v));
        assert_eq!(v, -9.5);

        let mut bad = Reader {
            bytes: b"NOPE0000".to_vec(),
            at: 0,
        };
        let bad_in = IStream {
            ctx: &mut bad as *mut Reader as *mut c_void,
            read,
        };
        assert!(!(state.load)(b, &bad_in));
        let mut cut = Reader {
            bytes: saved[..10].to_vec(),
            at: 0,
        };
        let cut_in = IStream {
            ctx: &mut cut as *mut Reader as *mut c_void,
            read,
        };
        assert!(!(state.load)(b, &cut_in));
        ((*a).destroy)(a);
        ((*b).destroy)(b);
    }
}

unsafe fn params_flush_once(plugin: *const Plugin, events: &Events) {
    let params: &PluginParams = ext(plugin, EXT_PARAMS);
    let list = input_events(events);
    (params.flush)(plugin, &list, null());
}

#[test]
fn per_channel_parameters_become_one_clap_parameter_for_each_channel() {
    unsafe {
        let host = test_host();
        let plugin = create_probe(&host);
        let params: &PluginParams = ext(plugin, EXT_PARAMS);
        assert_eq!((params.count)(plugin), 2, "stereo by default");
        let first = param_info(plugin, 0);
        let second = param_info(plugin, 1);
        assert_eq!(text(&first.name), "Trim 1");
        assert_eq!(text(&second.name), "Trim 2");
        assert_eq!(text(&second.module), "Mix/Channel 2");
        assert_ne!(first.id, second.id);

        // Four channels give four parameters, and the ids of the first two stay put.
        let configurable: &PluginConfigurableAudioPorts = ext(plugin, EXT_CONFIGURABLE_AUDIO_PORTS);
        let ask = AudioPortConfigurationRequest {
            is_input: true,
            port_index: 0,
            channel_count: 4,
            port_type: null(),
            port_details: null(),
        };
        let rescans = RESCANS.load(Ordering::Relaxed);
        assert!((configurable.apply_configuration)(plugin, &ask, 1));
        assert!(RESCANS.load(Ordering::Relaxed) > rescans, "the host is told to reread the parameters");
        assert_eq!((params.count)(plugin), 4);
        assert_eq!(param_info(plugin, 0).id, first.id);
        assert_eq!(param_info(plugin, 1).id, second.id);

        assert!(((*plugin).activate)(plugin, 44100.0, 1, 256));
        assert_eq!(PREPARED_CHANNELS.load(Ordering::Relaxed), 4);

        // A write to one channel's parameter reaches that channel only.
        let events = Events {
            items: vec![param_event(second.id, -6.0)],
        };
        let list = input_events(&events);
        let mut buf = Buffers::new(4, 64, 0.5);
        let mut out = buf.audio();
        assert_eq!(process(plugin, None, &mut out, 64, &list, null(), -1), PROCESS_CONTINUE);
        // With no input the plugin sees silence, so the gain has nothing to scale. Check the value instead.
        let mut v = 0.0;
        assert!((params.get_value)(plugin, second.id, &mut v));
        assert_eq!(v, -6.0);
        assert!((params.get_value)(plugin, first.id, &mut v));
        assert_eq!(v, 0.0);

        let mut input = Buffers::new(4, 64, 0.5);
        let mut ia = input.audio();
        let mut output = Buffers::new(4, 64, 0.0);
        let mut oa = output.audio();
        let (_k, none) = no_events();
        assert_eq!(process(plugin, Some(&mut ia), &mut oa, 64, &none, null(), -1), PROCESS_CONTINUE);
        assert!((rms(&output.data[0]) - 0.5).abs() < 1e-6);
        assert!((rms(&output.data[1]) - 0.5 * 10f32.powf(-6.0 / 20.0)).abs() < 1e-5);
        assert!((rms(&output.data[3]) - 0.5).abs() < 1e-6);

        // A global write to a per-channel id that does not exist is ignored.
        let stray = Events {
            items: vec![param_event(9, 12.0)],
        };
        params_flush_once(plugin, &stray);
        ((*plugin).destroy)(plugin);
    }
}

#[cfg(any(target_arch = "aarch64", target_arch = "x86_64"))]
#[test]
fn subnormal_output_is_flushed_to_zero_and_the_mode_comes_back() {
    unsafe {
        let host = test_host();
        let plugin = create_probe(&host);
        assert!(((*plugin).activate)(plugin, 48000.0, 1, 256));
        let (_k, none) = no_events();
        let mut input = Buffers::new(2, 64, 0.5);
        let mut ia = input.audio();
        let mut output = Buffers::new(2, 64, 0.0);
        let mut oa = output.audio();
        PROBE_DENORMAL.store(true, Ordering::Relaxed);
        assert_eq!(process(plugin, Some(&mut ia), &mut oa, 64, &none, null(), -1), PROCESS_CONTINUE);
        PROBE_DENORMAL.store(false, Ordering::Relaxed);
        assert!(output.data[0].iter().all(|v| *v == 0.0), "the tail came out as exact zeros");
        // Outside the call the thread's own mode is unchanged, so the same product is subnormal again.
        let outside = std::hint::black_box(1e-38f32) * std::hint::black_box(0.1f32);
        assert!(outside.is_subnormal());
        ((*plugin).destroy)(plugin);
    }
}

#[test]
fn loading_state_tells_the_host_to_reread_the_values() {
    unsafe {
        unsafe extern "C" fn read(stream: *const IStream, buffer: *mut c_void, size: u64) -> i64 {
            let (bytes, at) = &mut *((*stream).ctx as *mut (Vec<u8>, usize));
            let n = (size as usize).min(bytes.len() - *at);
            std::ptr::copy_nonoverlapping(bytes.as_ptr().add(*at), buffer as *mut u8, n);
            *at += n;
            n as i64
        }
        let host = test_host();
        let plugin = create_probe(&host);
        let state: &PluginState = ext(plugin, EXT_STATE);
        let mut data = (b"RUSE".to_vec(), 0usize);
        data.0.extend_from_slice(&1u32.to_le_bytes());
        data.0.extend_from_slice(&0u32.to_le_bytes());
        let stream = IStream {
            ctx: &mut data as *mut (Vec<u8>, usize) as *mut c_void,
            read,
        };
        let before = RESCANS.load(Ordering::Relaxed);
        assert!((state.load)(plugin, &stream));
        assert!(RESCANS.load(Ordering::Relaxed) > before);
        ((*plugin).destroy)(plugin);
    }
}

#[test]
fn the_transport_reaches_the_plugin_as_a_position_in_samples() {
    unsafe {
        let host = test_host();
        let plugin = create_probe(&host);
        assert!(((*plugin).activate)(plugin, 48000.0, 1, 256));
        let (_k, none) = no_events();
        let mut buf = Buffers::new(2, 64, 0.1);
        let mut out = buf.audio();

        // No transport: the steady clock stands in.
        process(plugin, None, &mut out, 64, &none, null(), 1234);
        assert_eq!(f64::from_bits(LAST_TIME.load(Ordering::Relaxed)), 1234.0);
        assert!(!LAST_PLAYING.load(Ordering::Relaxed));

        let mut t: EventTransport = std::mem::zeroed();
        t.flags = TRANSPORT_HAS_TEMPO | TRANSPORT_HAS_SECONDS_TIMELINE | TRANSPORT_IS_PLAYING;
        t.song_pos_seconds = (2.5 * SECTIME_FACTOR) as i64;
        t.tempo = 123.0;
        process(plugin, None, &mut out, 64, &none, &t, 99999);
        assert_eq!(f64::from_bits(LAST_TIME.load(Ordering::Relaxed)), 2.5 * 48000.0);
        assert!(LAST_PLAYING.load(Ordering::Relaxed));
        assert_eq!(f64::from_bits(LAST_TEMPO.load(Ordering::Relaxed)), 123.0);
        ((*plugin).destroy)(plugin);
    }
}

#[test]
fn a_change_of_latency_reaches_the_host_on_its_main_thread() {
    unsafe {
        let host = test_host();
        PROBE_LATENCY.store(0, Ordering::Relaxed);
        let plugin = create_probe(&host);
        assert!(((*plugin).activate)(plugin, 48000.0, 1, 256));
        let latency: &PluginLatency = ext(plugin, EXT_LATENCY);
        assert_eq!((latency.get)(plugin), 0);

        PROBE_LATENCY.store(96, Ordering::Relaxed);
        let (_k, none) = no_events();
        let mut buf = Buffers::new(2, 64, 0.1);
        let mut out = buf.audio();
        let calls = CALLBACKS.load(Ordering::Relaxed);
        let changed = LATENCY_CHANGED.load(Ordering::Relaxed);
        process(plugin, None, &mut out, 64, &none, null(), -1);
        assert!(CALLBACKS.load(Ordering::Relaxed) > calls, "the audio thread asked for a callback");
        assert_eq!(LATENCY_CHANGED.load(Ordering::Relaxed), changed, "nothing is announced from the audio thread");
        ((*plugin).on_main_thread)(plugin);
        assert_eq!(LATENCY_CHANGED.load(Ordering::Relaxed), changed + 1);
        assert_eq!((latency.get)(plugin), 96);
        ((*plugin).on_main_thread)(plugin);
        assert_eq!(LATENCY_CHANGED.load(Ordering::Relaxed), changed + 1, "announced once");
        PROBE_LATENCY.store(0, Ordering::Relaxed);
        ((*plugin).destroy)(plugin);
    }
}
