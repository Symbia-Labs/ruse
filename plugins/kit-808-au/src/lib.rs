// The MUSE 808 as an Audio Unit Music Device (aumu). Audio Units exist on macOS only.
#![cfg(target_os = "macos")]

muse_au::export_au! {
    plugin: plugin_kit_808::Kit808,
    factory: Kit808Factory,
}
