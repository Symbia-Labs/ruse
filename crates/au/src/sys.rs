//! The Audio Unit, Core Audio and Core Foundation interface MUSE uses, written out by hand.
//!
//! The layouts and constants are checked against the macOS SDK headers by `tests/abi.rs`. The numbers
//! there came from compiling `tests/abi/layout.c` against those headers.

use std::ffi::{c_char, c_void};

pub type OSStatus = i32;
pub type OSType = u32;
pub type AudioUnitPropertyID = u32;
pub type AudioUnitScope = u32;
pub type AudioUnitElement = u32;
pub type AudioUnitParameterID = u32;
pub type AudioUnitRenderActionFlags = u32;
pub type AudioComponentInstance = *mut c_void;

pub type CFTypeRef = *const c_void;
pub type CFStringRef = *const c_void;
pub type CFDataRef = *const c_void;
pub type CFNumberRef = *const c_void;
pub type CFArrayRef = *const c_void;
pub type CFDictionaryRef = *const c_void;
pub type CFMutableDictionaryRef = *mut c_void;

/// Packs a four-character code the way Apple's headers do.
pub const fn fourcc(s: &[u8; 4]) -> OSType {
    ((s[0] as u32) << 24) | ((s[1] as u32) << 16) | ((s[2] as u32) << 8) | (s[3] as u32)
}

pub fn fourcc_string(code: OSType) -> String {
    code.to_be_bytes().iter().map(|b| *b as char).collect()
}

// Errors.
pub const NO_ERR: OSStatus = 0;
pub const PARAM_ERR: OSStatus = -50;
pub const ERR_INVALID_PROPERTY: OSStatus = -10879;
pub const ERR_INVALID_PARAMETER: OSStatus = -10878;
pub const ERR_INVALID_ELEMENT: OSStatus = -10877;
pub const ERR_NO_CONNECTION: OSStatus = -10876;
pub const ERR_FAILED_INITIALIZATION: OSStatus = -10875;
pub const ERR_TOO_MANY_FRAMES: OSStatus = -10874;
pub const ERR_FORMAT_NOT_SUPPORTED: OSStatus = -10868;
pub const ERR_UNINITIALIZED: OSStatus = -10867;
pub const ERR_INVALID_SCOPE: OSStatus = -10866;
pub const ERR_PROPERTY_NOT_WRITABLE: OSStatus = -10865;
pub const ERR_INVALID_PROPERTY_VALUE: OSStatus = -10851;
pub const ERR_INITIALIZED: OSStatus = -10849;

// Method selectors.
pub const SELECT_INITIALIZE: i16 = 1;
pub const SELECT_UNINITIALIZE: i16 = 2;
pub const SELECT_GET_PROPERTY_INFO: i16 = 3;
pub const SELECT_GET_PROPERTY: i16 = 4;
pub const SELECT_SET_PROPERTY: i16 = 5;
pub const SELECT_GET_PARAMETER: i16 = 6;
pub const SELECT_SET_PARAMETER: i16 = 7;
pub const SELECT_RESET: i16 = 9;
pub const SELECT_ADD_PROPERTY_LISTENER: i16 = 10;
pub const SELECT_REMOVE_PROPERTY_LISTENER: i16 = 11;
pub const SELECT_RENDER: i16 = 14;
pub const SELECT_ADD_RENDER_NOTIFY: i16 = 15;
pub const SELECT_REMOVE_RENDER_NOTIFY: i16 = 16;
pub const SELECT_SCHEDULE_PARAMETERS: i16 = 17;
pub const SELECT_REMOVE_PROPERTY_LISTENER_WITH_USER_DATA: i16 = 18;

// Scopes.
pub const SCOPE_GLOBAL: u32 = 0;
pub const SCOPE_INPUT: u32 = 1;
pub const SCOPE_OUTPUT: u32 = 2;

// Properties.
pub const PROP_CLASS_INFO: u32 = 0;
pub const PROP_MAKE_CONNECTION: u32 = 1;
pub const PROP_SAMPLE_RATE: u32 = 2;
pub const PROP_PARAMETER_LIST: u32 = 3;
pub const PROP_PARAMETER_INFO: u32 = 4;
pub const PROP_STREAM_FORMAT: u32 = 8;
pub const PROP_ELEMENT_COUNT: u32 = 11;
pub const PROP_LATENCY: u32 = 12;
pub const PROP_SUPPORTED_NUM_CHANNELS: u32 = 13;
pub const PROP_MAXIMUM_FRAMES_PER_SLICE: u32 = 14;
pub const PROP_PARAMETER_VALUE_STRINGS: u32 = 16;
pub const PROP_TAIL_TIME: u32 = 20;
pub const PROP_BYPASS_EFFECT: u32 = 21;
pub const PROP_LAST_RENDER_ERROR: u32 = 22;
pub const PROP_SET_RENDER_CALLBACK: u32 = 23;
pub const PROP_HOST_CALLBACKS: u32 = 27;
pub const PROP_IN_PLACE_PROCESSING: u32 = 29;
pub const PROP_PRESENT_PRESET: u32 = 36;
pub const PROP_PARAMETER_STRING_FROM_VALUE: u32 = 33;
pub const PROP_PARAMETER_ID_NAME: u32 = 34;
pub const PROP_PARAMETER_VALUE_FROM_STRING: u32 = 38;
pub const PROP_SHOULD_ALLOCATE_BUFFER: u32 = 51;

// Render action flags.
pub const RENDER_PRE: u32 = 1 << 2;
pub const RENDER_POST: u32 = 1 << 3;
pub const RENDER_OUTPUT_IS_SILENCE: u32 = 1 << 4;

// Parameter units and flags.
pub const UNIT_GENERIC: u32 = 0;
pub const UNIT_INDEXED: u32 = 1;
pub const UNIT_BOOLEAN: u32 = 2;
pub const UNIT_PERCENT: u32 = 3;
pub const UNIT_SECONDS: u32 = 4;
pub const UNIT_HERTZ: u32 = 8;
pub const UNIT_DECIBELS: u32 = 13;
pub const UNIT_MILLISECONDS: u32 = 24;
pub const PARAM_FLAG_CF_NAME_RELEASE: u32 = 1 << 4;
pub const PARAM_FLAG_METER_READ_ONLY: u32 = 1 << 15;
pub const PARAM_FLAG_VALUES_HAVE_STRINGS: u32 = 1 << 21;
pub const PARAM_FLAG_DISPLAY_LOGARITHMIC: u32 = 1 << 22;
pub const PARAM_FLAG_HAS_CF_NAME_STRING: u32 = 1 << 27;
pub const PARAM_FLAG_IS_READABLE: u32 = 1 << 30;
pub const PARAM_FLAG_IS_WRITABLE: u32 = 1 << 31;
pub const PARAM_EVENT_IMMEDIATE: u32 = 1;
pub const PARAM_EVENT_RAMPED: u32 = 2;

// Audio formats.
pub const FORMAT_LINEAR_PCM: u32 = fourcc(b"lpcm");
pub const FORMAT_FLAG_IS_FLOAT: u32 = 1;
pub const FORMAT_FLAG_IS_PACKED: u32 = 8;
pub const FORMAT_FLAG_IS_NON_INTERLEAVED: u32 = 32;
pub const FORMAT_FLAGS_NATIVE_FLOAT_PACKED: u32 = 9;
pub const TIMESTAMP_SAMPLE_TIME_VALID: u32 = 1;
pub const TYPE_EFFECT: OSType = fourcc(b"aufx");

pub const CF_STRING_ENCODING_UTF8: u32 = 0x0800_0100;
pub const CF_NUMBER_SINT32_TYPE: isize = 3;

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct AudioComponentDescription {
    pub component_type: OSType,
    pub component_sub_type: OSType,
    pub component_manufacturer: OSType,
    pub component_flags: u32,
    pub component_flags_mask: u32,
}

/// What a host calls through. `lookup` hands back the function for each selector.
#[repr(C)]
pub struct PlugInInterface {
    pub open: unsafe extern "C" fn(this: *mut c_void, instance: AudioComponentInstance) -> OSStatus,
    pub close: unsafe extern "C" fn(this: *mut c_void) -> OSStatus,
    pub lookup: unsafe extern "C" fn(selector: i16) -> Option<unsafe extern "C" fn()>,
    pub reserved: *mut c_void,
}

pub type FactoryFunction = unsafe extern "C" fn(desc: *const AudioComponentDescription) -> *mut PlugInInterface;

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct AudioStreamBasicDescription {
    pub sample_rate: f64,
    pub format_id: u32,
    pub format_flags: u32,
    pub bytes_per_packet: u32,
    pub frames_per_packet: u32,
    pub bytes_per_frame: u32,
    pub channels_per_frame: u32,
    pub bits_per_channel: u32,
    pub reserved: u32,
}

impl AudioStreamBasicDescription {
    /// 32-bit float, one buffer for each channel, which is how effects exchange audio.
    pub fn float_planar(sample_rate: f64, channels: u32) -> Self {
        AudioStreamBasicDescription {
            sample_rate,
            format_id: FORMAT_LINEAR_PCM,
            format_flags: FORMAT_FLAGS_NATIVE_FLOAT_PACKED | FORMAT_FLAG_IS_NON_INTERLEAVED,
            bytes_per_packet: 4,
            frames_per_packet: 1,
            bytes_per_frame: 4,
            channels_per_frame: channels,
            bits_per_channel: 32,
            reserved: 0,
        }
    }
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct AudioBuffer {
    pub number_channels: u32,
    pub data_byte_size: u32,
    pub data: *mut c_void,
}

/// The list holds `number_buffers` buffers. The array is declared with one, as in C, and a host
/// allocates room for more.
#[repr(C)]
pub struct AudioBufferList {
    pub number_buffers: u32,
    pub buffers: [AudioBuffer; 1],
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct SmpteTime {
    pub subframes: i16,
    pub subframe_divisor: i16,
    pub counter: u32,
    pub kind: u32,
    pub flags: u32,
    pub hours: i16,
    pub minutes: i16,
    pub seconds: i16,
    pub frames: i16,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct AudioTimeStamp {
    pub sample_time: f64,
    pub host_time: u64,
    pub rate_scalar: f64,
    pub word_clock_time: u64,
    pub smpte_time: SmpteTime,
    pub flags: u32,
    pub reserved: u32,
}

#[repr(C)]
pub struct AudioUnitParameterInfo {
    pub name: [c_char; 52],
    pub unit_name: CFStringRef,
    pub clump_id: u32,
    pub cf_name_string: CFStringRef,
    pub unit: u32,
    pub min_value: f32,
    pub max_value: f32,
    pub default_value: f32,
    pub flags: u32,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct AUChannelInfo {
    pub in_channels: i16,
    pub out_channels: i16,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct AudioUnitConnection {
    pub source_audio_unit: AudioComponentInstance,
    pub source_output_number: u32,
    pub dest_input_number: u32,
}

pub type AURenderCallback = unsafe extern "C" fn(
    ref_con: *mut c_void,
    action_flags: *mut AudioUnitRenderActionFlags,
    time_stamp: *const AudioTimeStamp,
    bus_number: u32,
    number_frames: u32,
    io_data: *mut AudioBufferList,
) -> OSStatus;

#[repr(C)]
#[derive(Clone, Copy)]
pub struct AURenderCallbackStruct {
    pub input_proc: Option<AURenderCallback>,
    pub input_proc_ref_con: *mut c_void,
}

pub type HostBeatAndTempo = unsafe extern "C" fn(user: *mut c_void, beat: *mut f64, tempo: *mut f64) -> OSStatus;
pub type HostTransportState = unsafe extern "C" fn(
    user: *mut c_void,
    playing: *mut u8,
    changed: *mut u8,
    sample_in_timeline: *mut f64,
    cycling: *mut u8,
    cycle_start_beat: *mut f64,
    cycle_end_beat: *mut f64,
) -> OSStatus;

#[repr(C)]
#[derive(Clone, Copy)]
pub struct HostCallbackInfo {
    pub host_user_data: *mut c_void,
    pub beat_and_tempo_proc: Option<HostBeatAndTempo>,
    pub musical_time_location_proc: Option<unsafe extern "C" fn()>,
    pub transport_state_proc: Option<HostTransportState>,
    pub transport_state_proc2: Option<unsafe extern "C" fn()>,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct AudioUnitParameterEvent {
    pub scope: AudioUnitScope,
    pub element: AudioUnitElement,
    pub parameter: AudioUnitParameterID,
    pub event_type: u32,
    /// Either `{startBufferOffset, durationInFrames, startValue, endValue}` for a ramp or
    /// `{bufferOffset, value}` for an immediate event, as raw words.
    pub event_values: [u32; 4],
}

#[repr(C)]
pub struct AudioUnitParameterStringFromValue {
    pub param_id: AudioUnitParameterID,
    pub in_value: *const f32,
    pub out_string: CFStringRef,
}

#[repr(C)]
pub struct AudioUnitParameterValueFromString {
    pub param_id: AudioUnitParameterID,
    pub in_string: CFStringRef,
    pub out_value: f32,
}

#[repr(C)]
pub struct AudioUnitParameterNameInfo {
    pub id: AudioUnitParameterID,
    pub desired_length: i32,
    pub out_name: CFStringRef,
}

#[repr(C)]
pub struct AUPreset {
    pub preset_number: i32,
    pub preset_name: CFStringRef,
}

#[link(name = "CoreFoundation", kind = "framework")]
extern "C" {
    pub static kCFTypeDictionaryKeyCallBacks: u8;
    pub static kCFTypeDictionaryValueCallBacks: u8;
    pub static kCFTypeArrayCallBacks: u8;

    pub fn CFRelease(cf: CFTypeRef);
    pub fn CFGetTypeID(cf: CFTypeRef) -> usize;
    pub fn CFStringCreateWithBytes(alloc: *const c_void, bytes: *const u8, count: isize, encoding: u32, external: u8) -> CFStringRef;
    pub fn CFStringGetCString(s: CFStringRef, buffer: *mut c_char, size: isize, encoding: u32) -> u8;
    pub fn CFStringGetTypeID() -> usize;
    pub fn CFDataCreate(alloc: *const c_void, bytes: *const u8, length: isize) -> CFDataRef;
    pub fn CFDataGetBytePtr(data: CFDataRef) -> *const u8;
    pub fn CFDataGetLength(data: CFDataRef) -> isize;
    pub fn CFDataGetTypeID() -> usize;
    pub fn CFNumberCreate(alloc: *const c_void, kind: isize, value: *const c_void) -> CFNumberRef;
    pub fn CFNumberGetValue(number: CFNumberRef, kind: isize, value: *mut c_void) -> u8;
    pub fn CFNumberGetTypeID() -> usize;
    pub fn CFArrayCreate(alloc: *const c_void, values: *const *const c_void, count: isize, callbacks: *const c_void) -> CFArrayRef;
    pub fn CFArrayGetCount(array: CFArrayRef) -> isize;
    pub fn CFArrayGetValueAtIndex(array: CFArrayRef, index: isize) -> *const c_void;
    pub fn CFDictionaryCreateMutable(
        alloc: *const c_void,
        capacity: isize,
        key_callbacks: *const c_void,
        value_callbacks: *const c_void,
    ) -> CFMutableDictionaryRef;
    pub fn CFDictionarySetValue(dict: CFMutableDictionaryRef, key: *const c_void, value: *const c_void);
    pub fn CFDictionaryGetValue(dict: CFDictionaryRef, key: *const c_void) -> *const c_void;
    pub fn CFDictionaryGetTypeID() -> usize;
}

#[link(name = "AudioToolbox", kind = "framework")]
extern "C" {
    pub fn AudioUnitRender(
        unit: AudioComponentInstance,
        action_flags: *mut AudioUnitRenderActionFlags,
        time_stamp: *const AudioTimeStamp,
        output_bus_number: u32,
        number_frames: u32,
        io_data: *mut AudioBufferList,
    ) -> OSStatus;
}

/// A Core Foundation string made from Rust text. Release it with `CFRelease` unless a caller takes it.
pub fn cf_string(s: &str) -> CFStringRef {
    unsafe { CFStringCreateWithBytes(std::ptr::null(), s.as_ptr(), s.len() as isize, CF_STRING_ENCODING_UTF8, 0) }
}

/// The text of a Core Foundation string, or `None` if it is not one.
///
/// # Safety
/// `s` must be null or a valid `CFStringRef`.
pub unsafe fn string_from_cf(s: CFStringRef) -> Option<String> {
    if s.is_null() || CFGetTypeID(s) != CFStringGetTypeID() {
        return None;
    }
    let mut buf = vec![0 as c_char; 1024];
    if CFStringGetCString(s, buf.as_mut_ptr(), buf.len() as isize, CF_STRING_ENCODING_UTF8) == 0 {
        return None;
    }
    Some(std::ffi::CStr::from_ptr(buf.as_ptr()).to_string_lossy().to_string())
}
