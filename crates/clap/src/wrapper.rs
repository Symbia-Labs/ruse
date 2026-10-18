//! Wraps any RUSE plugin as a CLAP plugin.
//!
//! Threading follows the CLAP rules. The audio thread alone runs `process` on the plugin. The main thread
//! never touches the plugin while it is processing. It works through a shared handle to the plugin's
//! `ParamSet` and through atomics the audio thread keeps up to date, such as latency and tail.
//!
//! The wrapper exposes the plugin's first input bus as one CLAP audio port in and one out. Parameters
//! that are per-channel appear as one CLAP parameter for each channel.

use crate::abi::*;
use ruse_core::{
    Channels, Kind, Layout, ParamInfo as RuseParamInfo, ParamSet, Plugin as RusePlugin, ProcessContext, Scope, Setup,
};
use std::cell::UnsafeCell;
use std::ffi::{c_char, c_void, CStr, CString};
use std::mem::size_of;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::ptr::{null, null_mut};
use std::sync::atomic::{AtomicBool, AtomicPtr, AtomicU32, AtomicU64, Ordering};
use std::sync::OnceLock;

/// The most channels a wrapped plugin can have.
const MAX_CHANNELS: usize = 64;
/// The most sidechain channels passed on to the plugin.
const MAX_SIDECHAIN: usize = 8;

/// What the host shows for the plugin.
pub struct Config {
    pub id: &'static str,
    pub name: &'static str,
    pub vendor: &'static str,
    pub url: &'static str,
    pub version: &'static str,
    pub description: &'static str,
    pub features: &'static [&'static str],
}

/// The C descriptor and the strings it points into.
pub struct Descriptor {
    raw: PluginDescriptor,
    _strings: Vec<CString>,
    _features: Vec<*const c_char>,
}

// The raw pointers point into the CStrings and the Vec held alongside them. Neither changes after construction.
unsafe impl Send for Descriptor {}
unsafe impl Sync for Descriptor {}

fn c_string(s: &str) -> CString {
    CString::new(s.replace('\0', "")).unwrap_or_default()
}

impl Descriptor {
    fn new(cfg: &Config) -> Box<Descriptor> {
        let mut strings: Vec<CString> = [cfg.id, cfg.name, cfg.vendor, cfg.url, cfg.version, cfg.description, ""]
            .iter()
            .map(|s| c_string(s))
            .collect();
        let features: Vec<CString> = cfg.features.iter().map(|f| c_string(f)).collect();
        let mut feature_ptrs: Vec<*const c_char> = features.iter().map(|f| f.as_ptr()).collect();
        feature_ptrs.push(null());
        let raw = {
            let p = |i: usize| strings[i].as_ptr();
            PluginDescriptor {
                clap_version: CLAP_VERSION,
                id: p(0),
                name: p(1),
                vendor: p(2),
                url: p(3),
                manual_url: p(6),
                support_url: p(6),
                version: p(4),
                description: p(5),
                features: feature_ptrs.as_ptr(),
            }
        };
        strings.extend(features);
        Box::new(Descriptor {
            raw,
            _strings: strings,
            _features: feature_ptrs,
        })
    }
}

/// One exported plugin: its configuration and the descriptor built from it.
pub struct Registry {
    cfg: &'static Config,
    descriptor: OnceLock<Box<Descriptor>>,
}

impl Registry {
    pub const fn new(cfg: &'static Config) -> Self {
        Registry {
            cfg,
            descriptor: OnceLock::new(),
        }
    }

    fn descriptor(&self) -> &PluginDescriptor {
        &self.descriptor.get_or_init(|| Descriptor::new(self.cfg)).raw
    }
}

/// # Safety
/// Called by a host through the entry point.
pub unsafe fn entry_init(reg: &Registry, _plugin_path: *const c_char) -> bool {
    reg.descriptor();
    true
}

/// # Safety
/// `factory_id` must be null or a NUL-terminated string.
pub unsafe fn entry_get_factory(factory_id: *const c_char, factory: &'static PluginFactory) -> *const c_void {
    if is_id(factory_id, PLUGIN_FACTORY_ID) {
        factory as *const PluginFactory as *const c_void
    } else {
        null()
    }
}

pub fn factory_descriptor(reg: &Registry, index: u32) -> *const PluginDescriptor {
    if index == 0 {
        reg.descriptor() as *const PluginDescriptor
    } else {
        null()
    }
}

/// Builds the CLAP plugin for the configured id.
///
/// # Safety
/// `host` and `plugin_id` must be null or valid for the duration of the call.
pub unsafe fn create_plugin<P: RusePlugin + Default + 'static>(
    reg: &'static Registry,
    host: *const Host,
    plugin_id: *const c_char,
) -> *const Plugin {
    if host.is_null() || plugin_id.is_null() || !(*host).clap_version.is_compatible() {
        return null();
    }
    if CStr::from_ptr(plugin_id).to_bytes() != reg.cfg.id.as_bytes() {
        return null();
    }
    let inner: Box<dyn Inner> = Box::new(P::default());
    Wrapper::create(reg.descriptor(), host, inner)
}

/// What the wrapper needs from a plugin, with the plugin's own type erased.
trait Inner: Send {
    fn layout(&self) -> Layout;
    fn params(&self) -> ParamSet;
    fn prepare(&mut self, setup: &Setup);
    fn process(&mut self, channels: &mut [&mut [f32]], ctx: &ProcessContext);
    fn process_with_sidechain(&mut self, channels: &mut [&mut [f32]], sidechain: &[&[f32]], ctx: &ProcessContext);
    fn latency(&self) -> u32;
    fn tail(&self) -> u32;
}

impl<P: RusePlugin> Inner for P {
    fn layout(&self) -> Layout {
        RusePlugin::layout(self)
    }
    fn params(&self) -> ParamSet {
        RusePlugin::params(self).clone()
    }
    fn prepare(&mut self, setup: &Setup) {
        RusePlugin::prepare(self, setup)
    }
    fn process(&mut self, channels: &mut [&mut [f32]], ctx: &ProcessContext) {
        RusePlugin::process(self, channels, ctx)
    }
    fn process_with_sidechain(&mut self, channels: &mut [&mut [f32]], sidechain: &[&[f32]], ctx: &ProcessContext) {
        RusePlugin::process_with_sidechain(self, channels, sidechain, ctx)
    }
    fn latency(&self) -> u32 {
        self.latency_samples()
    }
    fn tail(&self) -> u32 {
        self.tail_samples()
    }
}

fn bounds(layout: &Layout) -> (u32, u32) {
    let top = MAX_CHANNELS as u32;
    match layout.inputs.first().map(|b| b.channels) {
        Some(Channels::Fixed(n)) => (n.clamp(1, top), n.clamp(1, top)),
        Some(Channels::Range { min, max }) => (min.clamp(1, top), max.clamp(min.clamp(1, top), top)),
        _ => (1, top),
    }
}

#[repr(C)]
struct Wrapper {
    // First, so a pointer to the wrapper is also a pointer to the CLAP plugin.
    clap: Plugin,
    host: *const Host,
    // Touched by the audio thread while processing, and by the main thread only while it is not.
    inner: UnsafeCell<Box<dyn Inner>>,
    // Set up before processing and used only by the audio thread, so it never allocates while running.
    slices: UnsafeCell<Vec<&'static mut [f32]>>,
    params: ParamSet,
    min_channels: u32,
    max_channels: u32,
    /// The layout, kept so the sidechain's channel count can follow the main bus's.
    layout: Layout,
    channels: AtomicU32,
    sample_rate: AtomicU64,
    max_frames: AtomicU32,
    active: AtomicBool,
    latency: AtomicU32,
    announced_latency: AtomicU32,
    tail: AtomicU32,
    dirty: AtomicBool,
    host_params: AtomicPtr<HostParams>,
    host_latency: AtomicPtr<HostLatency>,
}

// The raw pointers are the host's, valid for the plugin's life. The UnsafeCells follow the thread rules above.
unsafe impl Send for Wrapper {}
unsafe impl Sync for Wrapper {}

unsafe fn wrapper<'a>(plugin: *const Plugin) -> &'a Wrapper {
    &*((*plugin).plugin_data as *const Wrapper)
}

impl Wrapper {
    unsafe fn create(desc: *const PluginDescriptor, host: *const Host, inner: Box<dyn Inner>) -> *const Plugin {
        let layout = inner.layout();
        let (min, max) = bounds(&layout);
        let channels = if (min..=max).contains(&2) { 2 } else { min };
        let params = inner.params();
        params.set_channels(channels as usize);
        let latency = inner.latency();
        let tail = inner.tail();
        let w = Box::new(Wrapper {
            clap: Plugin {
                desc,
                plugin_data: null_mut(),
                init: plugin_init,
                destroy: plugin_destroy,
                activate: plugin_activate,
                deactivate: plugin_deactivate,
                start_processing: plugin_start_processing,
                stop_processing: plugin_stop_processing,
                reset: plugin_reset,
                process: plugin_process,
                get_extension: plugin_get_extension,
                on_main_thread: plugin_on_main_thread,
            },
            host,
            inner: UnsafeCell::new(inner),
            slices: UnsafeCell::new(Vec::with_capacity(MAX_CHANNELS)),
            params,
            min_channels: min,
            max_channels: max,
            layout,
            channels: AtomicU32::new(channels),
            sample_rate: AtomicU64::new(48000f64.to_bits()),
            max_frames: AtomicU32::new(4096),
            active: AtomicBool::new(false),
            latency: AtomicU32::new(latency),
            announced_latency: AtomicU32::new(latency),
            tail: AtomicU32::new(tail),
            dirty: AtomicBool::new(false),
            host_params: AtomicPtr::new(null_mut()),
            host_latency: AtomicPtr::new(null_mut()),
        });
        let raw = Box::into_raw(w);
        (*raw).clap.plugin_data = raw as *mut c_void;
        &(*raw).clap
    }

    fn channel_count(&self) -> usize {
        self.channels.load(Ordering::Relaxed) as usize
    }

    /// Asks the host for a call on its main thread. Safe from any thread.
    unsafe fn request_callback(&self) {
        if !self.host.is_null() {
            ((*self.host).request_callback)(self.host);
        }
    }

    unsafe fn mark_dirty(&self) {
        self.dirty.store(true, Ordering::Release);
        self.request_callback();
    }

    unsafe fn prepare(&self) {
        let n = self.channels.load(Ordering::Relaxed);
        let sample_rate = f64::from_bits(self.sample_rate.load(Ordering::Relaxed));
        let max = self.max_frames.load(Ordering::Relaxed);
        self.params.set_channels(n as usize);
        let inner = &mut *self.inner.get();
        inner.prepare(&Setup {
            sample_rate: sample_rate as f32,
            max_block: max as usize,
            channels: n,
        });
        let l = inner.latency();
        self.latency.store(l, Ordering::Relaxed);
        self.announced_latency.store(l, Ordering::Relaxed);
        self.tail.store(inner.tail(), Ordering::Relaxed);
    }

    /// How many CLAP parameters there are. A per-channel parameter counts once for each channel.
    fn param_count(&self) -> usize {
        let n = self.channel_count();
        self.params
            .iter()
            .map(|p| if p.info().scope == Scope::PerChannel { n } else { 1 })
            .sum()
    }

    /// The parameter and channel behind a CLAP parameter index.
    fn locate(&self, mut index: usize) -> Option<(usize, Option<usize>)> {
        let n = self.channel_count();
        for (pi, p) in self.params.iter().enumerate() {
            let per_channel = p.info().scope == Scope::PerChannel;
            let count = if per_channel { n } else { 1 };
            if index < count {
                return Some((pi, if per_channel { Some(index) } else { None }));
            }
            index -= count;
        }
        None
    }

    /// The parameter and channel behind a CLAP parameter id. The id keeps the parameter's position in
    /// bits 7 and up and the channel in the low seven bits, so ids stay the same when the channel count changes.
    fn decode(&self, id: ClapId) -> Option<(usize, Option<usize>)> {
        let pi = (id >> 7) as usize;
        let ch = (id & 127) as usize;
        if pi >= self.params.len() {
            return None;
        }
        match self.params.at(pi).info().scope {
            Scope::Global => (ch == 0).then_some((pi, None)),
            Scope::PerChannel => (ch < self.channel_count()).then_some((pi, Some(ch))),
        }
    }

    /// Applies the parameter events in a list. Returns whether any value was written.
    unsafe fn apply_events(&self, events: *const InputEvents) -> bool {
        if events.is_null() {
            return false;
        }
        let list = &*events;
        let mut changed = false;
        for i in 0..(list.size)(events) {
            let header = (list.get)(events, i);
            if header.is_null() {
                continue;
            }
            let h = &*header;
            if h.space_id != EVENT_SPACE_CORE || h.type_ != EVENT_PARAM_VALUE || (h.size as usize) < size_of::<EventParamValue>() {
                continue;
            }
            let e = &*(header as *const EventParamValue);
            if !e.value.is_finite() {
                continue;
            }
            if let Some((pi, ch)) = self.decode(e.param_id) {
                let p = self.params.at(pi);
                if p.info().kind == Kind::Meter {
                    continue;
                }
                match ch {
                    None => {
                        p.set(e.value as f32);
                    }
                    Some(c) => {
                        p.set_ch(c, e.value as f32);
                    }
                }
                changed = true;
            }
        }
        changed
    }
}

unsafe extern "C" fn plugin_init(plugin: *const Plugin) -> bool {
    let w = wrapper(plugin);
    if !w.host.is_null() {
        let host = &*w.host;
        w.host_params
            .store((host.get_extension)(w.host, EXT_PARAMS.as_ptr() as *const c_char) as *mut HostParams, Ordering::Relaxed);
        w.host_latency.store(
            (host.get_extension)(w.host, EXT_LATENCY.as_ptr() as *const c_char) as *mut HostLatency,
            Ordering::Relaxed,
        );
    }
    true
}

unsafe extern "C" fn plugin_destroy(plugin: *const Plugin) {
    drop(Box::from_raw((*plugin).plugin_data as *mut Wrapper));
}

unsafe extern "C" fn plugin_activate(plugin: *const Plugin, sample_rate: f64, _min_frames: u32, max_frames: u32) -> bool {
    let w = wrapper(plugin);
    if w.active.load(Ordering::Acquire) || !sample_rate.is_finite() || sample_rate <= 0.0 {
        return false;
    }
    w.sample_rate.store(sample_rate.to_bits(), Ordering::Relaxed);
    w.max_frames.store(max_frames.max(1), Ordering::Relaxed);
    if catch_unwind(AssertUnwindSafe(|| w.prepare())).is_err() {
        return false;
    }
    w.active.store(true, Ordering::Release);
    true
}

unsafe extern "C" fn plugin_deactivate(plugin: *const Plugin) {
    wrapper(plugin).active.store(false, Ordering::Release);
}

unsafe extern "C" fn plugin_start_processing(_plugin: *const Plugin) -> bool {
    true
}

unsafe extern "C" fn plugin_stop_processing(_plugin: *const Plugin) {}

unsafe extern "C" fn plugin_reset(plugin: *const Plugin) {
    let w = wrapper(plugin);
    if w.active.load(Ordering::Acquire) {
        let _ = catch_unwind(AssertUnwindSafe(|| w.prepare()));
    }
}

unsafe extern "C" fn plugin_process(plugin: *const Plugin, process: *const Process) -> i32 {
    if plugin.is_null() || process.is_null() {
        return PROCESS_ERROR;
    }
    let w = wrapper(plugin);
    catch_unwind(AssertUnwindSafe(|| process_block(w, &*process))).unwrap_or(PROCESS_ERROR)
}

unsafe fn process_block(w: &Wrapper, pr: &Process) -> i32 {
    // Subnormal results become zero for the length of the block, whatever the plugin's own filters do.
    let _flush = ruse_core::denormals::FlushToZero::enable();
    if w.apply_events(pr.in_events) {
        w.mark_dirty();
    }
    let frames = pr.frames_count as usize;
    if frames == 0 {
        return PROCESS_CONTINUE;
    }
    if pr.audio_outputs.is_null() || pr.audio_outputs_count == 0 {
        return PROCESS_CONTINUE;
    }
    let out = &*pr.audio_outputs;
    let n = out.channel_count as usize;
    if out.data32.is_null() || n == 0 || n > MAX_CHANNELS || n != w.channel_count() {
        return PROCESS_ERROR;
    }
    let input = if pr.audio_inputs.is_null() || pr.audio_inputs_count == 0 {
        None
    } else {
        Some(&*pr.audio_inputs)
    };
    let slices = &mut *w.slices.get();
    slices.clear();
    for c in 0..n {
        let dst = *out.data32.add(c);
        if dst.is_null() {
            slices.clear();
            return PROCESS_ERROR;
        }
        let src = match input {
            Some(i) if (i.channel_count as usize) > c && !i.data32.is_null() => *i.data32.add(c),
            _ => null_mut(),
        };
        if src.is_null() {
            std::ptr::write_bytes(dst, 0, frames);
        } else if src != dst {
            std::ptr::copy(src as *const f32, dst, frames);
        }
        slices.push(std::slice::from_raw_parts_mut(dst, frames));
    }

    let sample_rate = f64::from_bits(w.sample_rate.load(Ordering::Relaxed));
    let mut ctx = ProcessContext {
        sample_rate: sample_rate as f32,
        sample_time: if pr.steady_time >= 0 { Some(pr.steady_time as f64) } else { None },
        playing: false,
        tempo_bpm: None,
    };
    if !pr.transport.is_null() {
        let t = &*pr.transport;
        ctx.playing = t.flags & TRANSPORT_IS_PLAYING != 0;
        if t.flags & TRANSPORT_HAS_TEMPO != 0 {
            ctx.tempo_bpm = Some(t.tempo);
        }
        if t.flags & TRANSPORT_HAS_SECONDS_TIMELINE != 0 {
            ctx.sample_time = Some(t.song_pos_seconds as f64 / SECTIME_FACTOR * sample_rate);
        }
    }

    let inner = &mut *w.inner.get();
    if w.layout.sidechain().is_some() {
        // The sidechain is the second input port. Channels the host left out read as silence to the plugin.
        let mut keys: [&[f32]; MAX_SIDECHAIN] = [&[]; MAX_SIDECHAIN];
        let mut count = 0;
        if !pr.audio_inputs.is_null() && pr.audio_inputs_count > 1 {
            let key = &*pr.audio_inputs.add(1);
            if !key.data32.is_null() {
                count = (key.channel_count as usize).min(MAX_SIDECHAIN);
                for (c, slot) in keys.iter_mut().enumerate().take(count) {
                    let src = *key.data32.add(c);
                    if !src.is_null() {
                        *slot = std::slice::from_raw_parts(src as *const f32, frames);
                    }
                }
            }
        }
        inner.process_with_sidechain(&mut slices[..], &keys[..count], &ctx);
    } else {
        inner.process(&mut slices[..], &ctx);
    }
    slices.clear();

    w.tail.store(inner.tail(), Ordering::Relaxed);
    let latency = inner.latency();
    if w.latency.swap(latency, Ordering::Relaxed) != latency {
        w.request_callback();
    }
    PROCESS_CONTINUE
}

unsafe extern "C" fn plugin_get_extension(_plugin: *const Plugin, id: *const c_char) -> *const c_void {
    if is_id(id, EXT_AUDIO_PORTS) {
        &AUDIO_PORTS as *const PluginAudioPorts as *const c_void
    } else if is_id(id, EXT_CONFIGURABLE_AUDIO_PORTS) || is_id(id, EXT_CONFIGURABLE_AUDIO_PORTS_COMPAT) {
        &CONFIGURABLE_PORTS as *const PluginConfigurableAudioPorts as *const c_void
    } else if is_id(id, EXT_PARAMS) {
        &PARAMS as *const PluginParams as *const c_void
    } else if is_id(id, EXT_STATE) {
        &STATE as *const PluginState as *const c_void
    } else if is_id(id, EXT_LATENCY) {
        &LATENCY as *const PluginLatency as *const c_void
    } else if is_id(id, EXT_TAIL) {
        &TAIL as *const PluginTail as *const c_void
    } else {
        null()
    }
}

unsafe extern "C" fn plugin_on_main_thread(plugin: *const Plugin) {
    let w = wrapper(plugin);
    if w.dirty.swap(false, Ordering::AcqRel) {
        w.params.notify();
    }
    let latency = w.latency.load(Ordering::Relaxed);
    if w.announced_latency.swap(latency, Ordering::Relaxed) != latency {
        let ext = w.host_latency.load(Ordering::Relaxed);
        if !ext.is_null() && !w.host.is_null() {
            ((*ext).changed)(w.host);
        }
    }
}

// Audio ports.

static AUDIO_PORTS: PluginAudioPorts = PluginAudioPorts {
    count: ports_count,
    get: ports_get,
};

unsafe extern "C" fn ports_count(plugin: *const Plugin, is_input: bool) -> u32 {
    if is_input && wrapper(plugin).layout.sidechain().is_some() {
        2
    } else {
        1
    }
}

unsafe extern "C" fn ports_get(plugin: *const Plugin, index: u32, is_input: bool, info: *mut AudioPortInfo) -> bool {
    if info.is_null() {
        return false;
    }
    let w = wrapper(plugin);
    let n = w.channels.load(Ordering::Relaxed);
    let info = &mut *info;
    if index == 1 && is_input {
        let (Some(bus), Some(k)) = (w.layout.sidechain(), w.layout.sidechain_channels(n)) else {
            return false;
        };
        info.id = 1;
        copy_str(info.name.as_mut_ptr(), NAME_SIZE, bus.name);
        info.flags = 0;
        info.channel_count = k;
        info.port_type = match k {
            1 => PORT_MONO.as_ptr() as *const c_char,
            2 => PORT_STEREO.as_ptr() as *const c_char,
            _ => null(),
        };
        info.in_place_pair = INVALID_ID;
        return true;
    }
    if index != 0 {
        return false;
    }
    info.id = 0;
    copy_str(info.name.as_mut_ptr(), NAME_SIZE, "Main");
    info.flags = AUDIO_PORT_IS_MAIN;
    info.channel_count = n;
    info.port_type = match n {
        1 => PORT_MONO.as_ptr() as *const c_char,
        2 => PORT_STEREO.as_ptr() as *const c_char,
        _ => null(),
    };
    info.in_place_pair = 0;
    true
}

static CONFIGURABLE_PORTS: PluginConfigurableAudioPorts = PluginConfigurableAudioPorts {
    can_apply_configuration: configurable_can_apply,
    apply_configuration: configurable_apply,
};

/// The one channel count a set of requests asks for, if the plugin can take it.
unsafe fn requested_channels(w: &Wrapper, requests: *const AudioPortConfigurationRequest, count: u32) -> Option<u32> {
    if requests.is_null() || count == 0 || w.active.load(Ordering::Acquire) {
        return None;
    }
    let mut wanted: Option<u32> = None;
    let mut key: Option<u32> = None;
    for i in 0..count as usize {
        let r = &*requests.add(i);
        if r.is_input && r.port_index == 1 && w.layout.sidechain().is_some() {
            key = Some(r.channel_count);
            continue;
        }
        if r.port_index != 0 || r.channel_count < w.min_channels || r.channel_count > w.max_channels {
            return None;
        }
        match wanted {
            Some(c) if c != r.channel_count => return None,
            _ => wanted = Some(r.channel_count),
        }
    }
    // The sidechain's channel count follows the main bus's, so a request for it must agree.
    match (wanted, key) {
        (Some(n), Some(k)) if w.layout.sidechain_channels(n) != Some(k) => None,
        _ => wanted,
    }
}

unsafe extern "C" fn configurable_can_apply(plugin: *const Plugin, requests: *const AudioPortConfigurationRequest, count: u32) -> bool {
    requested_channels(wrapper(plugin), requests, count).is_some()
}

unsafe extern "C" fn configurable_apply(plugin: *const Plugin, requests: *const AudioPortConfigurationRequest, count: u32) -> bool {
    let w = wrapper(plugin);
    let Some(n) = requested_channels(w, requests, count) else {
        return false;
    };
    w.channels.store(n, Ordering::Relaxed);
    w.params.set_channels(n as usize);
    let ext = w.host_params.load(Ordering::Relaxed);
    if !ext.is_null() && !w.host.is_null() {
        ((*ext).rescan)(w.host, PARAM_RESCAN_ALL);
    }
    true
}

// Parameters.

static PARAMS: PluginParams = PluginParams {
    count: params_count,
    get_info: params_get_info,
    get_value: params_get_value,
    value_to_text: params_value_to_text,
    text_to_value: params_text_to_value,
    flush: params_flush,
};

/// Copies a string into a fixed C buffer, cutting at a character boundary and ending with a zero.
unsafe fn copy_str(dst: *mut c_char, capacity: usize, s: &str) {
    if capacity == 0 {
        return;
    }
    let mut end = s.len().min(capacity - 1);
    while !s.is_char_boundary(end) {
        end -= 1;
    }
    std::ptr::copy_nonoverlapping(s.as_ptr() as *const c_char, dst, end);
    *dst.add(end) = 0;
}

unsafe extern "C" fn params_count(plugin: *const Plugin) -> u32 {
    wrapper(plugin).param_count() as u32
}

unsafe extern "C" fn params_get_info(plugin: *const Plugin, index: u32, out: *mut ParamInfo) -> bool {
    let w = wrapper(plugin);
    if out.is_null() {
        return false;
    }
    let Some((pi, ch)) = w.locate(index as usize) else {
        return false;
    };
    let info = w.params.at(pi).info();
    let out = &mut *out;
    out.id = ((pi as u32) << 7) | ch.unwrap_or(0) as u32;
    out.flags = match info.kind {
        Kind::Knob => PARAM_IS_AUTOMATABLE,
        Kind::Choice => PARAM_IS_AUTOMATABLE | PARAM_IS_STEPPED | PARAM_IS_ENUM,
        Kind::Toggle => PARAM_IS_AUTOMATABLE | PARAM_IS_STEPPED | if info.id == "bypass" { PARAM_IS_BYPASS } else { 0 },
        Kind::Meter => PARAM_IS_READONLY,
    };
    out.cookie = null_mut();
    let (name, module) = match ch {
        None => (info.name.to_string(), info.group.to_string()),
        Some(c) => (
            format!("{} {}", info.name, c + 1),
            if info.group.is_empty() {
                format!("Channel {}", c + 1)
            } else {
                format!("{}/Channel {}", info.group, c + 1)
            },
        ),
    };
    copy_str(out.name.as_mut_ptr(), NAME_SIZE, &name);
    copy_str(out.module.as_mut_ptr(), PATH_SIZE, &module);
    out.min_value = info.min as f64;
    out.max_value = info.max as f64;
    out.default_value = info.default as f64;
    true
}

unsafe extern "C" fn params_get_value(plugin: *const Plugin, id: ClapId, out: *mut f64) -> bool {
    let w = wrapper(plugin);
    if out.is_null() {
        return false;
    }
    let Some((pi, ch)) = w.decode(id) else {
        return false;
    };
    let p = w.params.at(pi);
    *out = match ch {
        None => p.get(),
        Some(c) => p.get_ch(c),
    } as f64;
    true
}

unsafe extern "C" fn params_value_to_text(plugin: *const Plugin, id: ClapId, value: f64, buffer: *mut c_char, capacity: u32) -> bool {
    let w = wrapper(plugin);
    if buffer.is_null() {
        return false;
    }
    let Some((pi, _)) = w.decode(id) else {
        return false;
    };
    let text = w.params.at(pi).info().display(value as f32);
    copy_str(buffer, capacity as usize, &text);
    true
}

fn parse_value(info: &RuseParamInfo, text: &str) -> Option<f64> {
    let t = text.trim();
    match info.kind {
        Kind::Choice => info
            .choices
            .iter()
            .position(|c| c.eq_ignore_ascii_case(t))
            .map(|i| i as f64)
            .or_else(|| t.parse::<f64>().ok()),
        Kind::Toggle => match t.to_ascii_lowercase().as_str() {
            "on" | "1" | "true" => Some(1.0),
            "off" | "0" | "false" => Some(0.0),
            _ => None,
        },
        _ => t
            .split_whitespace()
            .next()
            .and_then(|s| s.trim_end_matches(|c: char| c.is_alphabetic() || c == '%').parse::<f64>().ok()),
    }
}

unsafe extern "C" fn params_text_to_value(plugin: *const Plugin, id: ClapId, text: *const c_char, out: *mut f64) -> bool {
    let w = wrapper(plugin);
    if text.is_null() || out.is_null() {
        return false;
    }
    let Some((pi, _)) = w.decode(id) else {
        return false;
    };
    let Ok(s) = CStr::from_ptr(text).to_str() else {
        return false;
    };
    match parse_value(w.params.at(pi).info(), s) {
        Some(v) => {
            *out = v;
            true
        }
        None => false,
    }
}

unsafe extern "C" fn params_flush(plugin: *const Plugin, in_events: *const InputEvents, _out_events: *const OutputEvents) {
    let w = wrapper(plugin);
    if w.apply_events(in_events) {
        if w.active.load(Ordering::Acquire) {
            w.mark_dirty();
        } else {
            w.params.notify();
        }
    }
}

// State: a short header and every value of every parameter, so a saved project restores exactly.

static STATE: PluginState = PluginState {
    save: state_save,
    load: state_load,
};

unsafe extern "C" fn state_save(plugin: *const Plugin, stream: *const OStream) -> bool {
    if stream.is_null() {
        return false;
    }
    let w = wrapper(plugin);
    let bytes = ruse_core::state::save(&w.params);
    let s = &*stream;
    let mut done = 0usize;
    while done < bytes.len() {
        let n = (s.write)(stream, bytes.as_ptr().add(done) as *const c_void, (bytes.len() - done) as u64);
        if n <= 0 {
            return false;
        }
        done += n as usize;
    }
    true
}

unsafe fn read_all(stream: *const IStream) -> Option<Vec<u8>> {
    let s = &*stream;
    let mut bytes: Vec<u8> = Vec::new();
    let mut chunk = [0u8; 4096];
    loop {
        let n = (s.read)(stream, chunk.as_mut_ptr() as *mut c_void, chunk.len() as u64);
        if n < 0 {
            return None;
        }
        if n == 0 {
            return Some(bytes);
        }
        bytes.extend_from_slice(&chunk[..n as usize]);
    }
}

unsafe extern "C" fn state_load(plugin: *const Plugin, stream: *const IStream) -> bool {
    if stream.is_null() {
        return false;
    }
    let w = wrapper(plugin);
    let Some(bytes) = read_all(stream) else {
        return false;
    };
    if !ruse_core::state::load(&w.params, &bytes) {
        return false;
    }
    // The values changed under the host, so it has to read them again.
    let ext = w.host_params.load(Ordering::Relaxed);
    if !ext.is_null() && !w.host.is_null() {
        ((*ext).rescan)(w.host, PARAM_RESCAN_VALUES);
    }
    true
}

// Latency and tail come from values the audio thread keeps current.

static LATENCY: PluginLatency = PluginLatency { get: latency_get };
static TAIL: PluginTail = PluginTail { get: tail_get };

unsafe extern "C" fn latency_get(plugin: *const Plugin) -> u32 {
    wrapper(plugin).latency.load(Ordering::Relaxed)
}

unsafe extern "C" fn tail_get(plugin: *const Plugin) -> u32 {
    wrapper(plugin).tail.load(Ordering::Relaxed)
}
