//! Checks an Audio Unit through its factory and interface, the way a host drives one.
//!
//! The same checks run against a plugin linked into a test and against a built `.component` loaded
//! with `dlopen`, so the verifier and the tests cannot drift apart.

use crate::sys::*;
use std::ffi::c_void;
use std::mem::size_of;
use std::ptr::{null, null_mut};

/// Collects the result of each check and prints it as it happens.
pub struct Report {
    pub lines: Vec<(String, bool, String)>,
    echo: bool,
}

impl Report {
    pub fn new(echo: bool) -> Self {
        Report { lines: Vec::new(), echo }
    }

    pub fn check(&mut self, name: &str, ok: bool, detail: impl Into<String>) -> bool {
        let detail = detail.into();
        if self.echo {
            println!("CHECK {} {} {}", name, if ok { "PASS" } else { "FAIL" }, detail);
        }
        self.lines.push((name.to_string(), ok, detail));
        ok
    }

    pub fn all_passed(&self) -> bool {
        self.lines.iter().all(|l| l.1)
    }

    pub fn passed(&self) -> usize {
        self.lines.iter().filter(|l| l.1).count()
    }

    pub fn finish(&self) -> bool {
        println!("CHECKS {} of {} passed", self.passed(), self.lines.len());
        self.all_passed()
    }
}

type Op = unsafe extern "C" fn(*mut c_void) -> OSStatus;
type GetInfo = unsafe extern "C" fn(*mut c_void, u32, u32, u32, *mut u32, *mut u8) -> OSStatus;
type GetProp = unsafe extern "C" fn(*mut c_void, u32, u32, u32, *mut c_void, *mut u32) -> OSStatus;
type SetProp = unsafe extern "C" fn(*mut c_void, u32, u32, u32, *const c_void, u32) -> OSStatus;
type GetParam = unsafe extern "C" fn(*mut c_void, u32, u32, u32, *mut f32) -> OSStatus;
type SetParam = unsafe extern "C" fn(*mut c_void, u32, u32, u32, f32, u32) -> OSStatus;
type Schedule = unsafe extern "C" fn(*mut c_void, *const AudioUnitParameterEvent, u32) -> OSStatus;
type Reset = unsafe extern "C" fn(*mut c_void, u32, u32) -> OSStatus;
type Listener = unsafe extern "C" fn(*mut c_void, AudioComponentInstance, u32, u32, u32);
type AddListener = unsafe extern "C" fn(*mut c_void, u32, Listener, *mut c_void) -> OSStatus;
type RemoveListener = unsafe extern "C" fn(*mut c_void, u32, Listener) -> OSStatus;
type AddNotify = unsafe extern "C" fn(*mut c_void, AURenderCallback, *mut c_void) -> OSStatus;
type Render = unsafe extern "C" fn(*mut c_void, *mut u32, *const AudioTimeStamp, u32, u32, *mut AudioBufferList) -> OSStatus;

/// One open Audio Unit and the functions its interface hands out.
struct Unit {
    iface: *mut PlugInInterface,
    initialize: Op,
    uninitialize: Op,
    get_info: GetInfo,
    get_prop: GetProp,
    set_prop: SetProp,
    get_param: GetParam,
    set_param: SetParam,
    schedule: Schedule,
    reset: Reset,
    add_listener: AddListener,
    remove_listener: RemoveListener,
    add_notify: AddNotify,
    remove_notify: AddNotify,
    render: Render,
}

static HANDLE: u8 = 0;

fn handle() -> AudioComponentInstance {
    &HANDLE as *const u8 as *mut c_void
}

impl Unit {
    unsafe fn open(factory: FactoryFunction, desc: &AudioComponentDescription) -> Result<Unit, String> {
        let iface = factory(desc);
        if iface.is_null() {
            return Err("the factory returned null".to_string());
        }
        let find = |selector: i16| -> Result<unsafe extern "C" fn(), String> {
            ((*iface).lookup)(selector).ok_or_else(|| format!("no function for selector {}", selector))
        };
        macro_rules! typed {
            ($sel:expr, $t:ty) => {
                std::mem::transmute::<unsafe extern "C" fn(), $t>(find($sel)?)
            };
        }
        let unit = Unit {
            iface,
            initialize: typed!(SELECT_INITIALIZE, Op),
            uninitialize: typed!(SELECT_UNINITIALIZE, Op),
            get_info: typed!(SELECT_GET_PROPERTY_INFO, GetInfo),
            get_prop: typed!(SELECT_GET_PROPERTY, GetProp),
            set_prop: typed!(SELECT_SET_PROPERTY, SetProp),
            get_param: typed!(SELECT_GET_PARAMETER, GetParam),
            set_param: typed!(SELECT_SET_PARAMETER, SetParam),
            schedule: typed!(SELECT_SCHEDULE_PARAMETERS, Schedule),
            reset: typed!(SELECT_RESET, Reset),
            add_listener: typed!(SELECT_ADD_PROPERTY_LISTENER, AddListener),
            remove_listener: typed!(SELECT_REMOVE_PROPERTY_LISTENER, RemoveListener),
            add_notify: typed!(SELECT_ADD_RENDER_NOTIFY, AddNotify),
            remove_notify: typed!(SELECT_REMOVE_RENDER_NOTIFY, AddNotify),
            render: typed!(SELECT_RENDER, Render),
        };
        let status = ((*iface).open)(iface as *mut c_void, handle());
        if status != NO_ERR {
            return Err(format!("open returned {}", status));
        }
        Ok(unit)
    }

    fn this(&self) -> *mut c_void {
        self.iface as *mut c_void
    }

    unsafe fn close(self) -> OSStatus {
        ((*self.iface).close)(self.iface as *mut c_void)
    }

    unsafe fn info(&self, id: u32, scope: u32, element: u32) -> Result<(u32, bool), OSStatus> {
        let (mut size, mut writable) = (0u32, 0u8);
        match (self.get_info)(self.this(), id, scope, element, &mut size, &mut writable) {
            NO_ERR => Ok((size, writable != 0)),
            e => Err(e),
        }
    }

    unsafe fn get<T: Copy + Default>(&self, id: u32, scope: u32, element: u32) -> Result<T, OSStatus> {
        let mut value = T::default();
        let mut size = size_of::<T>() as u32;
        match (self.get_prop)(self.this(), id, scope, element, &mut value as *mut T as *mut c_void, &mut size) {
            NO_ERR if size as usize == size_of::<T>() => Ok(value),
            NO_ERR => Err(PARAM_ERR),
            e => Err(e),
        }
    }

    unsafe fn get_bytes(&self, id: u32, scope: u32, element: u32) -> Result<Vec<u8>, OSStatus> {
        let (size, _) = self.info(id, scope, element)?;
        let mut bytes = vec![0u8; size as usize];
        let mut io = size;
        match (self.get_prop)(self.this(), id, scope, element, bytes.as_mut_ptr() as *mut c_void, &mut io) {
            NO_ERR => Ok(bytes),
            e => Err(e),
        }
    }

    unsafe fn set<T>(&self, id: u32, scope: u32, element: u32, value: &T) -> OSStatus {
        (self.set_prop)(self.this(), id, scope, element, value as *const T as *const c_void, size_of::<T>() as u32)
    }

    unsafe fn param(&self, id: u32) -> Result<f32, OSStatus> {
        let mut v = 0f32;
        match (self.get_param)(self.this(), id, SCOPE_GLOBAL, 0, &mut v) {
            NO_ERR => Ok(v),
            e => Err(e),
        }
    }

    unsafe fn param_ids(&self) -> Result<Vec<u32>, OSStatus> {
        let bytes = self.get_bytes(PROP_PARAMETER_LIST, SCOPE_GLOBAL, 0)?;
        Ok(bytes.as_chunks::<4>().0.iter().map(|c| u32::from_ne_bytes(*c)).collect())
    }

    unsafe fn param_info(&self, id: u32) -> Result<AudioUnitParameterInfo, OSStatus> {
        let mut info: AudioUnitParameterInfo = std::mem::zeroed();
        let mut size = size_of::<AudioUnitParameterInfo>() as u32;
        match (self.get_prop)(
            self.this(),
            PROP_PARAMETER_INFO,
            SCOPE_GLOBAL,
            id,
            &mut info as *mut AudioUnitParameterInfo as *mut c_void,
            &mut size,
        ) {
            NO_ERR => Ok(info),
            e => Err(e),
        }
    }

    unsafe fn set_format(&self, scope: u32, channels: u32, rate: f64) -> OSStatus {
        self.set(PROP_STREAM_FORMAT, scope, 0, &AudioStreamBasicDescription::float_planar(rate, channels))
    }

    /// The same format on the input and the output, which is what an effect needs before it initializes.
    unsafe fn set_both(&self, channels: u32, rate: f64) -> OSStatus {
        match self.set_format(SCOPE_INPUT, channels, rate) {
            NO_ERR => self.set_format(SCOPE_OUTPUT, channels, rate),
            e => e,
        }
    }

    unsafe fn set_source(&self, source: &mut Source) -> OSStatus {
        self.set(
            PROP_SET_RENDER_CALLBACK,
            SCOPE_INPUT,
            0,
            &AURenderCallbackStruct {
                input_proc: Some(source_callback),
                input_proc_ref_con: source as *mut Source as *mut c_void,
            },
        )
    }

    unsafe fn clear_source(&self) -> OSStatus {
        self.set(
            PROP_SET_RENDER_CALLBACK,
            SCOPE_INPUT,
            0,
            &AURenderCallbackStruct {
                input_proc: None,
                input_proc_ref_con: null_mut(),
            },
        )
    }
}

/// A deterministic input signal, handed to the unit from its render callback.
struct Source {
    pulls: u32,
    /// Replace the host's buffers with these, as a source that owns its own memory does.
    swap_in: Option<Vec<Vec<f32>>>,
}

fn sample(channel: usize, index: usize, block: u32) -> f32 {
    let t = (block as usize * 4096 + index) as f32;
    0.2 * (t * 0.05 + channel as f32).sin() + 0.05 * (t * 0.31).sin()
}

unsafe extern "C" fn source_callback(
    ref_con: *mut c_void,
    _flags: *mut u32,
    _ts: *const AudioTimeStamp,
    bus: u32,
    frames: u32,
    io: *mut AudioBufferList,
) -> OSStatus {
    let s = &mut *(ref_con as *mut Source);
    if bus != 0 {
        return ERR_INVALID_ELEMENT;
    }
    let block = s.pulls;
    s.pulls += 1;
    let n = (*io).number_buffers as usize;
    let buffers = std::ptr::addr_of_mut!((*io).buffers) as *mut AudioBuffer;
    for c in 0..n {
        let b = &mut *buffers.add(c);
        let dst: *mut f32 = match s.swap_in.as_mut() {
            Some(own) => {
                let p = own[c].as_mut_ptr();
                b.data = p as *mut c_void;
                p
            }
            None => b.data as *mut f32,
        };
        for i in 0..frames as usize {
            *dst.add(i) = sample(c, i, block);
        }
    }
    NO_ERR
}

/// An `AudioBufferList` with room for `n` buffers.
struct List {
    memory: Vec<u64>,
    n: usize,
}

impl List {
    fn new(n: usize) -> List {
        let bytes = size_of::<AudioBufferList>() + n.saturating_sub(1) * size_of::<AudioBuffer>();
        let mut l = List {
            memory: vec![0u64; bytes.div_ceil(8)],
            n,
        };
        l.as_mut().number_buffers = n as u32;
        l
    }

    fn as_mut(&mut self) -> &mut AudioBufferList {
        unsafe { &mut *(self.memory.as_mut_ptr() as *mut AudioBufferList) }
    }

    fn ptr(&mut self) -> *mut AudioBufferList {
        self.memory.as_mut_ptr() as *mut AudioBufferList
    }

    fn buffer(&mut self, i: usize) -> &mut AudioBuffer {
        unsafe { &mut *(std::ptr::addr_of_mut!(self.as_mut().buffers) as *mut AudioBuffer).add(i) }
    }

    /// Points each buffer at memory the host owns, or leaves it empty for the unit to fill in.
    fn attach(&mut self, host_buffers: Option<&mut Vec<Vec<f32>>>, frames: usize) {
        let n = self.n;
        match host_buffers {
            Some(bufs) => {
                for (i, buf) in bufs.iter_mut().enumerate().take(n) {
                    let b = self.buffer(i);
                    b.number_channels = 1;
                    b.data_byte_size = (frames * 4) as u32;
                    b.data = buf.as_mut_ptr() as *mut c_void;
                }
            }
            None => {
                for i in 0..n {
                    let b = self.buffer(i);
                    b.number_channels = 1;
                    b.data_byte_size = (frames * 4) as u32;
                    b.data = null_mut();
                }
            }
        }
    }

    unsafe fn read(&mut self, i: usize, frames: usize) -> Vec<f32> {
        let b = self.buffer(i);
        std::slice::from_raw_parts(b.data as *const f32, frames).to_vec()
    }
}

fn timestamp(sample_time: f64) -> AudioTimeStamp {
    AudioTimeStamp {
        sample_time,
        flags: TIMESTAMP_SAMPLE_TIME_VALID,
        ..Default::default()
    }
}

fn rms(x: &[f32]) -> f32 {
    (x.iter().map(|v| (*v as f64) * (*v as f64)).sum::<f64>() / x.len().max(1) as f64).sqrt() as f32
}

unsafe fn render_blocks(
    unit: &Unit,
    channels: usize,
    frames: usize,
    blocks: u32,
    own_buffers: bool,
) -> Result<Vec<Vec<f32>>, OSStatus> {
    let mut source = Source { pulls: 0, swap_in: None };
    let status = unit.set_source(&mut source);
    if status != NO_ERR {
        return Err(status);
    }
    let mut host: Vec<Vec<f32>> = (0..channels).map(|_| vec![0.0f32; frames]).collect();
    let mut all: Vec<Vec<f32>> = vec![Vec::new(); channels];
    let mut list = List::new(channels);
    for b in 0..blocks {
        if own_buffers {
            list.attach(Some(&mut host), frames);
        } else {
            list.attach(None, frames);
        }
        let ts = timestamp((b as usize * frames) as f64);
        let mut flags = 0u32;
        let status = (unit.render)(unit.this(), &mut flags, &ts, 0, frames as u32, list.ptr());
        if status != NO_ERR {
            unit.clear_source();
            return Err(status);
        }
        for (c, a) in all.iter_mut().enumerate() {
            a.extend_from_slice(&list.read(c, frames));
        }
    }
    unit.clear_source();
    Ok(all)
}

#[derive(Default)]
struct Heard {
    calls: u32,
    pre: u32,
    post: u32,
}

unsafe extern "C" fn notify_callback(
    ref_con: *mut c_void,
    flags: *mut u32,
    _ts: *const AudioTimeStamp,
    _bus: u32,
    _frames: u32,
    _io: *mut AudioBufferList,
) -> OSStatus {
    let h = &mut *(ref_con as *mut Heard);
    h.calls += 1;
    if *flags & RENDER_PRE != 0 {
        h.pre += 1;
    }
    if *flags & RENDER_POST != 0 {
        h.post += 1;
    }
    NO_ERR
}

unsafe extern "C" fn listener_callback(ref_con: *mut c_void, _unit: AudioComponentInstance, _id: u32, _scope: u32, _element: u32) {
    *(ref_con as *mut u32) += 1;
}

unsafe fn cf_text(s: CFStringRef) -> String {
    string_from_cf(s).unwrap_or_default()
}

fn status_name(s: OSStatus) -> String {
    match s {
        NO_ERR => "no error".to_string(),
        ERR_INVALID_PROPERTY => "invalid property".to_string(),
        ERR_INVALID_PARAMETER => "invalid parameter".to_string(),
        ERR_INVALID_ELEMENT => "invalid element".to_string(),
        ERR_NO_CONNECTION => "no connection".to_string(),
        ERR_TOO_MANY_FRAMES => "too many frames".to_string(),
        ERR_FORMAT_NOT_SUPPORTED => "format not supported".to_string(),
        ERR_UNINITIALIZED => "uninitialized".to_string(),
        ERR_INVALID_SCOPE => "invalid scope".to_string(),
        ERR_PROPERTY_NOT_WRITABLE => "property not writable".to_string(),
        ERR_INVALID_PROPERTY_VALUE => "invalid property value".to_string(),
        ERR_INITIALIZED => "already initialized".to_string(),
        other => format!("status {}", other),
    }
}

/// Runs every check. `factory` is the function the component's `factoryFunction` names.
///
/// # Safety
/// `factory` must be a real Audio Unit factory.
pub unsafe fn run(factory: FactoryFunction, desc: AudioComponentDescription, r: &mut Report) {
    let unit = match Unit::open(factory, &desc) {
        Ok(u) => u,
        Err(e) => {
            r.check("open", false, e);
            return;
        }
    };
    r.check("open", true, format!("factory and open for {}/{}/{}", fourcc_string(desc.component_type), fourcc_string(desc.component_sub_type), fourcc_string(desc.component_manufacturer)));

    let selectors = [
        SELECT_INITIALIZE,
        SELECT_UNINITIALIZE,
        SELECT_GET_PROPERTY_INFO,
        SELECT_GET_PROPERTY,
        SELECT_SET_PROPERTY,
        SELECT_GET_PARAMETER,
        SELECT_SET_PARAMETER,
        SELECT_RESET,
        SELECT_ADD_PROPERTY_LISTENER,
        SELECT_REMOVE_PROPERTY_LISTENER,
        SELECT_RENDER,
        SELECT_ADD_RENDER_NOTIFY,
        SELECT_REMOVE_RENDER_NOTIFY,
        SELECT_SCHEDULE_PARAMETERS,
        SELECT_REMOVE_PROPERTY_LISTENER_WITH_USER_DATA,
    ];
    let all_found = selectors.iter().all(|s| ((*unit.iface).lookup)(*s).is_some());
    let unknown_refused = ((*unit.iface).lookup)(0x7777).is_none();
    r.check("selectors", all_found && unknown_refused, format!("{} selectors found, an unknown one is refused", selectors.len()));

    // Properties: what exists, how big it is, and the errors for what does not.
    let mut problems: Vec<String> = Vec::new();
    let expect = |p: &mut Vec<String>, what: &str, got: Result<(u32, bool), OSStatus>, size: u32, writable: bool| match got {
        Ok((s, w)) if s == size && w == writable => {}
        Ok((s, w)) => p.push(format!("{} gives size {} writable {}, wanted {} and {}", what, s, w, size, writable)),
        Err(e) => p.push(format!("{} failed with {}", what, status_name(e))),
    };
    expect(&mut problems, "class info", unit.info(PROP_CLASS_INFO, SCOPE_GLOBAL, 0), 8, true);
    expect(&mut problems, "sample rate in", unit.info(PROP_SAMPLE_RATE, SCOPE_INPUT, 0), 8, true);
    expect(&mut problems, "sample rate out", unit.info(PROP_SAMPLE_RATE, SCOPE_OUTPUT, 0), 8, true);
    expect(&mut problems, "stream format in", unit.info(PROP_STREAM_FORMAT, SCOPE_INPUT, 0), 40, true);
    expect(&mut problems, "stream format out", unit.info(PROP_STREAM_FORMAT, SCOPE_OUTPUT, 0), 40, true);
    expect(&mut problems, "element count", unit.info(PROP_ELEMENT_COUNT, SCOPE_GLOBAL, 0), 4, false);
    expect(&mut problems, "latency", unit.info(PROP_LATENCY, SCOPE_GLOBAL, 0), 8, false);
    expect(&mut problems, "tail time", unit.info(PROP_TAIL_TIME, SCOPE_GLOBAL, 0), 8, false);
    expect(&mut problems, "maximum frames", unit.info(PROP_MAXIMUM_FRAMES_PER_SLICE, SCOPE_GLOBAL, 0), 4, true);
    expect(&mut problems, "bypass", unit.info(PROP_BYPASS_EFFECT, SCOPE_GLOBAL, 0), 4, true);
    expect(&mut problems, "render callback", unit.info(PROP_SET_RENDER_CALLBACK, SCOPE_INPUT, 0), 16, true);
    expect(&mut problems, "connection", unit.info(PROP_MAKE_CONNECTION, SCOPE_INPUT, 0), 16, true);
    expect(&mut problems, "host callbacks", unit.info(PROP_HOST_CALLBACKS, SCOPE_GLOBAL, 0), 40, true);
    let errors = [
        ("a stream format in the global scope", unit.info(PROP_STREAM_FORMAT, SCOPE_GLOBAL, 0), ERR_INVALID_SCOPE),
        ("a stream format on element 1", unit.info(PROP_STREAM_FORMAT, SCOPE_INPUT, 1), ERR_INVALID_ELEMENT),
        ("an unknown property", unit.info(9999, SCOPE_GLOBAL, 0), ERR_INVALID_PROPERTY),
    ];
    for (what, got, want) in errors {
        if got.err() != Some(want) {
            problems.push(format!("{} should give {}", what, status_name(want)));
        }
    }
    let read_only = unit.set(PROP_LATENCY, SCOPE_GLOBAL, 0, &0.5f64);
    if read_only != ERR_PROPERTY_NOT_WRITABLE {
        problems.push(format!("writing latency gave {}", status_name(read_only)));
    }
    let counts = [SCOPE_GLOBAL, SCOPE_INPUT, SCOPE_OUTPUT].map(|s| unit.get::<u32>(PROP_ELEMENT_COUNT, s, 0).unwrap_or(99));
    if counts != [1, 1, 1] {
        problems.push(format!("element counts are {:?}", counts));
    }
    r.check(
        "properties",
        problems.is_empty(),
        if problems.is_empty() { "sizes, writability and the errors for bad scopes, elements and ids are right".to_string() } else { problems.join("; ") },
    );

    // Channel configurations and the stream format.
    let infos = unit.get_bytes(PROP_SUPPORTED_NUM_CHANNELS, SCOPE_GLOBAL, 0).unwrap_or_default();
    let supported: Vec<(i16, i16)> = infos
        .as_chunks::<4>()
        .0
        .iter()
        .map(|c| (i16::from_ne_bytes([c[0], c[1]]), i16::from_ne_bytes([c[2], c[3]])))
        .collect();
    let any = supported == [(-1, -1)];
    let min = if any { 1 } else { supported.iter().map(|s| s.0).min().unwrap_or(1) as u32 };
    let max = if any { 64 } else { supported.iter().map(|s| s.0).max().unwrap_or(1) as u32 };
    let default_format: Result<AudioStreamBasicDescription, _> = unit.get(PROP_STREAM_FORMAT, SCOPE_OUTPUT, 0);
    let mut format_problems: Vec<String> = Vec::new();
    match default_format {
        Ok(f) => {
            let planar = f.format_id == FORMAT_LINEAR_PCM
                && f.format_flags & FORMAT_FLAG_IS_NON_INTERLEAVED != 0
                && f.format_flags & FORMAT_FLAG_IS_FLOAT != 0
                && f.bits_per_channel == 32
                && f.channels_per_frame >= min
                && f.channels_per_frame <= max;
            if !planar {
                format_problems.push(format!("the default format is not 32-bit float planar in range: {:?}", f));
            }
        }
        Err(e) => format_problems.push(format!("reading the format failed: {}", status_name(e))),
    }
    let channels_now = default_format.map(|f| f.channels_per_frame).unwrap_or(2);
    let interleaved = AudioStreamBasicDescription {
        format_flags: FORMAT_FLAGS_NATIVE_FLOAT_PACKED,
        bytes_per_frame: 4 * channels_now,
        bytes_per_packet: 4 * channels_now,
        ..AudioStreamBasicDescription::float_planar(44100.0, channels_now)
    };
    if unit.set(PROP_STREAM_FORMAT, SCOPE_INPUT, 0, &interleaved) != ERR_FORMAT_NOT_SUPPORTED {
        format_problems.push("an interleaved format was not refused".to_string());
    }
    if unit.set_format(SCOPE_INPUT, 0, 44100.0) != ERR_FORMAT_NOT_SUPPORTED {
        format_problems.push("zero channels was not refused".to_string());
    }
    if max < 64 && unit.set_format(SCOPE_INPUT, max + 1, 44100.0) != ERR_FORMAT_NOT_SUPPORTED {
        format_problems.push("more channels than the plugin takes was not refused".to_string());
    }
    if unit.set_both(min, 96000.0) != NO_ERR {
        format_problems.push("a valid format was refused".to_string());
    } else {
        let output: Result<AudioStreamBasicDescription, _> = unit.get(PROP_STREAM_FORMAT, SCOPE_OUTPUT, 0);
        match output {
            Ok(f) if f.channels_per_frame == min && f.sample_rate == 96000.0 => {}
            other => format_problems.push(format!("the format read back is wrong: {:?}", other.map(|f| (f.channels_per_frame, f.sample_rate)))),
        }
    }
    // An effect that is given a different number of channels in and out must not initialize.
    if max > min {
        unit.set_format(SCOPE_INPUT, min, 44100.0);
        unit.set_format(SCOPE_OUTPUT, min + 1, 44100.0);
        let status = (unit.initialize)(unit.this());
        if status != ERR_FORMAT_NOT_SUPPORTED {
            format_problems.push(format!("initializing with {} channels in and {} out gave {}", min, min + 1, status_name(status)));
            (unit.uninitialize)(unit.this());
        }
    }
    // Restore a plain stereo (or smallest) setup for the checks that follow.
    let base = if min <= 2 && max >= 2 { 2 } else { min };
    unit.set_both(base, 48000.0);
    r.check(
        "stream_format",
        format_problems.is_empty() && !supported.is_empty(),
        if format_problems.is_empty() {
            format!("{} channel configuration(s) supported, from {} to {} channels; bad formats refused, and a different count in and out does not initialize", supported.len(), min, max)
        } else {
            format_problems.join("; ")
        },
    );
    let channels = base as usize;

    // Parameters.
    let ids = unit.param_ids().unwrap_or_default();
    let mut problems: Vec<String> = Vec::new();
    for (i, id) in ids.iter().enumerate() {
        if ids[..i].contains(id) {
            problems.push(format!("id {} appears twice", id));
        }
        let info = match unit.param_info(*id) {
            Ok(i) => i,
            Err(e) => {
                problems.push(format!("info for {} failed with {}", id, status_name(e)));
                continue;
            }
        };
        let name = cf_text(info.cf_name_string);
        if name.is_empty() {
            problems.push(format!("parameter {} has no name", id));
        }
        let ok_range = info.min_value.is_finite() && info.max_value.is_finite() && info.min_value < info.max_value && info.default_value >= info.min_value && info.default_value <= info.max_value;
        if !ok_range {
            problems.push(format!("'{}' has a bad range {} to {} with default {}", name, info.min_value, info.max_value, info.default_value));
        }
        if info.flags & PARAM_FLAG_IS_READABLE == 0 {
            problems.push(format!("'{}' is not readable", name));
        }
        match unit.param(*id) {
            Ok(v) if (v - info.default_value).abs() < 1e-6 => {}
            other => problems.push(format!("'{}' starts at {:?}, not its default {}", name, other, info.default_value)),
        }
        if info.unit == UNIT_INDEXED {
            let strings = unit.get::<CFArrayRef>(PROP_PARAMETER_VALUE_STRINGS, SCOPE_GLOBAL, *id);
            match strings {
                Ok(a) if !a.is_null() => {
                    let n = CFArrayGetCount(a);
                    if n != info.max_value as isize + 1 {
                        problems.push(format!("'{}' lists {} strings for {} values", name, n, info.max_value as isize + 1));
                    }
                    CFRelease(a);
                }
                other => problems.push(format!("'{}' has no value strings: {:?}", name, other.err().map(status_name))),
            }
        }
        // Text from a value and back.
        let mut sv = AudioUnitParameterStringFromValue {
            param_id: *id,
            in_value: &info.default_value,
            out_string: null(),
        };
        let mut size = size_of::<AudioUnitParameterStringFromValue>() as u32;
        let status = (unit.get_prop)(unit.this(), PROP_PARAMETER_STRING_FROM_VALUE, SCOPE_GLOBAL, 0, &mut sv as *mut _ as *mut c_void, &mut size);
        let text = if status == NO_ERR { cf_text(sv.out_string) } else { String::new() };
        if status != NO_ERR || text.is_empty() {
            problems.push(format!("'{}' gave no text: {}", name, status_name(status)));
        } else {
            let mut vs = AudioUnitParameterValueFromString {
                param_id: *id,
                in_string: sv.out_string,
                out_value: f32::NAN,
            };
            let mut size = size_of::<AudioUnitParameterValueFromString>() as u32;
            let status = (unit.get_prop)(unit.this(), PROP_PARAMETER_VALUE_FROM_STRING, SCOPE_GLOBAL, 0, &mut vs as *mut _ as *mut c_void, &mut size);
            if status != NO_ERR || (vs.out_value - info.default_value).abs() > (info.max_value - info.min_value) * 0.01 {
                problems.push(format!("'{}' text {:?} reads back as {} ({})", name, text, vs.out_value, status_name(status)));
            }
        }
        if !sv.out_string.is_null() {
            CFRelease(sv.out_string);
        }
        let mut nm = AudioUnitParameterNameInfo {
            id: *id,
            desired_length: -1,
            out_name: null(),
        };
        let mut size = size_of::<AudioUnitParameterNameInfo>() as u32;
        if (unit.get_prop)(unit.this(), PROP_PARAMETER_ID_NAME, SCOPE_GLOBAL, 0, &mut nm as *mut _ as *mut c_void, &mut size) != NO_ERR || cf_text(nm.out_name).is_empty() {
            problems.push(format!("'{}' has no id name", name));
        }
        if !nm.out_name.is_null() {
            CFRelease(nm.out_name);
        }
        if !info.cf_name_string.is_null() && info.flags & PARAM_FLAG_CF_NAME_RELEASE != 0 {
            CFRelease(info.cf_name_string);
        }
    }
    let wrong_id = unit.param(0x7fff_ffff).err();
    let wrong_scope = (unit.get_param)(unit.this(), ids.first().copied().unwrap_or(0), SCOPE_INPUT, 0, &mut 0f32);
    if wrong_id != Some(ERR_INVALID_PARAMETER) {
        problems.push("an unknown parameter id was not refused".to_string());
    }
    if wrong_scope != ERR_INVALID_SCOPE {
        problems.push("a parameter in the input scope was not refused".to_string());
    }
    r.check(
        "parameters",
        problems.is_empty() && !ids.is_empty(),
        if problems.is_empty() { format!("{} parameters: unique ids, names, ranges, defaults, value strings and text round trips, bad ids and scopes refused", ids.len()) } else { problems.join("; ") },
    );

    // Set and read back a writable parameter, through SetParameter and through ScheduleParameters.
    let writable: Vec<u32> = ids
        .iter()
        .copied()
        .filter(|id| unit.param_info(*id).map(|i| i.flags & PARAM_FLAG_IS_WRITABLE != 0).unwrap_or(false))
        .collect();
    if let Some(id) = writable.first().copied() {
        let info = unit.param_info(id).unwrap();
        let direct = (unit.set_param)(unit.this(), id, SCOPE_GLOBAL, 0, info.max_value, 0);
        let read = unit.param(id).unwrap_or(f32::NAN);
        let clamped = (unit.set_param)(unit.this(), id, SCOPE_GLOBAL, 0, info.max_value + 1000.0, 0);
        let after = unit.param(id).unwrap_or(f32::NAN);
        let nan = (unit.set_param)(unit.this(), id, SCOPE_GLOBAL, 0, f32::NAN, 0);
        r.check(
            "parameter_set",
            direct == NO_ERR && (read - info.max_value).abs() < 1e-6 && clamped == NO_ERR && (after - info.max_value).abs() < 1e-3 && nan != NO_ERR,
            format!("'{}' set to its maximum reads {}, an out-of-range value is kept in range at {}, NaN is refused", cf_text(info.cf_name_string), read, after),
        );
        (unit.set_param)(unit.this(), id, SCOPE_GLOBAL, 0, info.default_value, 0);
        if !info.cf_name_string.is_null() {
            CFRelease(info.cf_name_string);
        }
    } else {
        r.check("parameter_set", true, "no writable parameters");
    }

    // Rendering before initialization is an error.
    let mut list = List::new(channels);
    let mut host: Vec<Vec<f32>> = (0..channels).map(|_| vec![0.0f32; 64]).collect();
    list.attach(Some(&mut host), 64);
    let ts = timestamp(0.0);
    let mut flags = 0u32;
    let early = (unit.render)(unit.this(), &mut flags, &ts, 0, 64, list.ptr());
    r.check("render_before_initialize", early == ERR_UNINITIALIZED, format!("render gives {}", status_name(early)));

    // Property listeners hear a format change while the unit is still open.
    let mut heard_format: u32 = 0;
    (unit.add_listener)(unit.this(), PROP_STREAM_FORMAT, listener_callback, &mut heard_format as *mut u32 as *mut c_void);
    unit.set_both(base, 48000.0);
    let during = heard_format;
    (unit.remove_listener)(unit.this(), PROP_STREAM_FORMAT, listener_callback);
    unit.set_both(base, 48000.0);
    r.check("property_listeners", during >= 1 && heard_format == during, format!("a listener heard {} change(s) and none after it was removed", during));

    // Initialization.
    let init = (unit.initialize)(unit.this());
    let again = (unit.initialize)(unit.this());
    let format_locked = unit.set_format(SCOPE_INPUT, base, 48000.0);
    let frames_locked = unit.set(PROP_MAXIMUM_FRAMES_PER_SLICE, SCOPE_GLOBAL, 0, &512u32);
    let rate_locked = unit.set(PROP_SAMPLE_RATE, SCOPE_OUTPUT, 0, &44100.0f64);
    r.check(
        "initialize",
        init == NO_ERR && again == NO_ERR && format_locked == ERR_INITIALIZED && frames_locked == ERR_INITIALIZED && rate_locked == ERR_INITIALIZED,
        format!("initialize twice is fine, and changing the format, frame limit or rate afterwards gives {}", status_name(format_locked)),
    );

    // Latency and tail time are in seconds.
    let latency: Result<f64, _> = unit.get(PROP_LATENCY, SCOPE_GLOBAL, 0);
    let tail: Result<f64, _> = unit.get(PROP_TAIL_TIME, SCOPE_GLOBAL, 0);
    r.check(
        "latency_tail",
        latency.as_ref().is_ok_and(|v| v.is_finite() && *v >= 0.0) && tail.as_ref().is_ok_and(|v| v.is_finite() && *v >= 0.0),
        format!("latency {:?} s, tail {:?} s", latency.ok(), tail.ok()),
    );

    // Rendering: through a render callback into the host's buffers, and into buffers the unit supplies.
    let frames = 256usize;
    let with_host = render_blocks(&unit, channels, frames, 8, true);
    let with_own = render_blocks(&unit, channels, frames, 8, false);
    match (&with_host, &with_own) {
        (Ok(a), Ok(b)) => {
            let finite = a.iter().chain(b.iter()).all(|c| c.iter().all(|v| v.is_finite()));
            let input_level = 20.0 * rms(&(0..frames).map(|i| sample(0, i, 0)).collect::<Vec<_>>()).log10();
            let output_level = 20.0 * rms(&a[0][..frames]).max(1e-12).log10();
            r.check("render", finite, format!("8 blocks of {} frames on {} channels, channel 1 reads {:+.1} dB against {:+.1} dB in", frames, channels, output_level, input_level));
        }
        (Err(e), _) | (_, Err(e)) => {
            r.check("render", false, format!("render failed: {}", status_name(*e)));
        }
    }

    // The same audio comes out whether the host or the unit owns the buffers. A plugin with memory, such
    // as a filter, would differ on a second pass, so each pass gets a fresh unit.
    {
        let fresh = || -> Option<Unit> {
            let u = Unit::open(factory, &desc).ok()?;
            u.set_both(base, 48000.0);
            ((u.initialize)(u.this()) == NO_ERR).then_some(u)
        };
        match (fresh(), fresh()) {
            (Some(a), Some(b)) => {
                let host_owned = render_blocks(&a, channels, frames, 4, true);
                let unit_owned = render_blocks(&b, channels, frames, 4, false);
                let same = match (&host_owned, &unit_owned) {
                    (Ok(x), Ok(y)) => x.iter().zip(y.iter()).all(|(p, q)| p.iter().zip(q).all(|(s, t)| (s - t).abs() < 1e-5)),
                    _ => false,
                };
                r.check(
                    "render_buffer_ownership",
                    same,
                    if same { "the same audio whether the host or the unit owns the buffers" } else { "the audio differs when the unit supplies the buffers" },
                );
                a.close();
                b.close();
            }
            _ => {
                r.check("render_buffer_ownership", false, "could not open two fresh units");
            }
        }
    }

    // A source that brings its own buffers is processed in place.
    {
        let mut swap = Source {
            pulls: 0,
            swap_in: Some((0..channels).map(|_| vec![0.0f32; frames]).collect()),
        };
        unit.set_source(&mut swap);
        let mut list = List::new(channels);
        list.attach(None, frames);
        let ts = timestamp(0.0);
        let mut flags = 0u32;
        let status = (unit.render)(unit.this(), &mut flags, &ts, 0, frames as u32, list.ptr());
        let finite = status == NO_ERR && (0..channels).all(|c| list.read(c, frames).iter().all(|v| v.is_finite()));
        unit.clear_source();
        r.check("render_with_a_source_that_swaps_buffers", finite, format!("render gives {}", status_name(status)));
    }

    // Errors the render call has to give.
    {
        let mut source = Source { pulls: 0, swap_in: None };
        unit.set_source(&mut source);
        let mut list = List::new(channels);
        let mut host: Vec<Vec<f32>> = (0..channels).map(|_| vec![0.0f32; 8192]).collect();
        list.attach(Some(&mut host), 4096);
        let ts = timestamp(0.0);
        let mut flags = 0u32;
        let max_frames: u32 = unit.get(PROP_MAXIMUM_FRAMES_PER_SLICE, SCOPE_GLOBAL, 0).unwrap_or(1156);
        let bus = (unit.render)(unit.this(), &mut flags, &ts, 1, 64, list.ptr());
        let too_many = (unit.render)(unit.this(), &mut flags, &ts, 0, max_frames + 1, list.ptr());
        let mut wrong = List::new(channels + 1);
        wrong.attach(None, 64);
        let wrong_count = (unit.render)(unit.this(), &mut flags, &ts, 0, 64, wrong.ptr());
        unit.clear_source();
        let no_source = (unit.render)(unit.this(), &mut flags, &ts, 0, 64, list.ptr());
        let last: Result<i32, _> = unit.get(PROP_LAST_RENDER_ERROR, SCOPE_GLOBAL, 0);
        r.check(
            "render_errors",
            bus == ERR_INVALID_ELEMENT && too_many == ERR_TOO_MANY_FRAMES && wrong_count == ERR_FORMAT_NOT_SUPPORTED && no_source == ERR_NO_CONNECTION && last.is_ok(),
            format!("bus 1 gives {}, {} frames gives {}, the wrong buffer count gives {}, no source gives {}", status_name(bus), max_frames + 1, status_name(too_many), status_name(wrong_count), status_name(no_source)),
        );
    }

    // Render notifications arrive before and after each render, and stop when removed.
    {
        let mut heard = Heard::default();
        (unit.add_notify)(unit.this(), notify_callback, &mut heard as *mut Heard as *mut c_void);
        let _ = render_blocks(&unit, channels, 64, 3, true);
        let (pre, post) = (heard.pre, heard.post);
        (unit.remove_notify)(unit.this(), notify_callback, &mut heard as *mut Heard as *mut c_void);
        let _ = render_blocks(&unit, channels, 64, 2, true);
        r.check(
            "render_notifications",
            pre == 3 && post == 3 && heard.calls == 6,
            format!("3 renders gave {} before and {} after, and a removed notify heard nothing more", pre, post),
        );
    }

    // Bypass hands the input through untouched.
    {
        unit.set(PROP_BYPASS_EFFECT, SCOPE_GLOBAL, 0, &1u32);
        let out = render_blocks(&unit, channels, frames, 1, true);
        unit.set(PROP_BYPASS_EFFECT, SCOPE_GLOBAL, 0, &0u32);
        match out {
            Ok(o) => {
                let same = (0..frames).all(|i| (o[0][i] - sample(0, i, 0)).abs() < 1e-7);
                r.check("bypass", same, if same { "bypass passes the input through exactly" } else { "bypass changed the audio" });
            }
            Err(e) => {
                r.check("bypass", false, format!("render failed: {}", status_name(e)));
            }
        }
    }

    // A scheduled parameter lands at the next render.
    if let Some(id) = writable.first().copied() {
        let info = unit.param_info(id).unwrap();
        let target = info.max_value;
        let mut values = [0u32; 4];
        values[0] = 0;
        values[1] = target.to_bits();
        let event = AudioUnitParameterEvent {
            scope: SCOPE_GLOBAL,
            element: 0,
            parameter: id,
            event_type: PARAM_EVENT_IMMEDIATE,
            event_values: values,
        };
        (unit.set_param)(unit.this(), id, SCOPE_GLOBAL, 0, info.default_value, 0);
        let status = (unit.schedule)(unit.this(), &event, 1);
        let _ = render_blocks(&unit, channels, 64, 1, true);
        let landed = unit.param(id).unwrap_or(f32::NAN);
        let bad = AudioUnitParameterEvent { parameter: 0x7fff_ffff, ..event };
        let refused = (unit.schedule)(unit.this(), &bad, 1);
        r.check(
            "schedule_parameters",
            status == NO_ERR && (landed - target).abs() < 1e-5 && refused == ERR_INVALID_PARAMETER,
            format!("an immediate event for '{}' reads {} after the next render, an unknown id gives {}", cf_text(info.cf_name_string), landed, status_name(refused)),
        );
        (unit.set_param)(unit.this(), id, SCOPE_GLOBAL, 0, info.default_value, 0);
        if !info.cf_name_string.is_null() {
            CFRelease(info.cf_name_string);
        }
    } else {
        r.check("schedule_parameters", true, "no writable parameters");
    }

    // Saved state moves to a second unit, and a state for a different plugin is refused.
    {
        if let Some(id) = writable.first().copied() {
            let info = unit.param_info(id).unwrap();
            (unit.set_param)(unit.this(), id, SCOPE_GLOBAL, 0, info.max_value, 0);
            if !info.cf_name_string.is_null() {
                CFRelease(info.cf_name_string);
            }
        }
        let saved = unit.get::<CFDictionaryRef>(PROP_CLASS_INFO, SCOPE_GLOBAL, 0);
        match (saved, Unit::open(factory, &desc)) {
            (Ok(dict), Ok(second)) if !dict.is_null() => {
                let loaded = second.set(PROP_CLASS_INFO, SCOPE_GLOBAL, 0, &dict);
                let mut same = true;
                for id in unit.param_ids().unwrap_or_default() {
                    if unit.param(id).ok() != second.param(id).ok() {
                        same = false;
                    }
                }
                let other = AudioComponentDescription {
                    component_sub_type: fourcc(b"Nope"),
                    ..desc
                };
                let third = Unit::open(factory, &other);
                let refused = match &third {
                    Ok(t) => t.set(PROP_CLASS_INFO, SCOPE_GLOBAL, 0, &dict),
                    Err(_) => PARAM_ERR,
                };
                let not_a_dictionary = cf_string("not a dictionary");
                let junk = second.set(PROP_CLASS_INFO, SCOPE_GLOBAL, 0, &not_a_dictionary);
                CFRelease(not_a_dictionary);
                CFRelease(dict);
                second.close();
                if let Ok(t) = third {
                    t.close();
                }
                r.check(
                    "class_info",
                    loaded == NO_ERR && same && refused == ERR_INVALID_PROPERTY_VALUE && junk == ERR_INVALID_PROPERTY_VALUE,
                    format!("state loads into a second unit with every value equal ({}), a state for another subtype gives {}, a non-dictionary gives {}", same, status_name(refused), status_name(junk)),
                );
            }
            (saved, second) => {
                r.check("class_info", false, format!("could not read or reload the state: {:?} {:?}", saved.err().map(status_name), second.err()));
            }
        }
        if let Some(id) = writable.first().copied() {
            let info = unit.param_info(id).unwrap();
            (unit.set_param)(unit.this(), id, SCOPE_GLOBAL, 0, info.default_value, 0);
            if !info.cf_name_string.is_null() {
                CFRelease(info.cf_name_string);
            }
        }
    }

    // The present preset can be read and set, and its name travels with the saved state.
    {
        let mut problems: Vec<String> = Vec::new();
        let mut now: AUPreset = std::mem::zeroed();
        let mut size = size_of::<AUPreset>() as u32;
        let read = (unit.get_prop)(unit.this(), PROP_PRESENT_PRESET, SCOPE_GLOBAL, 0, &mut now as *mut AUPreset as *mut c_void, &mut size);
        if read != NO_ERR {
            problems.push(format!("reading the present preset gave {}", status_name(read)));
        } else if !now.preset_name.is_null() {
            CFRelease(now.preset_name);
        }
        let name = cf_string("Roundtrip");
        let set = unit.set(PROP_PRESENT_PRESET, SCOPE_GLOBAL, 0, &AUPreset { preset_number: -1, preset_name: name });
        CFRelease(name);
        if set != NO_ERR {
            problems.push(format!("setting the present preset gave {}", status_name(set)));
        }
        match unit.get::<CFDictionaryRef>(PROP_CLASS_INFO, SCOPE_GLOBAL, 0) {
            Ok(dict) if !dict.is_null() => {
                let key = cf_string("name");
                let value = CFDictionaryGetValue(dict, key);
                CFRelease(key);
                let kept = string_from_cf(value);
                if kept.as_deref() != Some("Roundtrip") {
                    problems.push(format!("the state carries the preset name {:?}", kept));
                }
                CFRelease(dict);
            }
            other => problems.push(format!("reading the state gave {:?}", other.err().map(status_name))),
        }
        r.check(
            "present_preset",
            problems.is_empty(),
            if problems.is_empty() { "the present preset reads and sets, and its name is kept in the saved state".to_string() } else { problems.join("; ") },
        );
    }

    // Reset keeps the unit initialized.
    let reset = (unit.reset)(unit.this(), SCOPE_GLOBAL, 0);
    let still = render_blocks(&unit, channels, 64, 1, true).is_ok();
    r.check("reset", reset == NO_ERR && still, "reset returns, and the unit still renders");

    // A different channel count while stopped: the parameter list follows, and rendering works.
    {
        (unit.uninitialize)(unit.this());
        let other = if channels == min as usize && max > min { min + 1 } else { min };
        let before = unit.param_ids().map(|v| v.len()).unwrap_or(0);
        let mut heard_list: u32 = 0;
        (unit.add_listener)(unit.this(), PROP_PARAMETER_LIST, listener_callback, &mut heard_list as *mut u32 as *mut c_void);
        let set = unit.set_both(other, 44100.0);
        let after = unit.param_ids().map(|v| v.len()).unwrap_or(0);
        (unit.remove_listener)(unit.this(), PROP_PARAMETER_LIST, listener_callback);
        let init = (unit.initialize)(unit.this());
        let out = render_blocks(&unit, other as usize, 128, 4, true);
        let finite = out.as_ref().is_ok_and(|o| o.iter().all(|c| c.iter().all(|v| v.is_finite())));
        r.check(
            "reconfigure",
            set == NO_ERR && init == NO_ERR && finite && (other as usize == channels || heard_list >= 1 || before == after),
            format!("{} channels at 44100 Hz: parameter list {} to {} entries (heard {} change(s)), initialize {}, render {}", other, before, after, heard_list, status_name(init), if finite { "finite" } else { "failed" }),
        );
    }

    (unit.uninitialize)(unit.this());
    let closed = unit.close();
    r.check("close", closed == NO_ERR, "uninitialize and close completed");
}

/// The factory-name argument for a built component is passed as text. Used by the verifier binary.
pub fn description(subtype: &str, manufacturer: &str) -> Option<AudioComponentDescription> {
    let code = |s: &str| -> Option<u32> {
        let b = s.as_bytes();
        (b.len() == 4 && b.iter().all(|c| c.is_ascii_graphic())).then(|| fourcc(&[b[0], b[1], b[2], b[3]]))
    };
    Some(AudioComponentDescription {
        component_type: TYPE_EFFECT,
        component_sub_type: code(subtype)?,
        component_manufacturer: code(manufacturer)?,
        component_flags: 0,
        component_flags_mask: 0,
    })
}
