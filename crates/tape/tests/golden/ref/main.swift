// Writes the Swift tape stage's output for fixed inputs, so the Rust port can be held to it.
// Run ref/run.sh. Each case leaves NAME.in.f32 and NAME.out.f32 (little-endian Float32, mono,
// 44.1 kHz, 128-frame blocks, transport clock starting at 0) next to this folder.
import Foundation

setvbuf(stdout, nil, _IONBF, 0)
let fs: Float = 44100
let count = 16384
let block = 128
let dir = CommandLine.arguments.count > 1 ? CommandLine.arguments[1] : "."

func signal(_ amp: Float) -> [Float] {
    (0..<count).map { i in
        let t = Float(i) / fs
        return amp * (0.6 * sin(2 * Float.pi * 220 * t) + 0.3 * sin(2 * Float.pi * 3100 * t) + 0.1 * sin(2 * Float.pi * 9000 * t))
    }
}

func render(_ stage: TapeStage, _ x: [Float]) -> [Float] {
    var y = x
    let io = UnsafeMutablePointer<UnsafeMutablePointer<Float>>.allocate(capacity: 1)
    defer { io.deallocate() }
    y.withUnsafeMutableBufferPointer { buf in
        var at = 0
        while at < buf.count {
            let n = min(block, buf.count - at)
            io[0] = buf.baseAddress! + at
            stage.process(io, 1, n, sampleTime: Double(at))
            at += n
        }
    }
    return y
}

func save(_ name: String, _ suffix: String, _ v: [Float]) {
    let data = v.withUnsafeBufferPointer { Data(buffer: $0) }
    try! data.write(to: URL(fileURLWithPath: "\(dir)/\(name).\(suffix).f32"))
}

struct Case {
    let name: String
    let settings: TapeSettings
    let amp: Float
}

var cases: [Case] = []
cases.append(Case(name: "default", settings: TapeSettings(), amp: 0.5))

var b = TapeSettings()
b.formula = 0; b.speed = 0; b.bias = -0.5; b.inputDB = 6; b.wow = 0; b.flutter = 0; b.hissDB = -120
cases.append(Case(name: "type1_slow_underbiased", settings: b, amp: 0.9))

var c = TapeSettings()
c.formula = 2; c.speed = 3; c.bias = 0.5; c.wow = 0.2; c.flutter = 0.1; c.hissDB = -60; c.format = 5
cases.append(Case(name: "type4_15ips_wow", settings: c, amp: 0.7))

var d = TapeSettings()
d.speed = 4; d.formula = 1; d.inputDB = 12; d.wow = 0; d.flutter = 0; d.hissDB = -120
cases.append(Case(name: "hot_30ips", settings: d, amp: 1.0))

var e = TapeSettings()
e.speed = 2; e.formula = 1; e.format = 7; e.hissDB = -60; e.link = false
cases.append(Case(name: "reel_7_5ips_hiss", settings: e, amp: 0.3))

// Nearly linear, so rounding differences cannot be amplified: the two stages should agree closely.
var q = TapeSettings()
q.wow = 0; q.flutter = 0; q.hissDB = -120
cases.append(Case(name: "linear_quiet", settings: q, amp: 0.003))

for k in cases {
    let x = signal(k.amp)
    let y = render(TapeStage(fs: fs, settings: k.settings), x)
    save(k.name, "in", x)
    save(k.name, "out", y)
    // The numbers the settings work out to, so a mismatch can be traced to design or to processing.
    let cfg = TapeConfig(k.settings, fs: fs)
    var lines: [String] = []
    func add(_ key: String, _ v: Float) { lines.append("\(key) \(v)") }
    add("drive", cfg.drive); add("makeup", cfg.makeup); add("output", cfg.output); add("hiss", cfg.hiss)
    add("noisePole", cfg.noisePole); add("noiseLow", cfg.noiseLow); add("noiseNorm", cfg.noiseNorm)
    add("bleed", cfg.bleed); add("bleedPole", cfg.bleedPole); add("k", cfg.k); add("c", cfg.c)
    add("fieldScale", cfg.fieldScale); add("emphasisB0", cfg.emphasisB0); add("emphasisB1", cfg.emphasisB1)
    add("emphasisA1", cfg.emphasisA1); add("baseDelay", cfg.baseDelay)
    for i in 0..<Tape.firTaps { add("playback\(i)", cfg.playback[i]) }
    for i in 0..<10 { add("biquad\(i)", cfg.biquads[i]) }
    for i in 0..<Tape.halfTaps { add("half\(i)", TapeFilters.halfBandTaps[i]) }
    try! lines.joined(separator: "\n").write(toFile: "\(dir)/\(k.name).cfg.txt", atomically: true, encoding: .utf8)
    let peak = y.map { abs($0) }.max() ?? 0
    print("\(k.name): \(y.count) samples, peak \(peak)")
}
