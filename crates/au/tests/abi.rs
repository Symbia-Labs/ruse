//! The numbers below come from compiling tests/abi/layout.c against the macOS SDK headers.
//! A struct, field or constant that drifts from those headers fails here.
#![cfg(target_os = "macos")]

use muse_au::sys::*;
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
fn struct_sizes_match_the_headers() {
    sizes! {
        AudioComponentDescription => 20,
        PlugInInterface => 32,
        AudioStreamBasicDescription => 40,
        AudioBuffer => 16,
        AudioBufferList => 24,
        AudioTimeStamp => 64,
        AudioUnitParameterInfo => 104,
        AUChannelInfo => 4,
        AudioUnitConnection => 16,
        AURenderCallbackStruct => 16,
        HostCallbackInfo => 40,
        AudioUnitParameterEvent => 32,
        AudioUnitParameterStringFromValue => 24,
        AudioUnitParameterValueFromString => 24,
        AudioUnitParameterNameInfo => 16,
        AUPreset => 16,
    }
}

#[test]
fn field_offsets_match_the_headers() {
    offsets! {
        AudioComponentDescription, component_manufacturer => 8,
        AudioComponentDescription, component_flags_mask => 16,
        PlugInInterface, close => 8,
        PlugInInterface, lookup => 16,
        PlugInInterface, reserved => 24,
        AudioStreamBasicDescription, format_id => 8,
        AudioStreamBasicDescription, bytes_per_frame => 24,
        AudioStreamBasicDescription, channels_per_frame => 28,
        AudioStreamBasicDescription, reserved => 36,
        AudioBuffer, data_byte_size => 4,
        AudioBuffer, data => 8,
        AudioBufferList, buffers => 8,
        AudioTimeStamp, host_time => 8,
        AudioTimeStamp, rate_scalar => 16,
        AudioTimeStamp, smpte_time => 32,
        AudioTimeStamp, flags => 56,
        AudioUnitParameterInfo, unit_name => 56,
        AudioUnitParameterInfo, clump_id => 64,
        AudioUnitParameterInfo, cf_name_string => 72,
        AudioUnitParameterInfo, unit => 80,
        AudioUnitParameterInfo, min_value => 84,
        AudioUnitParameterInfo, default_value => 92,
        AudioUnitParameterInfo, flags => 96,
        AUChannelInfo, out_channels => 2,
        AudioUnitConnection, source_output_number => 8,
        AudioUnitConnection, dest_input_number => 12,
        AURenderCallbackStruct, input_proc_ref_con => 8,
        HostCallbackInfo, beat_and_tempo_proc => 8,
        HostCallbackInfo, transport_state_proc => 24,
        HostCallbackInfo, transport_state_proc2 => 32,
        AudioUnitParameterEvent, event_type => 12,
        AudioUnitParameterEvent, event_values => 16,
        AudioUnitParameterStringFromValue, out_string => 16,
        AudioUnitParameterValueFromString, out_value => 16,
        AudioUnitParameterNameInfo, out_name => 8,
    }
}

#[test]
fn selectors_and_errors_match_the_headers() {
    let selectors = [
        (SELECT_INITIALIZE, 1),
        (SELECT_UNINITIALIZE, 2),
        (SELECT_GET_PROPERTY_INFO, 3),
        (SELECT_GET_PROPERTY, 4),
        (SELECT_SET_PROPERTY, 5),
        (SELECT_GET_PARAMETER, 6),
        (SELECT_SET_PARAMETER, 7),
        (SELECT_RESET, 9),
        (SELECT_ADD_PROPERTY_LISTENER, 10),
        (SELECT_REMOVE_PROPERTY_LISTENER, 11),
        (SELECT_RENDER, 14),
        (SELECT_ADD_RENDER_NOTIFY, 15),
        (SELECT_REMOVE_RENDER_NOTIFY, 16),
        (SELECT_SCHEDULE_PARAMETERS, 17),
        (SELECT_REMOVE_PROPERTY_LISTENER_WITH_USER_DATA, 18),
    ];
    for (got, want) in selectors {
        assert_eq!(got, want);
    }
    let errors = [
        (ERR_INVALID_PROPERTY, -10879),
        (ERR_INVALID_PARAMETER, -10878),
        (ERR_INVALID_ELEMENT, -10877),
        (ERR_NO_CONNECTION, -10876),
        (ERR_FAILED_INITIALIZATION, -10875),
        (ERR_TOO_MANY_FRAMES, -10874),
        (ERR_FORMAT_NOT_SUPPORTED, -10868),
        (ERR_UNINITIALIZED, -10867),
        (ERR_INVALID_SCOPE, -10866),
        (ERR_PROPERTY_NOT_WRITABLE, -10865),
        (ERR_INVALID_PROPERTY_VALUE, -10851),
        (ERR_INITIALIZED, -10849),
    ];
    for (got, want) in errors {
        assert_eq!(got, want);
    }
}

#[test]
fn property_ids_scopes_and_flags_match_the_headers() {
    let properties = [
        (PROP_CLASS_INFO, 0),
        (PROP_MAKE_CONNECTION, 1),
        (PROP_SAMPLE_RATE, 2),
        (PROP_PARAMETER_LIST, 3),
        (PROP_PARAMETER_INFO, 4),
        (PROP_STREAM_FORMAT, 8),
        (PROP_ELEMENT_COUNT, 11),
        (PROP_LATENCY, 12),
        (PROP_SUPPORTED_NUM_CHANNELS, 13),
        (PROP_MAXIMUM_FRAMES_PER_SLICE, 14),
        (PROP_PARAMETER_VALUE_STRINGS, 16),
        (PROP_TAIL_TIME, 20),
        (PROP_BYPASS_EFFECT, 21),
        (PROP_LAST_RENDER_ERROR, 22),
        (PROP_SET_RENDER_CALLBACK, 23),
        (PROP_HOST_CALLBACKS, 27),
        (PROP_IN_PLACE_PROCESSING, 29),
        (PROP_PRESENT_PRESET, 36),
        (PROP_PARAMETER_STRING_FROM_VALUE, 33),
        (PROP_PARAMETER_ID_NAME, 34),
        (PROP_PARAMETER_VALUE_FROM_STRING, 38),
        (PROP_SHOULD_ALLOCATE_BUFFER, 51),
    ];
    for (got, want) in properties {
        assert_eq!(got, want);
    }
    assert_eq!((SCOPE_GLOBAL, SCOPE_INPUT, SCOPE_OUTPUT), (0, 1, 2));
    assert_eq!((RENDER_PRE, RENDER_POST, RENDER_OUTPUT_IS_SILENCE), (4, 8, 16));
    assert_eq!(
        (UNIT_GENERIC, UNIT_INDEXED, UNIT_BOOLEAN, UNIT_PERCENT, UNIT_SECONDS, UNIT_HERTZ, UNIT_DECIBELS, UNIT_MILLISECONDS),
        (0, 1, 2, 3, 4, 8, 13, 24)
    );
    assert_eq!(PARAM_FLAG_CF_NAME_RELEASE, 16);
    assert_eq!(PARAM_FLAG_METER_READ_ONLY, 32768);
    assert_eq!(PARAM_FLAG_VALUES_HAVE_STRINGS, 2097152);
    assert_eq!(PARAM_FLAG_DISPLAY_LOGARITHMIC, 4194304);
    assert_eq!(PARAM_FLAG_HAS_CF_NAME_STRING, 134217728);
    assert_eq!(PARAM_FLAG_IS_READABLE, 1073741824);
    assert_eq!(PARAM_FLAG_IS_WRITABLE, 2147483648);
    assert_eq!((PARAM_EVENT_IMMEDIATE, PARAM_EVENT_RAMPED), (1, 2));
    assert_eq!(FORMAT_LINEAR_PCM, 1819304813);
    assert_eq!(FORMAT_FLAGS_NATIVE_FLOAT_PACKED, FORMAT_FLAG_IS_FLOAT | FORMAT_FLAG_IS_PACKED);
    assert_eq!(FORMAT_FLAG_IS_NON_INTERLEAVED, 32);
    assert_eq!(TIMESTAMP_SAMPLE_TIME_VALID, 1);
    assert_eq!(TYPE_EFFECT, 1635083896);
    assert_eq!(CF_STRING_ENCODING_UTF8, 134217984);
    assert_eq!(CF_NUMBER_SINT32_TYPE, 3);
}

#[test]
fn four_character_codes_pack_the_way_apple_does() {
    assert_eq!(fourcc(b"lpcm"), 0x6c70_636d);
    assert_eq!(fourcc_string(fourcc(b"Wsat")), "Wsat");
}
