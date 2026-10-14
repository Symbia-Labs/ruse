//! The CLAP C interface, written out by hand for CLAP 1.2.10.
//!
//! The layouts are checked against the real `clap/clap.h` by `tests/abi.rs`. The sizes and offsets
//! there came from compiling a C program against the header, so a slip here shows up as a failing test.

use std::ffi::{c_char, c_void, CStr};

pub type ClapId = u32;
pub const INVALID_ID: ClapId = u32::MAX;
pub const NAME_SIZE: usize = 256;
pub const PATH_SIZE: usize = 1024;

#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Version {
    pub major: u32,
    pub minor: u32,
    pub revision: u32,
}

pub const CLAP_VERSION: Version = Version {
    major: 1,
    minor: 2,
    revision: 10,
};

impl Version {
    pub fn is_compatible(self) -> bool {
        self.major >= 1
    }
}

// Extension and factory identifiers, with the terminating zero C expects.
pub const PLUGIN_FACTORY_ID: &[u8] = b"clap.plugin-factory\0";
pub const EXT_AUDIO_PORTS: &[u8] = b"clap.audio-ports\0";
pub const EXT_CONFIGURABLE_AUDIO_PORTS: &[u8] = b"clap.configurable-audio-ports/1\0";
pub const EXT_CONFIGURABLE_AUDIO_PORTS_COMPAT: &[u8] = b"clap.configurable-audio-ports.draft1\0";
pub const EXT_PARAMS: &[u8] = b"clap.params\0";
pub const EXT_STATE: &[u8] = b"clap.state\0";
pub const EXT_LATENCY: &[u8] = b"clap.latency\0";
pub const EXT_TAIL: &[u8] = b"clap.tail\0";
pub const PORT_MONO: &[u8] = b"mono\0";
pub const PORT_STEREO: &[u8] = b"stereo\0";

/// Whether a C string from the other side of the interface equals one of the identifiers above.
///
/// # Safety
/// `id` must be null or point to a NUL-terminated string.
pub unsafe fn is_id(id: *const c_char, expected: &[u8]) -> bool {
    !id.is_null() && CStr::from_ptr(id).to_bytes_with_nul() == expected
}

pub const PROCESS_ERROR: i32 = 0;
pub const PROCESS_CONTINUE: i32 = 1;

pub const EVENT_SPACE_CORE: u16 = 0;
pub const EVENT_PARAM_VALUE: u16 = 5;

pub const TRANSPORT_HAS_TEMPO: u32 = 1 << 0;
pub const TRANSPORT_HAS_SECONDS_TIMELINE: u32 = 1 << 2;
pub const TRANSPORT_IS_PLAYING: u32 = 1 << 4;
pub const SECTIME_FACTOR: f64 = (1u64 << 31) as f64;

pub const AUDIO_PORT_IS_MAIN: u32 = 1 << 0;

pub const PARAM_IS_STEPPED: u32 = 1 << 0;
pub const PARAM_IS_READONLY: u32 = 1 << 3;
pub const PARAM_IS_BYPASS: u32 = 1 << 4;
pub const PARAM_IS_AUTOMATABLE: u32 = 1 << 5;
pub const PARAM_IS_ENUM: u32 = 1 << 16;
pub const PARAM_RESCAN_ALL: u32 = 1 << 3;

#[repr(C)]
pub struct PluginEntry {
    pub clap_version: Version,
    pub init: unsafe extern "C" fn(plugin_path: *const c_char) -> bool,
    pub deinit: unsafe extern "C" fn(),
    pub get_factory: unsafe extern "C" fn(factory_id: *const c_char) -> *const c_void,
}

#[repr(C)]
pub struct PluginDescriptor {
    pub clap_version: Version,
    pub id: *const c_char,
    pub name: *const c_char,
    pub vendor: *const c_char,
    pub url: *const c_char,
    pub manual_url: *const c_char,
    pub support_url: *const c_char,
    pub version: *const c_char,
    pub description: *const c_char,
    pub features: *const *const c_char,
}

#[repr(C)]
pub struct PluginFactory {
    pub get_plugin_count: unsafe extern "C" fn(factory: *const PluginFactory) -> u32,
    pub get_plugin_descriptor: unsafe extern "C" fn(factory: *const PluginFactory, index: u32) -> *const PluginDescriptor,
    pub create_plugin:
        unsafe extern "C" fn(factory: *const PluginFactory, host: *const Host, plugin_id: *const c_char) -> *const Plugin,
}

#[repr(C)]
pub struct Host {
    pub clap_version: Version,
    pub host_data: *mut c_void,
    pub name: *const c_char,
    pub vendor: *const c_char,
    pub url: *const c_char,
    pub version: *const c_char,
    pub get_extension: unsafe extern "C" fn(host: *const Host, extension_id: *const c_char) -> *const c_void,
    pub request_restart: unsafe extern "C" fn(host: *const Host),
    pub request_process: unsafe extern "C" fn(host: *const Host),
    pub request_callback: unsafe extern "C" fn(host: *const Host),
}

#[repr(C)]
pub struct Plugin {
    pub desc: *const PluginDescriptor,
    pub plugin_data: *mut c_void,
    pub init: unsafe extern "C" fn(plugin: *const Plugin) -> bool,
    pub destroy: unsafe extern "C" fn(plugin: *const Plugin),
    pub activate:
        unsafe extern "C" fn(plugin: *const Plugin, sample_rate: f64, min_frames_count: u32, max_frames_count: u32) -> bool,
    pub deactivate: unsafe extern "C" fn(plugin: *const Plugin),
    pub start_processing: unsafe extern "C" fn(plugin: *const Plugin) -> bool,
    pub stop_processing: unsafe extern "C" fn(plugin: *const Plugin),
    pub reset: unsafe extern "C" fn(plugin: *const Plugin),
    pub process: unsafe extern "C" fn(plugin: *const Plugin, process: *const Process) -> i32,
    pub get_extension: unsafe extern "C" fn(plugin: *const Plugin, id: *const c_char) -> *const c_void,
    pub on_main_thread: unsafe extern "C" fn(plugin: *const Plugin),
}

#[repr(C)]
pub struct AudioBuffer {
    pub data32: *mut *mut f32,
    pub data64: *mut *mut f64,
    pub channel_count: u32,
    pub latency: u32,
    pub constant_mask: u64,
}

#[repr(C)]
pub struct Process {
    pub steady_time: i64,
    pub frames_count: u32,
    pub transport: *const EventTransport,
    pub audio_inputs: *const AudioBuffer,
    pub audio_outputs: *mut AudioBuffer,
    pub audio_inputs_count: u32,
    pub audio_outputs_count: u32,
    pub in_events: *const InputEvents,
    pub out_events: *const OutputEvents,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct EventHeader {
    pub size: u32,
    pub time: u32,
    pub space_id: u16,
    pub type_: u16,
    pub flags: u32,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct EventParamValue {
    pub header: EventHeader,
    pub param_id: ClapId,
    pub cookie: *mut c_void,
    pub note_id: i32,
    pub port_index: i16,
    pub channel: i16,
    pub key: i16,
    pub value: f64,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct EventTransport {
    pub header: EventHeader,
    pub flags: u32,
    pub song_pos_beats: i64,
    pub song_pos_seconds: i64,
    pub tempo: f64,
    pub tempo_inc: f64,
    pub loop_start_beats: i64,
    pub loop_end_beats: i64,
    pub loop_start_seconds: i64,
    pub loop_end_seconds: i64,
    pub bar_start: i64,
    pub bar_number: i32,
    pub tsig_num: u16,
    pub tsig_denom: u16,
}

#[repr(C)]
pub struct InputEvents {
    pub ctx: *mut c_void,
    pub size: unsafe extern "C" fn(list: *const InputEvents) -> u32,
    pub get: unsafe extern "C" fn(list: *const InputEvents, index: u32) -> *const EventHeader,
}

#[repr(C)]
pub struct OutputEvents {
    pub ctx: *mut c_void,
    pub try_push: unsafe extern "C" fn(list: *const OutputEvents, event: *const EventHeader) -> bool,
}

#[repr(C)]
pub struct AudioPortInfo {
    pub id: ClapId,
    pub name: [c_char; NAME_SIZE],
    pub flags: u32,
    pub channel_count: u32,
    pub port_type: *const c_char,
    pub in_place_pair: ClapId,
}

#[repr(C)]
pub struct PluginAudioPorts {
    pub count: unsafe extern "C" fn(plugin: *const Plugin, is_input: bool) -> u32,
    pub get: unsafe extern "C" fn(plugin: *const Plugin, index: u32, is_input: bool, info: *mut AudioPortInfo) -> bool,
}

#[repr(C)]
pub struct AudioPortConfigurationRequest {
    pub is_input: bool,
    pub port_index: u32,
    pub channel_count: u32,
    pub port_type: *const c_char,
    pub port_details: *const c_void,
}

#[repr(C)]
pub struct PluginConfigurableAudioPorts {
    pub can_apply_configuration:
        unsafe extern "C" fn(plugin: *const Plugin, requests: *const AudioPortConfigurationRequest, request_count: u32) -> bool,
    pub apply_configuration:
        unsafe extern "C" fn(plugin: *const Plugin, requests: *const AudioPortConfigurationRequest, request_count: u32) -> bool,
}

#[repr(C)]
pub struct ParamInfo {
    pub id: ClapId,
    pub flags: u32,
    pub cookie: *mut c_void,
    pub name: [c_char; NAME_SIZE],
    pub module: [c_char; PATH_SIZE],
    pub min_value: f64,
    pub max_value: f64,
    pub default_value: f64,
}

#[repr(C)]
pub struct PluginParams {
    pub count: unsafe extern "C" fn(plugin: *const Plugin) -> u32,
    pub get_info: unsafe extern "C" fn(plugin: *const Plugin, param_index: u32, param_info: *mut ParamInfo) -> bool,
    pub get_value: unsafe extern "C" fn(plugin: *const Plugin, param_id: ClapId, out_value: *mut f64) -> bool,
    pub value_to_text: unsafe extern "C" fn(
        plugin: *const Plugin,
        param_id: ClapId,
        value: f64,
        out_buffer: *mut c_char,
        out_buffer_capacity: u32,
    ) -> bool,
    pub text_to_value:
        unsafe extern "C" fn(plugin: *const Plugin, param_id: ClapId, param_value_text: *const c_char, out_value: *mut f64) -> bool,
    pub flush: unsafe extern "C" fn(plugin: *const Plugin, in_events: *const InputEvents, out_events: *const OutputEvents),
}

#[repr(C)]
pub struct HostParams {
    pub rescan: unsafe extern "C" fn(host: *const Host, flags: u32),
    pub clear: unsafe extern "C" fn(host: *const Host, param_id: ClapId, flags: u32),
    pub request_flush: unsafe extern "C" fn(host: *const Host),
}

#[repr(C)]
pub struct IStream {
    pub ctx: *mut c_void,
    pub read: unsafe extern "C" fn(stream: *const IStream, buffer: *mut c_void, size: u64) -> i64,
}

#[repr(C)]
pub struct OStream {
    pub ctx: *mut c_void,
    pub write: unsafe extern "C" fn(stream: *const OStream, buffer: *const c_void, size: u64) -> i64,
}

#[repr(C)]
pub struct PluginState {
    pub save: unsafe extern "C" fn(plugin: *const Plugin, stream: *const OStream) -> bool,
    pub load: unsafe extern "C" fn(plugin: *const Plugin, stream: *const IStream) -> bool,
}

#[repr(C)]
pub struct PluginLatency {
    pub get: unsafe extern "C" fn(plugin: *const Plugin) -> u32,
}

#[repr(C)]
pub struct PluginTail {
    pub get: unsafe extern "C" fn(plugin: *const Plugin) -> u32,
}

#[repr(C)]
pub struct HostLatency {
    pub changed: unsafe extern "C" fn(host: *const Host),
}
