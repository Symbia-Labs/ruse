use muse_gain::Gain;
use muse_host::cli;
use muse_host::render::{self, Options, Setting};
use muse_host::{measure, signal};

#[test]
fn six_db_of_gain_reads_six_db_more() {
    let input = signal::generate(&signal::Signal::Sine { hz: 1000.0, db: -12.0 }, 48000.0, 24000, 2);
    let opts = Options {
        settings: vec![Setting {
            id: "gain".to_string(),
            channel: None,
            value: 6.0,
        }],
        ..Options::default()
    };
    let r = render::render(&mut Gain::new(), &input, &opts).unwrap();
    let change = measure::rms_db(&r.output[1]) - measure::rms_db(&input[1]);
    assert!((change - 6.0).abs() < 0.01, "changed by {}", change);
}

#[test]
fn a_channel_count_the_layout_refuses_is_an_error() {
    let input = signal::generate(&signal::Signal::Silence, 48000.0, 128, 65);
    let err = render::render(&mut Gain::new(), &input, &Options::default()).err().unwrap();
    assert!(err.contains("65 channels"), "{}", err);
}

#[test]
fn an_unknown_parameter_is_an_error_that_names_it() {
    let input = signal::generate(&signal::Signal::Silence, 48000.0, 128, 1);
    let opts = Options {
        settings: vec![Setting {
            id: "volume".to_string(),
            channel: None,
            value: 1.0,
        }],
        ..Options::default()
    };
    let err = render::render(&mut Gain::new(), &input, &opts).err().unwrap();
    assert!(err.contains("volume"), "{}", err);
}

#[test]
fn the_command_line_renders_reports_and_writes_a_file() {
    let out = std::env::temp_dir().join("muse-host-cli-test.wav");
    let args: Vec<String> = [
        "--signal",
        "sine:1000:-12",
        "--channels",
        "2",
        "--seconds",
        "0.5",
        "--set",
        "gain=-6",
        "--out",
        out.to_str().unwrap(),
    ]
    .iter()
    .map(|s| s.to_string())
    .collect();
    let text = cli::run("gain", &Gain::new, args).unwrap();
    println!("{}", text);
    assert!(text.contains("gain = -6.00 dB"), "{}", text);
    assert!(text.contains("rms change -6.0 dB"), "{}", text);
    let w = muse_host::wav::read(&out).unwrap();
    assert_eq!(w.channels.len(), 2);
    assert_eq!(w.channels[0].len(), 24000);
}

#[test]
fn describe_lists_the_parameters_as_json() {
    let text = cli::run("gain", &Gain::new, vec!["--describe".to_string()]).unwrap();
    assert!(text.contains("\"id\":\"gain\""), "{}", text);
}

#[test]
fn a_bad_option_is_refused_by_name() {
    let err = cli::run("gain", &Gain::new, vec!["--loud".to_string()]).err().unwrap();
    assert!(err.contains("--loud"));
}
