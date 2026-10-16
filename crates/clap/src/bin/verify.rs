//! ruse-clap-verify: loads a built `.clap` the way a host does and exercises it.
//!
//! ```text
//! ruse-clap-verify PLUGIN.clap
//! ```
//!
//! It prints one `CHECK name PASS|FAIL detail` line per check and a total, and exits with 1 if any check failed.
//! The argument is a macOS bundle directory or a bare shared library.

use ruse_clap::abi::*;
use std::ffi::{c_char, c_int, c_void, CStr, CString};
use std::mem::size_of;
use std::path::{Path, PathBuf};
use std::ptr::{null, null_mut};
use std::sync::atomic::{AtomicU32, Ordering};

extern "C" {
    fn dlopen(path: *const c_char, flags: c_int) -> *mut c_void;
    fn dlsym(handle: *mut c_void, symbol: *const c_char) -> *mut c_void;
    fn dlclose(handle: *mut c_void) -> c_int;
    fn dlerror() -> *const c_char;
}

const RTLD_NOW: c_int = 2;

// The host the plugin is created in.

static CALLBACKS: AtomicU32 = AtomicU32::new(0);
static RESCANS: AtomicU32 = AtomicU32::new(0);
static LATENCY_CHANGES: AtomicU32 = AtomicU32::new(0);

unsafe extern "C" fn h_changed(_h: *const Host) {
    LATENCY_CHANGES.fetch_add(1, Ordering::Relaxed);
}
unsafe extern "C" fn h_rescan(_h: *const Host, _flags: u32) {
    RESCANS.fetch_add(1, Ordering::Relaxed);
}
unsafe extern "C" fn h_clear(_h: *const Host, _id: u32, _flags: u32) {}
unsafe extern "C" fn h_flush(_h: *const Host) {}
unsafe extern "C" fn h_noop(_h: *const Host) {}
unsafe extern "C" fn h_callback(_h: *const Host) {
    CALLBACKS.fetch_add(1, Ordering::Relaxed);
}

static HOST_LATENCY: HostLatency = HostLatency { changed: h_changed };
static HOST_PARAMS: HostParams = HostParams {
    rescan: h_rescan,
    clear: h_clear,
    request_flush: h_flush,
};

unsafe extern "C" fn h_extension(_h: *const Host, id: *const c_char) -> *const c_void {
    if is_id(id, EXT_LATENCY) {
        &HOST_LATENCY as *const HostLatency as *const c_void
    } else if is_id(id, EXT_PARAMS) {
        &HOST_PARAMS as *const HostParams as *const c_void
    } else {
        null()
    }
}

fn make_host() -> Host {
    Host {
        clap_version: CLAP_VERSION,
        host_data: null_mut(),
        name: c"ruse-clap-verify".as_ptr(),
        vendor: c"RUSE".as_ptr(),
        url: c"".as_ptr(),
        version: c"0.0.1".as_ptr(),
        get_extension: h_extension,
        request_restart: h_noop,
        request_process: h_noop,
        request_callback: h_callback,
    }
}

// Results.

struct Report {
    lines: Vec<(String, bool, String)>,
}

impl Report {
    /// Prints the result at once, so a crash later still leaves the checks that ran.
    fn check(&mut self, name: &str, ok: bool, detail: impl Into<String>) -> bool {
        let detail = detail.into();
        println!("CHECK {} {} {}", name, if ok { "PASS" } else { "FAIL" }, detail);
        self.lines.push((name.to_string(), ok, detail));
        ok
    }

    fn print(&self) -> bool {
        let passed = self.lines.iter().filter(|l| l.1).count();
        println!("CHECKS {} of {} passed", passed, self.lines.len());
        passed == self.lines.len()
    }
}

// Buffers and events.

struct Audio {
    data: Vec<Vec<f32>>,
    ptrs: Vec<*mut f32>,
}

impl Audio {
    fn new(channels: usize, frames: usize) -> Self {
        let mut data: Vec<Vec<f32>> = (0..channels).map(|_| vec![0.0; frames]).collect();
        let ptrs = data.iter_mut().map(|d| d.as_mut_ptr()).collect();
        Audio { data, ptrs }
    }

    fn fill_noise(&mut self, seed: u64, amplitude: f32) {
        let mut x = seed | 1;
        for ch in self.data.iter_mut() {
            for s in ch.iter_mut() {
                x ^= x >> 12;
                x ^= x << 25;
                x ^= x >> 27;
                let r = x.wrapping_mul(0x2545_F491_4F6C_DD1D);
                *s = (((r >> 40) as f32 / (1u64 << 24) as f32) * 2.0 - 1.0) * amplitude;
            }
        }
    }

    fn buffer(&mut self) -> AudioBuffer {
        AudioBuffer {
            data32: self.ptrs.as_mut_ptr(),
            data64: null_mut(),
            channel_count: self.data.len() as u32,
            latency: 0,
            constant_mask: 0,
        }
    }
}

struct Events {
    items: Vec<EventParamValue>,
}

unsafe extern "C" fn ev_size(list: *const InputEvents) -> u32 {
    let e = &*((*list).ctx as *const Events);
    e.items.len() as u32
}

unsafe extern "C" fn ev_get(list: *const InputEvents, index: u32) -> *const EventHeader {
    let e = &*((*list).ctx as *const Events);
    &e.items[index as usize].header
}

fn event_list(e: &Events) -> InputEvents {
    InputEvents {
        ctx: e as *const Events as *mut c_void,
        size: ev_size,
        get: ev_get,
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

// Helpers over the plugin.

unsafe fn ext<T>(plugin: *const Plugin, id: &[u8]) -> Option<&'static T> {
    let p = ((*plugin).get_extension)(plugin, id.as_ptr() as *const c_char) as *const T;
    if p.is_null() {
        None
    } else {
        Some(&*p)
    }
}

unsafe fn cstr(p: *const c_char) -> String {
    if p.is_null() {
        String::new()
    } else {
        CStr::from_ptr(p).to_string_lossy().to_string()
    }
}

struct Param {
    id: u32,
    name: String,
    flags: u32,
    min: f64,
    max: f64,
    default: f64,
}

unsafe fn read_params(plugin: *const Plugin, params: &PluginParams) -> Result<Vec<Param>, String> {
    let n = (params.count)(plugin);
    let mut out = Vec::new();
    for i in 0..n {
        let mut info: ParamInfo = std::mem::zeroed();
        if !(params.get_info)(plugin, i, &mut info) {
            return Err(format!("get_info failed for index {} of {}", i, n));
        }
        out.push(Param {
            id: info.id,
            name: cstr(info.name.as_ptr()),
            flags: info.flags,
            min: info.min_value,
            max: info.max_value,
            default: info.default_value,
        });
    }
    Ok(out)
}

unsafe fn process_block(
    plugin: *const Plugin,
    input: Option<&mut AudioBuffer>,
    output: &mut AudioBuffer,
    frames: usize,
    events: &InputEvents,
    steady: i64,
) -> i32 {
    let p = Process {
        steady_time: steady,
        frames_count: frames as u32,
        transport: null(),
        audio_inputs: input.as_ref().map_or(null(), |i| *i as *const AudioBuffer),
        audio_outputs: output,
        audio_inputs_count: input.is_some() as u32,
        audio_outputs_count: 1,
        in_events: events,
        out_events: null(),
    };
    ((*plugin).process)(plugin, &p)
}

fn rms_db(x: &[f32]) -> f32 {
    let s: f64 = x.iter().map(|v| (*v as f64) * (*v as f64)).sum();
    let r = (s / x.len().max(1) as f64).sqrt() as f32;
    if r <= 1e-12 {
        -240.0
    } else {
        20.0 * r.log10()
    }
}

unsafe fn port_channels(plugin: *const Plugin, ports: &PluginAudioPorts, is_input: bool) -> Option<u32> {
    let mut info: AudioPortInfo = std::mem::zeroed();
    if (ports.get)(plugin, 0, is_input, &mut info) {
        Some(info.channel_count)
    } else {
        None
    }
}

/// Runs `blocks` blocks of noise through the plugin and returns the output, in place or not.
unsafe fn run_noise(plugin: *const Plugin, channels: usize, in_place: bool, blocks: usize) -> Result<Vec<Vec<f32>>, String> {
    const FRAMES: usize = 256;
    let none = Events { items: Vec::new() };
    let list = event_list(&none);
    let mut all: Vec<Vec<f32>> = vec![Vec::new(); channels];
    let mut input = Audio::new(channels, FRAMES);
    let mut output = Audio::new(channels, FRAMES);
    for b in 0..blocks {
        input.fill_noise(1 + b as u64, 0.25);
        let status = if in_place {
            for (o, i) in output.data.iter_mut().zip(&input.data) {
                o.copy_from_slice(i);
            }
            let mut out = output.buffer();
            let mut same = AudioBuffer {
                data32: out.data32,
                data64: null_mut(),
                channel_count: channels as u32,
                latency: 0,
                constant_mask: 0,
            };
            process_block(plugin, Some(&mut same), &mut out, FRAMES, &list, (b * FRAMES) as i64)
        } else {
            let mut inb = input.buffer();
            let mut out = output.buffer();
            process_block(plugin, Some(&mut inb), &mut out, FRAMES, &list, (b * FRAMES) as i64)
        };
        if status == PROCESS_ERROR {
            return Err(format!("process returned an error on block {}", b));
        }
        for (a, o) in all.iter_mut().zip(&output.data) {
            a.extend_from_slice(o);
        }
    }
    Ok(all)
}

unsafe fn create(factory: *const PluginFactory, host: &Host, id: &CStr) -> *const Plugin {
    ((*factory).create_plugin)(factory, host, id.as_ptr())
}

unsafe fn bundle_library(path: &Path) -> Result<PathBuf, String> {
    if path.is_dir() {
        let dir = path.join("Contents/MacOS");
        let entries = std::fs::read_dir(&dir).map_err(|e| format!("{}: {}", dir.display(), e))?;
        for e in entries.flatten() {
            if e.path().is_file() {
                return Ok(e.path());
            }
        }
        return Err(format!("no executable in {}", dir.display()));
    }
    Ok(path.to_path_buf())
}

unsafe fn verify(path: &Path, r: &mut Report) {
    let lib = match bundle_library(path) {
        Ok(l) => l,
        Err(e) => {
            r.check("load", false, e);
            return;
        }
    };
    let c_lib = CString::new(lib.to_string_lossy().to_string()).unwrap();
    let handle = dlopen(c_lib.as_ptr(), RTLD_NOW);
    if handle.is_null() {
        r.check("load", false, format!("dlopen failed: {}", cstr(dlerror())));
        return;
    }
    r.check("load", true, lib.display().to_string());

    let entry_ptr = dlsym(handle, c"clap_entry".as_ptr()) as *const PluginEntry;
    if entry_ptr.is_null() {
        r.check("entry", false, "the library does not export clap_entry");
        return;
    }
    let entry = &*entry_ptr;
    let c_path = CString::new(path.to_string_lossy().to_string()).unwrap();
    let version = entry.clap_version;
    let initialised = (entry.init)(c_path.as_ptr());
    r.check(
        "entry",
        version.is_compatible() && initialised,
        format!("CLAP {}.{}.{}, init returned {}", version.major, version.minor, version.revision, initialised),
    );
    if !initialised {
        return;
    }

    let factory = (entry.get_factory)(PLUGIN_FACTORY_ID.as_ptr() as *const c_char) as *const PluginFactory;
    if factory.is_null() {
        r.check("factory", false, "get_factory returned null for clap.plugin-factory");
        return;
    }
    let count = ((*factory).get_plugin_count)(factory);
    let desc_ptr = ((*factory).get_plugin_descriptor)(factory, 0);
    if count == 0 || desc_ptr.is_null() {
        r.check("factory", false, format!("{} plugins, descriptor {}", count, if desc_ptr.is_null() { "null" } else { "present" }));
        return;
    }
    let d = &*desc_ptr;
    let mut features: Vec<String> = Vec::new();
    let mut f = d.features;
    while !f.is_null() && !(*f).is_null() && features.len() < 64 {
        features.push(cstr(*f));
        f = f.add(1);
    }
    let id = cstr(d.id);
    let well_formed = !id.is_empty() && !cstr(d.name).is_empty() && !cstr(d.vendor).is_empty() && !cstr(d.version).is_empty() && !features.is_empty() && d.clap_version.is_compatible();
    r.check(
        "factory",
        well_formed,
        format!(
            "{} plugin(s). id={} name={} vendor={} version={} features=[{}]",
            count,
            id,
            cstr(d.name),
            cstr(d.vendor),
            cstr(d.version),
            features.join(",")
        ),
    );
    if !((*factory).get_plugin_descriptor)(factory, count).is_null() {
        r.check("factory_bounds", false, "an index past the count returned a descriptor");
    }

    let host = make_host();
    let c_id = CString::new(id.clone()).unwrap();
    let plugin = create(factory, &host, &c_id);
    if plugin.is_null() {
        r.check("create", false, "create_plugin returned null for the advertised id");
        return;
    }
    let wrong = CString::new("verify.no.such.plugin").unwrap();
    let stray = create(factory, &host, &wrong);
    let initialised = ((*plugin).init)(plugin);
    r.check(
        "create",
        initialised && stray.is_null(),
        format!("init returned {}, an unknown id gives {}", initialised, if stray.is_null() { "null" } else { "a plugin" }),
    );
    if !initialised {
        return;
    }

    let ports: Option<&PluginAudioPorts> = ext(plugin, EXT_AUDIO_PORTS);
    let params_ext: Option<&PluginParams> = ext(plugin, EXT_PARAMS);
    let state_ext: Option<&PluginState> = ext(plugin, EXT_STATE);
    let configurable: Option<&PluginConfigurableAudioPorts> = ext(plugin, EXT_CONFIGURABLE_AUDIO_PORTS);
    let latency_ext: Option<&PluginLatency> = ext(plugin, EXT_LATENCY);
    let tail_ext: Option<&PluginTail> = ext(plugin, EXT_TAIL);
    let have = |o: bool| if o { "yes" } else { "no" };
    r.check(
        "extensions",
        ports.is_some() && params_ext.is_some() && state_ext.is_some(),
        format!(
            "audio-ports {}, params {}, state {}, configurable-audio-ports {}, latency {}, tail {}",
            have(ports.is_some()),
            have(params_ext.is_some()),
            have(state_ext.is_some()),
            have(configurable.is_some()),
            have(latency_ext.is_some()),
            have(tail_ext.is_some())
        ),
    );
    let (Some(ports), Some(params_ext), Some(state_ext)) = (ports, params_ext, state_ext) else {
        return;
    };

    let n_in = (ports.count)(plugin, true);
    let n_out = (ports.count)(plugin, false);
    let in_ch = port_channels(plugin, ports, true);
    let out_ch = port_channels(plugin, ports, false);
    let channels = in_ch.unwrap_or(0) as usize;
    r.check(
        "audio_ports",
        n_in == 1 && n_out == 1 && in_ch == out_ch && (1..=64).contains(&channels),
        format!("{} in, {} out, {:?} channels in and {:?} out", n_in, n_out, in_ch, out_ch),
    );
    if channels == 0 {
        return;
    }

    let params = match read_params(plugin, params_ext) {
        Ok(p) => p,
        Err(e) => {
            r.check("parameters", false, e);
            return;
        }
    };
    let mut problems: Vec<String> = Vec::new();
    for (i, p) in params.iter().enumerate() {
        if params[..i].iter().any(|q| q.id == p.id) {
            problems.push(format!("id {} appears twice", p.id));
        }
        if p.name.is_empty() {
            problems.push(format!("parameter {} has no name", p.id));
        }
        if !(p.min.is_finite() && p.max.is_finite() && p.min < p.max && p.default >= p.min && p.default <= p.max) {
            problems.push(format!("'{}' has a bad range {} to {} with default {}", p.name, p.min, p.max, p.default));
        }
        let mut v = f64::NAN;
        if !(params_ext.get_value)(plugin, p.id, &mut v) {
            problems.push(format!("get_value failed for '{}'", p.name));
        } else if (v - p.default).abs() > 1e-6 {
            problems.push(format!("'{}' starts at {} but its default is {}", p.name, v, p.default));
        }
    }
    let readonly = params.iter().filter(|p| p.flags & PARAM_IS_READONLY != 0).count();
    r.check(
        "parameters",
        problems.is_empty(),
        if problems.is_empty() {
            format!("{} parameters ({} read-only), ids unique, ranges sound, values start at their defaults", params.len(), readonly)
        } else {
            problems.join("; ")
        },
    );

    let mut text_problems: Vec<String> = Vec::new();
    for p in &params {
        let mut buf = [0 as c_char; 128];
        if !(params_ext.value_to_text)(plugin, p.id, p.default, buf.as_mut_ptr(), 128) || cstr(buf.as_ptr()).is_empty() {
            text_problems.push(format!("no text for '{}'", p.name));
            continue;
        }
        let mut back = f64::NAN;
        let shown = CString::new(cstr(buf.as_ptr())).unwrap();
        if !(params_ext.text_to_value)(plugin, p.id, shown.as_ptr(), &mut back) {
            text_problems.push(format!("'{}' cannot read back its own text {:?}", p.name, shown));
        } else if (back - p.default).abs() > (p.max - p.min) * 0.01 {
            text_problems.push(format!("'{}' text {:?} reads back as {} not {}", p.name, shown, back, p.default));
        }
    }
    r.check(
        "parameter_text",
        text_problems.is_empty(),
        if text_problems.is_empty() {
            "every parameter's text reads back to its value".to_string()
        } else {
            text_problems.join("; ")
        },
    );

    let activated = ((*plugin).activate)(plugin, 48000.0, 32, 512);
    let started = activated && ((*plugin).start_processing)(plugin);
    r.check("activate", started, format!("activate and start_processing returned {} and {}", activated, started));
    if !started {
        return;
    }

    // In place and out of place give the same audio from two fresh instances.
    let second = create(factory, &host, &c_id);
    let same_ok = !second.is_null() && ((*second).init)(second) && ((*second).activate)(second, 48000.0, 32, 512) && ((*second).start_processing)(second);
    let a = run_noise(plugin, channels, true, 8);
    let b = if same_ok { run_noise(second, channels, false, 8) } else { Err("could not create a second instance".into()) };
    match (&a, &b) {
        (Ok(a), Ok(b)) => {
            let finite = a.iter().chain(b.iter()).all(|c| c.iter().all(|v| v.is_finite()));
            let level_in = 20.0 * (0.25f32 / 3f32.sqrt()).log10();
            let level_out = rms_db(&a[0]);
            r.check("process_finite", finite, format!("8 blocks of 256 frames on {} channels, channel 1 reads {:+.1} dB against {:+.1} dB in", channels, level_out, level_in));
            let identical = a.iter().zip(b.iter()).all(|(x, y)| x.iter().zip(y).all(|(p, q)| (p - q).abs() <= 1e-6));
            r.check("in_place_matches_out_of_place", identical, if identical { "the same audio either way" } else { "in-place and out-of-place output differ" });
        }
        (Err(e), _) | (_, Err(e)) => {
            r.check("process_finite", false, e.clone());
        }
    }
    if same_ok {
        ((*second).stop_processing)(second);
        ((*second).deactivate)(second);
    }

    // A parameter event moves the value the host reads back.
    let writable: Vec<&Param> = params.iter().filter(|p| p.flags & PARAM_IS_READONLY == 0).collect();
    if let Some(p) = writable.first() {
        let events = Events { items: vec![param_event(p.id, p.max)] };
        let list = event_list(&events);
        let mut audio = Audio::new(channels, 64);
        let mut out = audio.buffer();
        let status = process_block(plugin, None, &mut out, 64, &list, 99999);
        let mut v = f64::NAN;
        let read = (params_ext.get_value)(plugin, p.id, &mut v);
        r.check(
            "parameter_events",
            status != PROCESS_ERROR && read && (v - p.max).abs() < 1e-6,
            format!("'{}' was sent {} and reads back {}", p.name, p.max, v),
        );
        let calls = CALLBACKS.load(Ordering::Relaxed);
        ((*plugin).on_main_thread)(plugin);
        r.check("main_thread_callback", calls > 0, format!("the plugin asked the host for a main-thread call {} time(s) after a parameter changed", calls));
    } else {
        r.check("parameter_events", true, "the plugin has no writable parameters");
    }

    let latency = latency_ext.map(|l| (l.get)(plugin));
    let tail = tail_ext.map(|t| (t.get)(plugin));
    r.check("latency_tail", true, format!("latency {:?} samples, tail {:?} samples", latency, tail));

    // State survives a round trip into a fresh instance.
    {
        unsafe extern "C" fn write(stream: *const OStream, buffer: *const c_void, size: u64) -> i64 {
            let v = &mut *((*stream).ctx as *mut Vec<u8>);
            let n = (size as usize).min(64);
            v.extend_from_slice(std::slice::from_raw_parts(buffer as *const u8, n));
            n as i64
        }
        struct Reader {
            bytes: Vec<u8>,
            at: usize,
        }
        unsafe extern "C" fn read(stream: *const IStream, buffer: *mut c_void, size: u64) -> i64 {
            let r = &mut *((*stream).ctx as *mut Reader);
            let n = (size as usize).min(64).min(r.bytes.len() - r.at);
            std::ptr::copy_nonoverlapping(r.bytes.as_ptr().add(r.at), buffer as *mut u8, n);
            r.at += n;
            n as i64
        }
        let mut saved: Vec<u8> = Vec::new();
        let out = OStream {
            ctx: &mut saved as *mut Vec<u8> as *mut c_void,
            write,
        };
        let saved_ok = (state_ext.save)(plugin, &out);
        let fresh = create(factory, &host, &c_id);
        let mut loaded_ok = false;
        let mut same = true;
        if !fresh.is_null() && ((*fresh).init)(fresh) {
            let mut reader = Reader { bytes: saved.clone(), at: 0 };
            let input = IStream {
                ctx: &mut reader as *mut Reader as *mut c_void,
                read,
            };
            loaded_ok = (state_ext.load)(fresh, &input);
            // Meters are read-only output, which the state leaves out, so only settings are compared.
            for p in &writable {
                let (mut x, mut y) = (f64::NAN, f64::NAN);
                (params_ext.get_value)(plugin, p.id, &mut x);
                (params_ext.get_value)(fresh, p.id, &mut y);
                if (x - y).abs() > 1e-6 {
                    same = false;
                }
            }
            let mut junk = Reader { bytes: vec![0xAB; 40], at: 0 };
            let junk_in = IStream {
                ctx: &mut junk as *mut Reader as *mut c_void,
                read,
            };
            let rejected = !(state_ext.load)(fresh, &junk_in);
            same = same && rejected;
            ((*fresh).destroy)(fresh);
        }
        r.check(
            "state",
            saved_ok && loaded_ok && same,
            format!("saved {} bytes, loaded {}, values match {}, garbage is refused", saved.len(), loaded_ok, same),
        );
    }

    // Reconfigure the channel count while the plugin is stopped.
    ((*plugin).stop_processing)(plugin);
    ((*plugin).deactivate)(plugin);
    if let Some(cfg) = configurable {
        let ask = |n: u32| AudioPortConfigurationRequest {
            is_input: true,
            port_index: 0,
            channel_count: n,
            port_type: null(),
            port_details: null(),
        };
        let refuses_zero = !(cfg.can_apply_configuration)(plugin, &ask(0), 1);
        let refuses_many = !(cfg.can_apply_configuration)(plugin, &ask(65), 1);
        let target = if channels == 1 { 2 } else { 1 };
        let can = (cfg.can_apply_configuration)(plugin, &ask(target), 1);
        let applied = can && (cfg.apply_configuration)(plugin, &ask(target), 1);
        let now = port_channels(plugin, ports, true);
        let reread = read_params(plugin, params_ext);
        let mut ok = refuses_zero && refuses_many && applied && now == Some(target) && reread.is_ok();
        let mut detail = format!(
            "refuses 0 and 65 channels {}, applied {} channels {}, ports now report {:?}",
            refuses_zero && refuses_many,
            target,
            applied,
            now
        );
        if ok {
            if ((*plugin).activate)(plugin, 44100.0, 32, 256) && ((*plugin).start_processing)(plugin) {
                match run_noise(plugin, target as usize, true, 4) {
                    Ok(out) => {
                        let finite = out.iter().all(|c| c.iter().all(|v| v.is_finite()));
                        ok = finite;
                        detail.push_str(&format!(", {} blocks processed on {} channels at 44100 Hz", 4, target));
                    }
                    Err(e) => {
                        ok = false;
                        detail.push_str(&format!(", {}", e));
                    }
                }
                ((*plugin).stop_processing)(plugin);
                ((*plugin).deactivate)(plugin);
            } else {
                ok = false;
                detail.push_str(", could not activate after reconfiguring");
            }
        }
        r.check("configurable_ports", ok, detail);
    } else {
        r.check("configurable_ports", true, "not offered, so the channel count stays at the default");
    }

    ((*plugin).destroy)(plugin);
    (entry.deinit)();
    let closed = dlclose(handle) == 0;
    r.check("unload", closed, "destroy, deinit and dlclose completed");
}

fn main() {
    let Some(path) = std::env::args().nth(1) else {
        eprintln!("usage: ruse-clap-verify PLUGIN.clap");
        std::process::exit(2);
    };
    let mut report = Report { lines: Vec::new() };
    unsafe { verify(Path::new(&path), &mut report) };
    let ok = report.print();
    std::process::exit(if ok { 0 } else { 1 });
}
