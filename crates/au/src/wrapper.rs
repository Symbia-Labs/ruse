//! Wraps any MUSE plugin as an Audio Unit v2 effect.
//!
//! The host finds the plugin through the factory function the `export_au!` macro defines, and calls it
//! through the interface that function returns. Each call is a numbered selector that `lookup` maps to
//! a function here.
//!
//! The unit has one input and one output element. Both carry the same channel count, which the host
//! sets with the stream format, within the range the plugin declares. A per-channel parameter appears
//! as one Audio Unit parameter for each channel. Parameter changes from the host land at the start of
//! the next render, so automation is not yet sample-accurate.

use crate::sys::*;
use muse_core::denormals::FlushToZero;
use muse_core::{Channels, Curve, Kind, Layout, ParamInfo as MuseParamInfo, ParamSet, Plugin as MusePlugin, ProcessContext, Scope, Setup};
use std::cell::UnsafeCell;
use std::ffi::{c_char, c_void};
use std::mem::size_of;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::ptr::{null, null_mut};
use std::sync::atomic::{AtomicBool, AtomicI32, AtomicPtr, AtomicU32, AtomicU64, Ordering};
use std::sync::Mutex;

/// The most channels a wrapped plugin can have.
const MAX_CHANNELS: usize = 64;

/// What the wrapper needs from a plugin, with the plugin's own type erased.
trait Inner: Send {
    fn layout(&self) -> Layout;
    fn params(&self) -> ParamSet;
    fn prepare(&mut self, setup: &Setup);
    fn process(&mut self, channels: &mut [&mut [f32]], ctx: &ProcessContext);
    fn latency(&self) -> u32;
    fn tail(&self) -> u32;
}

impl<P: MusePlugin> Inner for P {
    fn layout(&self) -> Layout {
        MusePlugin::layout(self)
    }
    fn params(&self) -> ParamSet {
        MusePlugin::params(self).clone()
    }
    fn prepare(&mut self, setup: &Setup) {
        MusePlugin::prepare(self, setup)
    }
    fn process(&mut self, channels: &mut [&mut [f32]], ctx: &ProcessContext) {
        MusePlugin::process(self, channels, ctx)
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

type PropertyListenerProc = unsafe extern "C" fn(
    ref_con: *mut c_void,
    unit: AudioComponentInstance,
    id: AudioUnitPropertyID,
    scope: AudioUnitScope,
    element: AudioUnitElement,
);

#[derive(Clone, Copy)]
struct Listener {
    id: AudioUnitPropertyID,
    proc: PropertyListenerProc,
    user: usize,
}

#[derive(Clone, Copy)]
struct RenderInput {
    proc: AURenderCallback,
    user: usize,
}

#[derive(Clone, Copy)]
struct Notify {
    proc: AURenderCallback,
    user: usize,
}

#[repr(C)]
struct Instance {
    // First, so the pointer the host holds is also a pointer to the instance.
    iface: PlugInInterface,
    desc: AudioComponentDescription,
    unit: AtomicPtr<c_void>,
    // Run by the render call, and by initialize and reset while no render is under way.
    inner: UnsafeCell<Box<dyn Inner>>,
    // One buffer for each channel, for a host that hands over empty buffer lists. Filled in at initialize.
    scratch: UnsafeCell<Vec<Vec<f32>>>,
    // Set up before rendering and used only by the render call, so it never allocates while running.
    slices: UnsafeCell<Vec<&'static mut [f32]>>,
    params: ParamSet,
    min_channels: u32,
    max_channels: u32,
    // An effect takes as many channels in as it gives out, and the host sets each side with its own
    // stream format. The parameters and the render call follow the output.
    in_channels: AtomicU32,
    out_channels: AtomicU32,
    sample_rate: AtomicU64,
    max_frames: AtomicU32,
    initialized: AtomicBool,
    bypass: AtomicBool,
    last_render_error: AtomicI32,
    preset_number: AtomicI32,
    preset_name: Mutex<String>,
    latency: AtomicU32,
    tail: AtomicU32,
    render_input: Mutex<Option<RenderInput>>,
    connection: Mutex<Option<(usize, u32)>>,
    host_callbacks: Mutex<Option<HostCallbackInfo>>,
    listeners: Mutex<Vec<Listener>>,
    notifies: Mutex<Vec<Notify>>,
    pending: Mutex<Vec<(u32, f32)>>,
}

// The raw pointers are the host's, valid for the unit's life. The UnsafeCells follow the rules above.
unsafe impl Send for Instance {}
unsafe impl Sync for Instance {}

unsafe fn inst<'a>(this: *mut c_void) -> &'a Instance {
    &*(this as *const Instance)
}

/// Builds the Audio Unit for a plugin type. The `export_au!` macro calls this from the factory function.
///
/// # Safety
/// `desc` must be null or point to a valid description.
pub unsafe fn create<P: MusePlugin + Default + 'static>(desc: *const AudioComponentDescription) -> *mut PlugInInterface {
    if desc.is_null() {
        return null_mut();
    }
    let inner: Box<dyn Inner> = Box::new(P::default());
    let (min, max) = bounds(&inner.layout());
    let params = inner.params();
    let channels = if (min..=max).contains(&2) { 2 } else { min };
    params.set_channels(channels as usize);
    let latency = inner.latency();
    let tail = inner.tail();
    let w = Box::new(Instance {
        iface: PlugInInterface {
            open: au_open,
            close: au_close,
            lookup: au_lookup,
            reserved: null_mut(),
        },
        desc: *desc,
        unit: AtomicPtr::new(null_mut()),
        inner: UnsafeCell::new(inner),
        scratch: UnsafeCell::new(Vec::new()),
        slices: UnsafeCell::new(Vec::with_capacity(MAX_CHANNELS)),
        params,
        min_channels: min,
        max_channels: max,
        in_channels: AtomicU32::new(channels),
        out_channels: AtomicU32::new(channels),
        sample_rate: AtomicU64::new(44100f64.to_bits()),
        max_frames: AtomicU32::new(1156),
        initialized: AtomicBool::new(false),
        bypass: AtomicBool::new(false),
        last_render_error: AtomicI32::new(0),
        preset_number: AtomicI32::new(-1),
        preset_name: Mutex::new("Untitled".to_string()),
        latency: AtomicU32::new(latency),
        tail: AtomicU32::new(tail),
        render_input: Mutex::new(None),
        connection: Mutex::new(None),
        host_callbacks: Mutex::new(None),
        listeners: Mutex::new(Vec::new()),
        notifies: Mutex::new(Vec::new()),
        pending: Mutex::new(Vec::new()),
    });
    Box::into_raw(w) as *mut PlugInInterface
}

fn erase<F: Copy>(f: F) -> unsafe extern "C" fn() {
    assert_eq!(size_of::<F>(), size_of::<unsafe extern "C" fn()>());
    // Every selector's function is a plain C function pointer, so the host casts it back to the right type.
    unsafe { std::mem::transmute_copy::<F, unsafe extern "C" fn()>(&f) }
}

unsafe extern "C" fn au_open(this: *mut c_void, unit: AudioComponentInstance) -> OSStatus {
    inst(this).unit.store(unit, Ordering::Relaxed);
    NO_ERR
}

unsafe extern "C" fn au_close(this: *mut c_void) -> OSStatus {
    drop(Box::from_raw(this as *mut Instance));
    NO_ERR
}

unsafe extern "C" fn au_lookup(selector: i16) -> Option<unsafe extern "C" fn()> {
    Some(match selector {
        SELECT_INITIALIZE => erase(initialize as unsafe extern "C" fn(*mut c_void) -> OSStatus),
        SELECT_UNINITIALIZE => erase(uninitialize as unsafe extern "C" fn(*mut c_void) -> OSStatus),
        SELECT_GET_PROPERTY_INFO => erase(
            get_property_info as unsafe extern "C" fn(*mut c_void, u32, u32, u32, *mut u32, *mut u8) -> OSStatus,
        ),
        SELECT_GET_PROPERTY => {
            erase(get_property as unsafe extern "C" fn(*mut c_void, u32, u32, u32, *mut c_void, *mut u32) -> OSStatus)
        }
        SELECT_SET_PROPERTY => {
            erase(set_property as unsafe extern "C" fn(*mut c_void, u32, u32, u32, *const c_void, u32) -> OSStatus)
        }
        SELECT_GET_PARAMETER => {
            erase(get_parameter as unsafe extern "C" fn(*mut c_void, u32, u32, u32, *mut f32) -> OSStatus)
        }
        SELECT_SET_PARAMETER => {
            erase(set_parameter as unsafe extern "C" fn(*mut c_void, u32, u32, u32, f32, u32) -> OSStatus)
        }
        SELECT_SCHEDULE_PARAMETERS => {
            erase(schedule_parameters as unsafe extern "C" fn(*mut c_void, *const AudioUnitParameterEvent, u32) -> OSStatus)
        }
        SELECT_RESET => erase(reset as unsafe extern "C" fn(*mut c_void, u32, u32) -> OSStatus),
        SELECT_ADD_PROPERTY_LISTENER => erase(
            add_property_listener as unsafe extern "C" fn(*mut c_void, u32, PropertyListenerProc, *mut c_void) -> OSStatus,
        ),
        SELECT_REMOVE_PROPERTY_LISTENER => {
            erase(remove_property_listener as unsafe extern "C" fn(*mut c_void, u32, PropertyListenerProc) -> OSStatus)
        }
        SELECT_REMOVE_PROPERTY_LISTENER_WITH_USER_DATA => erase(
            remove_property_listener_with_user_data
                as unsafe extern "C" fn(*mut c_void, u32, PropertyListenerProc, *mut c_void) -> OSStatus,
        ),
        SELECT_ADD_RENDER_NOTIFY => {
            erase(add_render_notify as unsafe extern "C" fn(*mut c_void, AURenderCallback, *mut c_void) -> OSStatus)
        }
        SELECT_REMOVE_RENDER_NOTIFY => {
            erase(remove_render_notify as unsafe extern "C" fn(*mut c_void, AURenderCallback, *mut c_void) -> OSStatus)
        }
        SELECT_RENDER => erase(
            render
                as unsafe extern "C" fn(*mut c_void, *mut u32, *const AudioTimeStamp, u32, u32, *mut AudioBufferList) -> OSStatus,
        ),
        _ => return None,
    })
}

impl Instance {
    fn channel_count(&self) -> usize {
        self.out_channels.load(Ordering::Relaxed) as usize
    }

    fn rate(&self) -> f64 {
        f64::from_bits(self.sample_rate.load(Ordering::Relaxed))
    }

    /// Tells every listener on a property that it changed.
    unsafe fn notify(&self, id: AudioUnitPropertyID, scope: AudioUnitScope, element: AudioUnitElement) {
        let listeners: Vec<Listener> = match self.listeners.lock() {
            Ok(l) => l.iter().filter(|l| l.id == id).copied().collect(),
            Err(_) => return,
        };
        let unit = self.unit.load(Ordering::Relaxed);
        for l in listeners {
            (l.proc)(l.user as *mut c_void, unit, id, scope, element);
        }
    }

    /// How many Audio Unit parameters there are. A per-channel parameter counts once for each channel.
    fn param_count(&self) -> usize {
        let n = self.channel_count();
        self.params
            .iter()
            .map(|p| if p.info().scope == Scope::PerChannel { n } else { 1 })
            .sum()
    }

    /// The ids in the order the unit lists them. The id keeps the parameter's position in bits 7 and up
    /// and the channel in the low seven bits, so ids stay the same when the channel count changes.
    fn param_ids(&self) -> Vec<u32> {
        let n = self.channel_count();
        let mut ids = Vec::new();
        for (pi, p) in self.params.iter().enumerate() {
            if p.info().scope == Scope::PerChannel {
                for c in 0..n {
                    ids.push(((pi as u32) << 7) | c as u32);
                }
            } else {
                ids.push((pi as u32) << 7);
            }
        }
        ids
    }

    /// The parameter and channel behind an id.
    fn decode(&self, id: u32) -> Option<(usize, Option<usize>)> {
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

    fn get_value(&self, id: u32) -> Option<f32> {
        let (pi, ch) = self.decode(id)?;
        let p = self.params.at(pi);
        Some(match ch {
            None => p.get(),
            Some(c) => p.get_ch(c),
        })
    }

    /// Sets a writable parameter. Returns false for an unknown id or a read-only meter.
    fn set_value(&self, id: u32, value: f32) -> bool {
        let Some((pi, ch)) = self.decode(id) else {
            return false;
        };
        let p = self.params.at(pi);
        if p.info().kind == Kind::Meter {
            return false;
        }
        match ch {
            None => p.set(value),
            Some(c) => p.set_ch(c, value),
        };
        true
    }

    /// Reads the plugin's latency and tail into values any thread can read.
    unsafe fn refresh_timing(&self) {
        let inner = &*self.inner.get();
        self.latency.store(inner.latency(), Ordering::Relaxed);
        self.tail.store(inner.tail(), Ordering::Relaxed);
    }

    unsafe fn prepare(&self) {
        let n = self.out_channels.load(Ordering::Relaxed);
        self.params.set_channels(n as usize);
        let inner = &mut *self.inner.get();
        inner.prepare(&Setup {
            sample_rate: self.rate() as f32,
            max_block: self.max_frames.load(Ordering::Relaxed) as usize,
            channels: n,
        });
        self.refresh_timing();
    }
}

// Lifecycle.

unsafe extern "C" fn initialize(this: *mut c_void) -> OSStatus {
    let w = inst(this);
    if w.initialized.load(Ordering::Acquire) {
        return NO_ERR;
    }
    let n = w.out_channels.load(Ordering::Relaxed);
    // An effect takes as many channels in as it gives out.
    if n != w.in_channels.load(Ordering::Relaxed) || n < w.min_channels || n > w.max_channels {
        return ERR_FORMAT_NOT_SUPPORTED;
    }
    let frames = w.max_frames.load(Ordering::Relaxed) as usize;
    *w.scratch.get() = (0..n).map(|_| vec![0.0f32; frames]).collect();
    if catch_unwind(AssertUnwindSafe(|| w.prepare())).is_err() {
        return ERR_FAILED_INITIALIZATION;
    }
    w.initialized.store(true, Ordering::Release);
    NO_ERR
}

unsafe extern "C" fn uninitialize(this: *mut c_void) -> OSStatus {
    inst(this).initialized.store(false, Ordering::Release);
    NO_ERR
}

unsafe extern "C" fn reset(this: *mut c_void, _scope: u32, _element: u32) -> OSStatus {
    let w = inst(this);
    if w.initialized.load(Ordering::Acquire) {
        let _ = catch_unwind(AssertUnwindSafe(|| w.prepare()));
    }
    NO_ERR
}

// Properties.

fn check_global(scope: u32, element: u32) -> Result<(), OSStatus> {
    if scope != SCOPE_GLOBAL {
        Err(ERR_INVALID_SCOPE)
    } else if element != 0 {
        Err(ERR_INVALID_ELEMENT)
    } else {
        Ok(())
    }
}

fn check_bus(scope: u32, element: u32) -> Result<(), OSStatus> {
    if scope != SCOPE_INPUT && scope != SCOPE_OUTPUT {
        Err(ERR_INVALID_SCOPE)
    } else if element != 0 {
        Err(ERR_INVALID_ELEMENT)
    } else {
        Ok(())
    }
}

fn channel_infos(w: &Instance) -> Vec<AUChannelInfo> {
    if w.min_channels == 1 && w.max_channels as usize >= MAX_CHANNELS {
        // Any count, as long as input and output match.
        vec![AUChannelInfo {
            in_channels: -1,
            out_channels: -1,
        }]
    } else {
        (w.min_channels..=w.max_channels)
            .map(|n| AUChannelInfo {
                in_channels: n as i16,
                out_channels: n as i16,
            })
            .collect()
    }
}

/// The size of a property's data and whether the host may write it, or the error to give.
fn property_info(w: &Instance, id: u32, scope: u32, element: u32) -> Result<(u32, bool), OSStatus> {
    match id {
        PROP_CLASS_INFO => check_global(scope, element).map(|_| (size_of::<CFDictionaryRef>() as u32, true)),
        PROP_MAKE_CONNECTION => {
            if scope != SCOPE_INPUT {
                Err(ERR_INVALID_SCOPE)
            } else if element != 0 {
                Err(ERR_INVALID_ELEMENT)
            } else {
                Ok((size_of::<AudioUnitConnection>() as u32, true))
            }
        }
        PROP_SAMPLE_RATE => check_bus(scope, element).map(|_| (8, true)),
        PROP_PARAMETER_LIST => {
            // Only the global scope has parameters. The other scopes list none.
            Ok(if scope == SCOPE_GLOBAL { (w.param_count() as u32 * 4, false) } else { (0, false) })
        }
        PROP_PARAMETER_INFO => {
            if scope != SCOPE_GLOBAL {
                Err(ERR_INVALID_SCOPE)
            } else if w.decode(element).is_none() {
                Err(ERR_INVALID_PARAMETER)
            } else {
                Ok((size_of::<AudioUnitParameterInfo>() as u32, false))
            }
        }
        PROP_STREAM_FORMAT => check_bus(scope, element).map(|_| (size_of::<AudioStreamBasicDescription>() as u32, true)),
        PROP_ELEMENT_COUNT => {
            if scope == SCOPE_GLOBAL || scope == SCOPE_INPUT || scope == SCOPE_OUTPUT {
                Ok((4, false))
            } else {
                Err(ERR_INVALID_SCOPE)
            }
        }
        PROP_LATENCY | PROP_TAIL_TIME => check_global(scope, element).map(|_| (8, false)),
        PROP_SUPPORTED_NUM_CHANNELS => {
            check_global(scope, element).map(|_| (channel_infos(w).len() as u32 * size_of::<AUChannelInfo>() as u32, false))
        }
        PROP_MAXIMUM_FRAMES_PER_SLICE | PROP_BYPASS_EFFECT | PROP_LAST_RENDER_ERROR | PROP_IN_PLACE_PROCESSING => {
            check_global(scope, element).map(|_| (4, true))
        }
        PROP_PARAMETER_VALUE_STRINGS => {
            if scope != SCOPE_GLOBAL {
                return Err(ERR_INVALID_SCOPE);
            }
            match w.decode(element) {
                Some((pi, _)) if w.params.at(pi).info().kind == Kind::Choice => Ok((size_of::<CFArrayRef>() as u32, false)),
                Some(_) => Err(ERR_INVALID_PROPERTY),
                None => Err(ERR_INVALID_PARAMETER),
            }
        }
        PROP_SET_RENDER_CALLBACK => {
            if scope != SCOPE_INPUT {
                Err(ERR_INVALID_SCOPE)
            } else if element != 0 {
                Err(ERR_INVALID_ELEMENT)
            } else {
                Ok((size_of::<AURenderCallbackStruct>() as u32, true))
            }
        }
        PROP_HOST_CALLBACKS => check_global(scope, element).map(|_| (size_of::<HostCallbackInfo>() as u32, true)),
        PROP_SHOULD_ALLOCATE_BUFFER => check_bus(scope, element).map(|_| (4, true)),
        PROP_PRESENT_PRESET => check_global(scope, element).map(|_| (size_of::<AUPreset>() as u32, true)),
        PROP_PARAMETER_STRING_FROM_VALUE => {
            check_global(scope, element).map(|_| (size_of::<AudioUnitParameterStringFromValue>() as u32, false))
        }
        PROP_PARAMETER_VALUE_FROM_STRING => {
            check_global(scope, element).map(|_| (size_of::<AudioUnitParameterValueFromString>() as u32, false))
        }
        PROP_PARAMETER_ID_NAME => check_global(scope, element).map(|_| (size_of::<AudioUnitParameterNameInfo>() as u32, false)),
        _ => Err(ERR_INVALID_PROPERTY),
    }
}

unsafe extern "C" fn get_property_info(
    this: *mut c_void,
    id: u32,
    scope: u32,
    element: u32,
    size: *mut u32,
    writable: *mut u8,
) -> OSStatus {
    match property_info(inst(this), id, scope, element) {
        Ok((n, can_write)) => {
            if !size.is_null() {
                *size = n;
            }
            if !writable.is_null() {
                *writable = can_write as u8;
            }
            NO_ERR
        }
        Err(e) => e,
    }
}

unsafe fn put<T>(out: *mut c_void, value: T) {
    std::ptr::write_unaligned(out as *mut T, value);
}

fn unit_for(info: &MuseParamInfo) -> u32 {
    match info.kind {
        Kind::Choice => UNIT_INDEXED,
        Kind::Toggle => UNIT_BOOLEAN,
        _ => match info.unit {
            "dB" => UNIT_DECIBELS,
            "Hz" => UNIT_HERTZ,
            "%" => UNIT_PERCENT,
            "ms" => UNIT_MILLISECONDS,
            "s" => UNIT_SECONDS,
            _ => UNIT_GENERIC,
        },
    }
}

unsafe fn fill_parameter_info(w: &Instance, id: u32, out: *mut AudioUnitParameterInfo) -> bool {
    let Some((pi, ch)) = w.decode(id) else {
        return false;
    };
    let info = w.params.at(pi).info();
    let name = match ch {
        None => info.name.to_string(),
        Some(c) => format!("{} {}", info.name, c + 1),
    };
    std::ptr::write_bytes(out as *mut u8, 0, size_of::<AudioUnitParameterInfo>());
    let o = &mut *out;
    // The fixed-size name is the old field. Cut it on a character boundary and end it with a zero.
    let mut end = name.len().min(o.name.len() - 1);
    while !name.is_char_boundary(end) {
        end -= 1;
    }
    std::ptr::copy_nonoverlapping(name.as_ptr() as *const c_char, o.name.as_mut_ptr(), end);
    o.cf_name_string = cf_string(&name);
    o.unit = unit_for(info);
    o.min_value = info.min;
    o.max_value = info.max;
    o.default_value = info.default;
    let mut flags = PARAM_FLAG_HAS_CF_NAME_STRING | PARAM_FLAG_CF_NAME_RELEASE | PARAM_FLAG_IS_READABLE;
    if info.kind == Kind::Meter {
        flags |= PARAM_FLAG_METER_READ_ONLY;
    } else {
        flags |= PARAM_FLAG_IS_WRITABLE;
    }
    if info.kind == Kind::Choice {
        flags |= PARAM_FLAG_VALUES_HAVE_STRINGS;
    }
    if info.curve == Curve::Log {
        flags |= PARAM_FLAG_DISPLAY_LOGARITHMIC;
    }
    o.flags = flags;
    true
}

unsafe fn class_info(w: &Instance) -> CFDictionaryRef {
    let key_callbacks = &kCFTypeDictionaryKeyCallBacks as *const u8 as *const c_void;
    let value_callbacks = &kCFTypeDictionaryValueCallBacks as *const u8 as *const c_void;
    let dict = CFDictionaryCreateMutable(null(), 0, key_callbacks, value_callbacks);
    let number = |key: &str, v: i32| {
        let k = cf_string(key);
        let n = CFNumberCreate(null(), CF_NUMBER_SINT32_TYPE, &v as *const i32 as *const c_void);
        CFDictionarySetValue(dict, k, n);
        CFRelease(k);
        CFRelease(n);
    };
    number("version", 0);
    number("type", w.desc.component_type as i32);
    number("subtype", w.desc.component_sub_type as i32);
    number("manufacturer", w.desc.component_manufacturer as i32);
    let k = cf_string("name");
    let preset = w.preset_name.lock().map(|n| n.clone()).unwrap_or_default();
    let v = cf_string(&preset);
    CFDictionarySetValue(dict, k, v);
    CFRelease(k);
    CFRelease(v);
    let bytes = muse_core::state::save(&w.params);
    let k = cf_string("data");
    let v = CFDataCreate(null(), bytes.as_ptr(), bytes.len() as isize);
    CFDictionarySetValue(dict, k, v);
    CFRelease(k);
    CFRelease(v);
    dict as CFDictionaryRef
}

unsafe fn apply_class_info(w: &Instance, dict: CFDictionaryRef) -> OSStatus {
    if dict.is_null() || CFGetTypeID(dict) != CFDictionaryGetTypeID() {
        return ERR_INVALID_PROPERTY_VALUE;
    }
    let number = |key: &str| -> Option<i32> {
        let k = cf_string(key);
        let v = CFDictionaryGetValue(dict, k);
        CFRelease(k);
        if v.is_null() || CFGetTypeID(v) != CFNumberGetTypeID() {
            return None;
        }
        let mut out: i32 = 0;
        (CFNumberGetValue(v, CF_NUMBER_SINT32_TYPE, &mut out as *mut i32 as *mut c_void) != 0).then_some(out)
    };
    let same = number("type") == Some(w.desc.component_type as i32)
        && number("subtype") == Some(w.desc.component_sub_type as i32)
        && number("manufacturer") == Some(w.desc.component_manufacturer as i32);
    if !same {
        return ERR_INVALID_PROPERTY_VALUE;
    }
    // The preset's name travels with the state, so a host that saves and restores it keeps the name.
    let k = cf_string("name");
    let saved_name = string_from_cf(CFDictionaryGetValue(dict, k));
    CFRelease(k);
    if let (Some(name), Ok(mut current)) = (saved_name, w.preset_name.lock()) {
        *current = name;
    }
    let k = cf_string("data");
    let data = CFDictionaryGetValue(dict, k);
    CFRelease(k);
    if data.is_null() {
        return NO_ERR;
    }
    if CFGetTypeID(data) != CFDataGetTypeID() {
        return ERR_INVALID_PROPERTY_VALUE;
    }
    let bytes = std::slice::from_raw_parts(CFDataGetBytePtr(data), CFDataGetLength(data).max(0) as usize);
    if muse_core::state::load(&w.params, bytes) {
        NO_ERR
    } else {
        ERR_INVALID_PROPERTY_VALUE
    }
}

fn parse_value(info: &MuseParamInfo, text: &str) -> Option<f32> {
    let t = text.trim();
    match info.kind {
        Kind::Choice => info
            .choices
            .iter()
            .position(|c| c.eq_ignore_ascii_case(t))
            .map(|i| i as f32)
            .or_else(|| t.parse::<f32>().ok()),
        Kind::Toggle => match t.to_ascii_lowercase().as_str() {
            "on" | "1" | "true" => Some(1.0),
            "off" | "0" | "false" => Some(0.0),
            _ => None,
        },
        _ => t
            .split_whitespace()
            .next()
            .and_then(|s| s.trim_end_matches(|c: char| c.is_alphabetic() || c == '%').parse::<f32>().ok()),
    }
}

/// Writes a property's data. `out` holds at least the size `property_info` gave.
unsafe fn write_property(w: &Instance, id: u32, scope: u32, element: u32, out: *mut c_void) -> OSStatus {
    match id {
        PROP_CLASS_INFO => put(out, class_info(w)),
        PROP_MAKE_CONNECTION => {
            let c = w.connection.lock().map(|g| *g).unwrap_or(None);
            put(
                out,
                AudioUnitConnection {
                    source_audio_unit: c.map_or(null_mut(), |c| c.0 as *mut c_void),
                    source_output_number: c.map_or(0, |c| c.1),
                    dest_input_number: 0,
                },
            );
        }
        PROP_SAMPLE_RATE => put(out, w.rate()),
        PROP_PARAMETER_LIST => {
            if scope == SCOPE_GLOBAL {
                for (i, id) in w.param_ids().iter().enumerate() {
                    put((out as *mut u32).add(i) as *mut c_void, *id);
                }
            }
        }
        PROP_PARAMETER_INFO => {
            if !fill_parameter_info(w, element, out as *mut AudioUnitParameterInfo) {
                return ERR_INVALID_PARAMETER;
            }
        }
        PROP_STREAM_FORMAT => {
            let side = if scope == SCOPE_INPUT { &w.in_channels } else { &w.out_channels };
            put(out, AudioStreamBasicDescription::float_planar(w.rate(), side.load(Ordering::Relaxed)))
        }
        PROP_ELEMENT_COUNT => put(out, 1u32),
        PROP_LATENCY => put(out, w.latency.load(Ordering::Relaxed) as f64 / w.rate()),
        PROP_TAIL_TIME => put(out, w.tail.load(Ordering::Relaxed) as f64 / w.rate()),
        PROP_SUPPORTED_NUM_CHANNELS => {
            for (i, info) in channel_infos(w).into_iter().enumerate() {
                put((out as *mut AUChannelInfo).add(i) as *mut c_void, info);
            }
        }
        PROP_MAXIMUM_FRAMES_PER_SLICE => put(out, w.max_frames.load(Ordering::Relaxed)),
        PROP_BYPASS_EFFECT => put(out, w.bypass.load(Ordering::Relaxed) as u32),
        PROP_LAST_RENDER_ERROR => put(out, w.last_render_error.load(Ordering::Relaxed)),
        PROP_IN_PLACE_PROCESSING => put(out, 1u32),
        PROP_SHOULD_ALLOCATE_BUFFER => put(out, 1u32),
        PROP_PRESENT_PRESET => {
            // The caller releases the name.
            let name = w.preset_name.lock().map(|n| n.clone()).unwrap_or_default();
            put(
                out,
                AUPreset {
                    preset_number: w.preset_number.load(Ordering::Relaxed),
                    preset_name: cf_string(&name),
                },
            );
        }
        PROP_PARAMETER_VALUE_STRINGS => {
            let Some((pi, _)) = w.decode(element) else {
                return ERR_INVALID_PARAMETER;
            };
            let strings: Vec<CFStringRef> = w.params.at(pi).info().choices.iter().map(|c| cf_string(c)).collect();
            let array = CFArrayCreate(
                null(),
                strings.as_ptr(),
                strings.len() as isize,
                &kCFTypeArrayCallBacks as *const u8 as *const c_void,
            );
            for s in strings {
                CFRelease(s);
            }
            put(out, array);
        }
        PROP_SET_RENDER_CALLBACK => {
            let cb = w.render_input.lock().map(|g| *g).unwrap_or(None);
            put(
                out,
                AURenderCallbackStruct {
                    input_proc: cb.map(|c| c.proc),
                    input_proc_ref_con: cb.map_or(null_mut(), |c| c.user as *mut c_void),
                },
            );
        }
        PROP_HOST_CALLBACKS => {
            let hc = w.host_callbacks.lock().map(|g| *g).unwrap_or(None);
            put(
                out,
                hc.unwrap_or(HostCallbackInfo {
                    host_user_data: null_mut(),
                    beat_and_tempo_proc: None,
                    musical_time_location_proc: None,
                    transport_state_proc: None,
                    transport_state_proc2: None,
                }),
            );
        }
        PROP_PARAMETER_STRING_FROM_VALUE => {
            let s = &mut *(out as *mut AudioUnitParameterStringFromValue);
            let Some((pi, _)) = w.decode(s.param_id) else {
                return ERR_INVALID_PARAMETER;
            };
            let value = if s.in_value.is_null() {
                match w.get_value(s.param_id) {
                    Some(v) => v,
                    None => return ERR_INVALID_PARAMETER,
                }
            } else {
                *s.in_value
            };
            s.out_string = cf_string(&w.params.at(pi).info().display(value));
        }
        PROP_PARAMETER_VALUE_FROM_STRING => {
            let s = &mut *(out as *mut AudioUnitParameterValueFromString);
            let Some((pi, _)) = w.decode(s.param_id) else {
                return ERR_INVALID_PARAMETER;
            };
            let Some(text) = string_from_cf(s.in_string) else {
                return ERR_INVALID_PROPERTY_VALUE;
            };
            match parse_value(w.params.at(pi).info(), &text) {
                Some(v) => s.out_value = v,
                None => return ERR_INVALID_PROPERTY_VALUE,
            }
        }
        PROP_PARAMETER_ID_NAME => {
            let s = &mut *(out as *mut AudioUnitParameterNameInfo);
            let Some((pi, ch)) = w.decode(s.id) else {
                return ERR_INVALID_PARAMETER;
            };
            let info = w.params.at(pi).info();
            let mut name = match ch {
                None => info.name.to_string(),
                Some(c) => format!("{} {}", info.name, c + 1),
            };
            if s.desired_length > 0 {
                while name.len() > s.desired_length as usize {
                    name.pop();
                }
            }
            s.out_name = cf_string(&name);
        }
        _ => return ERR_INVALID_PROPERTY,
    }
    NO_ERR
}

unsafe extern "C" fn get_property(
    this: *mut c_void,
    id: u32,
    scope: u32,
    element: u32,
    out: *mut c_void,
    io_size: *mut u32,
) -> OSStatus {
    if io_size.is_null() {
        return PARAM_ERR;
    }
    let w = inst(this);
    let (size, _) = match property_info(w, id, scope, element) {
        Ok(v) => v,
        Err(e) => return e,
    };
    if out.is_null() {
        *io_size = size;
        return NO_ERR;
    }
    if *io_size < size {
        return ERR_INVALID_PROPERTY_VALUE;
    }
    let status = write_property(w, id, scope, element, out);
    if status == NO_ERR {
        *io_size = size;
    }
    status
}

/// Checks a stream format and returns its channel count and sample rate.
fn accept_format(w: &Instance, f: &AudioStreamBasicDescription) -> Result<(u32, f64), OSStatus> {
    let wanted = FORMAT_FLAG_IS_FLOAT | FORMAT_FLAG_IS_PACKED | FORMAT_FLAG_IS_NON_INTERLEAVED;
    let ok = f.format_id == FORMAT_LINEAR_PCM
        && f.format_flags & wanted == wanted
        && f.bits_per_channel == 32
        && f.bytes_per_frame == 4
        && f.bytes_per_packet == 4
        && f.frames_per_packet == 1
        && f.channels_per_frame >= w.min_channels
        && f.channels_per_frame <= w.max_channels
        && f.sample_rate.is_finite()
        && f.sample_rate > 0.0;
    if ok {
        Ok((f.channels_per_frame, f.sample_rate))
    } else {
        Err(ERR_FORMAT_NOT_SUPPORTED)
    }
}

unsafe extern "C" fn set_property(this: *mut c_void, id: u32, scope: u32, element: u32, data: *const c_void, size: u32) -> OSStatus {
    let w = inst(this);
    let (wanted, writable) = match property_info(w, id, scope, element) {
        Ok(v) => v,
        Err(e) => return e,
    };
    if !writable {
        return ERR_PROPERTY_NOT_WRITABLE;
    }
    if data.is_null() || size != wanted {
        return ERR_INVALID_PROPERTY_VALUE;
    }
    let rd = |p: *const c_void| p;
    match id {
        PROP_CLASS_INFO => {
            let status = apply_class_info(w, *(rd(data) as *const CFDictionaryRef));
            if status == NO_ERR {
                w.notify(PROP_CLASS_INFO, SCOPE_GLOBAL, 0);
            }
            return status;
        }
        PROP_MAKE_CONNECTION => {
            let c = std::ptr::read_unaligned(data as *const AudioUnitConnection);
            if let Ok(mut g) = w.connection.lock() {
                *g = if c.source_audio_unit.is_null() { None } else { Some((c.source_audio_unit as usize, c.source_output_number)) };
            }
            if !c.source_audio_unit.is_null() {
                if let Ok(mut g) = w.render_input.lock() {
                    *g = None;
                }
            }
        }
        PROP_SET_RENDER_CALLBACK => {
            let cb = std::ptr::read_unaligned(data as *const AURenderCallbackStruct);
            if let Ok(mut g) = w.render_input.lock() {
                *g = cb.input_proc.map(|proc| RenderInput {
                    proc,
                    user: cb.input_proc_ref_con as usize,
                });
            }
            if cb.input_proc.is_some() {
                if let Ok(mut g) = w.connection.lock() {
                    *g = None;
                }
            }
        }
        PROP_SAMPLE_RATE => {
            if w.initialized.load(Ordering::Acquire) {
                return ERR_INITIALIZED;
            }
            let rate = std::ptr::read_unaligned(data as *const f64);
            if !rate.is_finite() || rate <= 0.0 {
                return ERR_INVALID_PROPERTY_VALUE;
            }
            w.sample_rate.store(rate.to_bits(), Ordering::Relaxed);
            w.notify(PROP_SAMPLE_RATE, SCOPE_INPUT, 0);
            w.notify(PROP_SAMPLE_RATE, SCOPE_OUTPUT, 0);
            w.notify(PROP_STREAM_FORMAT, SCOPE_INPUT, 0);
            w.notify(PROP_STREAM_FORMAT, SCOPE_OUTPUT, 0);
            return NO_ERR;
        }
        PROP_STREAM_FORMAT => {
            if w.initialized.load(Ordering::Acquire) {
                return ERR_INITIALIZED;
            }
            let f = std::ptr::read_unaligned(data as *const AudioStreamBasicDescription);
            let (channels, rate) = match accept_format(w, &f) {
                Ok(v) => v,
                Err(e) => return e,
            };
            // Each side keeps its own channel count, and initialize insists they match. The rate is shared.
            let side = if scope == SCOPE_INPUT { &w.in_channels } else { &w.out_channels };
            side.store(channels, Ordering::Relaxed);
            w.sample_rate.store(rate.to_bits(), Ordering::Relaxed);
            // The parameters follow the output, whose channel count is the one that renders.
            let changed_channels = scope == SCOPE_OUTPUT && w.params.channels() != channels as usize;
            if changed_channels {
                w.params.set_channels(channels as usize);
            }
            w.notify(PROP_STREAM_FORMAT, SCOPE_INPUT, 0);
            w.notify(PROP_STREAM_FORMAT, SCOPE_OUTPUT, 0);
            if changed_channels {
                w.notify(PROP_PARAMETER_LIST, SCOPE_GLOBAL, 0);
            }
            return NO_ERR;
        }
        PROP_MAXIMUM_FRAMES_PER_SLICE => {
            if w.initialized.load(Ordering::Acquire) {
                return ERR_INITIALIZED;
            }
            let frames = std::ptr::read_unaligned(data as *const u32);
            if frames == 0 || frames > 1 << 20 {
                return ERR_INVALID_PROPERTY_VALUE;
            }
            w.max_frames.store(frames, Ordering::Relaxed);
        }
        PROP_BYPASS_EFFECT => w.bypass.store(std::ptr::read_unaligned(data as *const u32) != 0, Ordering::Relaxed),
        PROP_LAST_RENDER_ERROR => w.last_render_error.store(std::ptr::read_unaligned(data as *const i32), Ordering::Relaxed),
        PROP_HOST_CALLBACKS => {
            let hc = std::ptr::read_unaligned(data as *const HostCallbackInfo);
            if let Ok(mut g) = w.host_callbacks.lock() {
                *g = if hc.beat_and_tempo_proc.is_none() && hc.transport_state_proc.is_none() { None } else { Some(hc) };
            }
        }
        PROP_IN_PLACE_PROCESSING | PROP_SHOULD_ALLOCATE_BUFFER => {}
        PROP_PRESENT_PRESET => {
            let p = std::ptr::read_unaligned(data as *const AUPreset);
            w.preset_number.store(p.preset_number, Ordering::Relaxed);
            if let Some(name) = string_from_cf(p.preset_name) {
                if let Ok(mut n) = w.preset_name.lock() {
                    *n = name;
                }
            }
        }
        _ => return ERR_PROPERTY_NOT_WRITABLE,
    }
    w.notify(id, scope, element);
    NO_ERR
}

unsafe extern "C" fn add_property_listener(this: *mut c_void, id: u32, proc: PropertyListenerProc, user: *mut c_void) -> OSStatus {
    if let Ok(mut l) = inst(this).listeners.lock() {
        l.push(Listener {
            id,
            proc,
            user: user as usize,
        });
    }
    NO_ERR
}

unsafe extern "C" fn remove_property_listener(this: *mut c_void, id: u32, proc: PropertyListenerProc) -> OSStatus {
    if let Ok(mut l) = inst(this).listeners.lock() {
        l.retain(|x| !(x.id == id && x.proc as usize == proc as usize));
    }
    NO_ERR
}

unsafe extern "C" fn remove_property_listener_with_user_data(
    this: *mut c_void,
    id: u32,
    proc: PropertyListenerProc,
    user: *mut c_void,
) -> OSStatus {
    if let Ok(mut l) = inst(this).listeners.lock() {
        l.retain(|x| !(x.id == id && x.proc as usize == proc as usize && x.user == user as usize));
    }
    NO_ERR
}

unsafe extern "C" fn add_render_notify(this: *mut c_void, proc: AURenderCallback, user: *mut c_void) -> OSStatus {
    if let Ok(mut n) = inst(this).notifies.lock() {
        n.push(Notify {
            proc,
            user: user as usize,
        });
    }
    NO_ERR
}

unsafe extern "C" fn remove_render_notify(this: *mut c_void, proc: AURenderCallback, user: *mut c_void) -> OSStatus {
    if let Ok(mut n) = inst(this).notifies.lock() {
        n.retain(|x| !(x.proc as usize == proc as usize && x.user == user as usize));
    }
    NO_ERR
}

// Parameters.

fn check_parameter(w: &Instance, id: u32, scope: u32, element: u32) -> Result<(), OSStatus> {
    check_global(scope, element)?;
    if w.decode(id).is_none() {
        return Err(ERR_INVALID_PARAMETER);
    }
    Ok(())
}

unsafe extern "C" fn get_parameter(this: *mut c_void, id: u32, scope: u32, element: u32, out: *mut f32) -> OSStatus {
    let w = inst(this);
    if let Err(e) = check_parameter(w, id, scope, element) {
        return e;
    }
    match (w.get_value(id), out.is_null()) {
        (Some(v), false) => {
            *out = v;
            NO_ERR
        }
        _ => PARAM_ERR,
    }
}

unsafe extern "C" fn set_parameter(this: *mut c_void, id: u32, scope: u32, element: u32, value: f32, _offset: u32) -> OSStatus {
    let w = inst(this);
    if let Err(e) = check_parameter(w, id, scope, element) {
        return e;
    }
    if !value.is_finite() || !w.set_value(id, value) {
        return ERR_INVALID_PARAMETER;
    }
    NO_ERR
}

unsafe extern "C" fn schedule_parameters(this: *mut c_void, events: *const AudioUnitParameterEvent, count: u32) -> OSStatus {
    let w = inst(this);
    if events.is_null() && count > 0 {
        return PARAM_ERR;
    }
    for i in 0..count as usize {
        let e = *events.add(i);
        if let Err(status) = check_parameter(w, e.parameter, e.scope, e.element) {
            return status;
        }
        // An immediate event carries its value in the second word. A ramp is applied at its end value.
        let value = match e.event_type {
            PARAM_EVENT_IMMEDIATE => f32::from_bits(e.event_values[1]),
            PARAM_EVENT_RAMPED => f32::from_bits(e.event_values[3]),
            _ => return ERR_INVALID_PROPERTY_VALUE,
        };
        if !value.is_finite() {
            return ERR_INVALID_PARAMETER;
        }
        if let Ok(mut p) = w.pending.lock() {
            p.push((e.parameter, value));
        }
    }
    NO_ERR
}

// Rendering.

unsafe fn run_notifies(w: &Instance, flags: *mut u32, ts: *const AudioTimeStamp, frames: u32, io: *mut AudioBufferList, phase: u32) {
    // The lock is only tried, so the audio thread never waits on a host thread that is adding a notify.
    let Ok(list) = w.notifies.try_lock() else {
        return;
    };
    for n in list.iter() {
        *flags |= phase;
        (n.proc)(n.user as *mut c_void, flags, ts, 0, frames, io);
        *flags &= !phase;
    }
}

unsafe extern "C" fn render(
    this: *mut c_void,
    flags: *mut u32,
    ts: *const AudioTimeStamp,
    bus: u32,
    frames: u32,
    io: *mut AudioBufferList,
) -> OSStatus {
    let w = inst(this);
    if !w.initialized.load(Ordering::Acquire) {
        return ERR_UNINITIALIZED;
    }
    if bus != 0 {
        return ERR_INVALID_ELEMENT;
    }
    if frames > w.max_frames.load(Ordering::Relaxed) {
        return ERR_TOO_MANY_FRAMES;
    }
    if io.is_null() {
        return PARAM_ERR;
    }
    let mut local_flags: u32 = 0;
    let flags = if flags.is_null() { &mut local_flags as *mut u32 } else { flags };
    let status = catch_unwind(AssertUnwindSafe(|| render_block(w, flags, ts, frames, io))).unwrap_or(PARAM_ERR);
    if status != NO_ERR {
        w.last_render_error.store(status, Ordering::Relaxed);
    }
    status
}

unsafe fn render_block(w: &Instance, flags: *mut u32, ts: *const AudioTimeStamp, frames: u32, io: *mut AudioBufferList) -> OSStatus {
    // Subnormal results become zero for the length of the block, whatever the plugin's own filters do.
    let _flush = FlushToZero::enable();
    let n = w.channel_count();
    if (*io).number_buffers as usize != n || n > MAX_CHANNELS {
        return ERR_FORMAT_NOT_SUPPORTED;
    }
    let buffers = std::ptr::addr_of_mut!((*io).buffers) as *mut AudioBuffer;
    let scratch = &mut *w.scratch.get();
    let bytes = frames * 4;
    for i in 0..n {
        let b = &mut *buffers.add(i);
        if b.data.is_null() {
            match scratch.get_mut(i) {
                Some(s) => b.data = s.as_mut_ptr() as *mut c_void,
                None => return ERR_UNINITIALIZED,
            }
        }
        b.data_byte_size = bytes;
        b.number_channels = 1;
    }

    run_notifies(w, flags, ts, frames, io, RENDER_PRE);

    // Pull the input into the same buffers. A source may swap in its own, and the plugin then works in those.
    let connection = w.connection.try_lock().ok().and_then(|g| *g);
    let callback = w.render_input.try_lock().ok().and_then(|g| *g);
    let status = if let Some((unit, source_bus)) = connection {
        AudioUnitRender(unit as *mut c_void, flags, ts, source_bus, frames, io)
    } else if let Some(cb) = callback {
        (cb.proc)(cb.user as *mut c_void, flags, ts, 0, frames, io)
    } else {
        ERR_NO_CONNECTION
    };
    if status != NO_ERR {
        return status;
    }

    if let Ok(mut pending) = w.pending.try_lock() {
        for (id, value) in pending.drain(..) {
            w.set_value(id, value);
        }
    }

    let slices = &mut *w.slices.get();
    slices.clear();
    for i in 0..n {
        let b = &mut *buffers.add(i);
        if b.data.is_null() {
            slices.clear();
            return PARAM_ERR;
        }
        b.data_byte_size = bytes;
        slices.push(std::slice::from_raw_parts_mut(b.data as *mut f32, frames as usize));
    }

    if !w.bypass.load(Ordering::Relaxed) {
        let rate = w.rate();
        let mut ctx = ProcessContext {
            sample_rate: rate as f32,
            sample_time: if !ts.is_null() && (*ts).flags & TIMESTAMP_SAMPLE_TIME_VALID != 0 {
                Some((*ts).sample_time)
            } else {
                None
            },
            playing: false,
            tempo_bpm: None,
        };
        if let Some(hc) = w.host_callbacks.try_lock().ok().and_then(|g| *g) {
            if let Some(proc) = hc.transport_state_proc {
                let (mut playing, mut changed, mut cycling) = (0u8, 0u8, 0u8);
                let (mut position, mut cycle_start, mut cycle_end) = (0f64, 0f64, 0f64);
                if proc(hc.host_user_data, &mut playing, &mut changed, &mut position, &mut cycling, &mut cycle_start, &mut cycle_end) == NO_ERR {
                    ctx.playing = playing != 0;
                    ctx.sample_time = Some(position);
                }
            }
            if let Some(proc) = hc.beat_and_tempo_proc {
                let (mut beat, mut tempo) = (0f64, 0f64);
                if proc(hc.host_user_data, &mut beat, &mut tempo) == NO_ERR && tempo > 0.0 {
                    ctx.tempo_bpm = Some(tempo);
                }
            }
        }
        let inner = &mut *w.inner.get();
        inner.process(&mut slices[..], &ctx);
        w.tail.store(inner.tail(), Ordering::Relaxed);
        w.latency.store(inner.latency(), Ordering::Relaxed);
    }
    slices.clear();

    *flags &= !RENDER_OUTPUT_IS_SILENCE;
    run_notifies(w, flags, ts, frames, io, RENDER_POST);
    NO_ERR
}
