//! Exports a RUSE plugin as an Audio Unit (v2 effect). macOS only.
//!
//! The plugin's crate stays free of unsafe code. A small companion crate calls [`export_au!`], which
//! defines the factory function the component's `Info.plist` names. The `package_plugin` tool in
//! `ruse-mcp` writes that crate, builds it and assembles the `.component` bundle.
//!
//! ```ignore
//! ruse_au::export_au! {
//!     plugin: my_plugin::MyPlugin,
//!     factory: MyPluginFactory,
//! }
//! ```
#![cfg(target_os = "macos")]

pub mod check;
pub mod sys;
pub mod wrapper;

/// Defines the factory function for a plugin type that implements `ruse_core::Plugin` and `Default`.
/// The function's name goes in the component's `factoryFunction` entry.
#[macro_export]
macro_rules! export_au {
    (plugin: $plugin:ty, factory: $factory:ident $(,)?) => {
        #[no_mangle]
        pub unsafe extern "C" fn $factory(
            desc: *const $crate::sys::AudioComponentDescription,
        ) -> *mut $crate::sys::PlugInInterface {
            $crate::wrapper::create::<$plugin>(desc)
        }
    };
}
