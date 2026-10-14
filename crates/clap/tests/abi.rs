//! The numbers below come from compiling tests/abi/layout.c against the real CLAP 1.2.10 header.
//! A struct or field that drifts from the header fails here.

use muse_clap::abi::*;
use std::mem::{offset_of, size_of};

macro_rules! sizes {
    ($($ty:ty => $size:expr),* $(,)?) => {
        $( assert_eq!(size_of::<$ty>(), $size, "size of {}", stringify!($ty)); )*
    };
}

macro_rules! offsets {
    ($($ty:ty, $field:ident => $offset:expr),* $(,)?) => {
        $( assert_eq!(offset_of!($ty, $field), $offset, "offset of {}.{}", stringify!($ty), stringify!($field)); )*
    };
}

#[test]
fn struct_sizes_match_the_header() {
    sizes! {
        Version => 12,
        PluginEntry => 40,
        PluginFactory => 24,
        PluginDescriptor => 88,
        Host => 88,
        Plugin => 96,
        Process => 64,
        AudioBuffer => 32,
        EventHeader => 16,
        EventParamValue => 56,
        EventTransport => 104,
        InputEvents => 24,
        OutputEvents => 16,
        AudioPortInfo => 288,
        AudioPortConfigurationRequest => 32,
        PluginAudioPorts => 16,
        PluginConfigurableAudioPorts => 16,
        ParamInfo => 1320,
        PluginParams => 48,
        HostParams => 24,
        IStream => 16,
        OStream => 16,
        PluginState => 16,
        PluginLatency => 8,
        PluginTail => 8,
        HostLatency => 8,
    }
}

#[test]
fn field_offsets_match_the_header() {
    offsets! {
        PluginEntry, init => 16,
        PluginEntry, deinit => 24,
        PluginEntry, get_factory => 32,
        PluginFactory, create_plugin => 16,
        PluginDescriptor, id => 16,
        PluginDescriptor, description => 72,
        PluginDescriptor, features => 80,
        Host, host_data => 16,
        Host, get_extension => 56,
        Host, request_callback => 80,
        Plugin, plugin_data => 8,
        Plugin, activate => 32,
        Plugin, process => 72,
        Plugin, on_main_thread => 88,
        Process, frames_count => 8,
        Process, transport => 16,
        Process, audio_outputs => 32,
        Process, audio_inputs_count => 40,
        Process, in_events => 48,
        Process, out_events => 56,
        AudioBuffer, channel_count => 16,
        AudioBuffer, constant_mask => 24,
        EventHeader, space_id => 8,
        EventHeader, flags => 12,
        EventParamValue, param_id => 16,
        EventParamValue, cookie => 24,
        EventParamValue, note_id => 32,
        EventParamValue, channel => 38,
        EventParamValue, value => 48,
        EventTransport, flags => 16,
        EventTransport, song_pos_seconds => 32,
        EventTransport, tempo => 40,
        EventTransport, bar_start => 88,
        EventTransport, bar_number => 96,
        EventTransport, tsig_num => 100,
        InputEvents, get => 16,
        OutputEvents, try_push => 8,
        AudioPortInfo, flags => 260,
        AudioPortInfo, port_type => 272,
        AudioPortInfo, in_place_pair => 280,
        AudioPortConfigurationRequest, channel_count => 8,
        AudioPortConfigurationRequest, port_type => 16,
        ParamInfo, cookie => 8,
        ParamInfo, name => 16,
        ParamInfo, module => 272,
        ParamInfo, min_value => 1296,
        ParamInfo, default_value => 1312,
        PluginParams, flush => 40,
        IStream, read => 8,
        OStream, write => 8,
    }
}

#[test]
fn constants_match_the_header() {
    assert_eq!(CLAP_VERSION, Version { major: 1, minor: 2, revision: 10 });
    assert_eq!(EVENT_PARAM_VALUE, 5);
    assert_eq!(TRANSPORT_HAS_TEMPO | TRANSPORT_HAS_SECONDS_TIMELINE | TRANSPORT_IS_PLAYING, 0b10101);
    assert_eq!(SECTIME_FACTOR, 2147483648.0);
    assert_eq!(PARAM_IS_ENUM, 65536);
    assert!(PLUGIN_FACTORY_ID.ends_with(b"\0"));
}
