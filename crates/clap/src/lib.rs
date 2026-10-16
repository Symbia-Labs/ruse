//! Exports a RUSE plugin as a CLAP plugin.
//!
//! The plugin's crate stays free of unsafe code. A small companion crate calls [`export_clap!`], which
//! defines the `clap_entry` symbol a host looks for. The `package_plugin` tool in `ruse-mcp` writes that
//! crate, builds it and assembles the bundle.
//!
//! ```ignore
//! ruse_clap::export_clap! {
//!     plugin: my_plugin::MyPlugin,
//!     id: "com.example.my-plugin",
//!     name: "My Plugin",
//!     vendor: "Example",
//!     url: "https://example.com",
//!     version: "1.0.0",
//!     description: "What it does.",
//!     features: ["audio-effect"],
//! }
//! ```

pub mod abi;
pub mod wrapper;

pub use wrapper::{Config, Registry};

/// Defines the `clap_entry` symbol for a plugin type that implements `ruse_core::Plugin` and `Default`.
#[macro_export]
macro_rules! export_clap {
    (
        plugin: $plugin:ty,
        id: $id:literal,
        name: $name:literal,
        vendor: $vendor:literal,
        url: $url:literal,
        version: $version:literal,
        description: $description:literal,
        features: [$($feature:literal),* $(,)?] $(,)?
    ) => {
        #[doc(hidden)]
        pub mod __ruse_clap_export {
            #![allow(non_upper_case_globals)]
            use super::*;
            use ::std::ffi::{c_char, c_void};
            use $crate::abi;
            use $crate::wrapper::{Config, Registry};

            static CONFIG: Config = Config {
                id: $id,
                name: $name,
                vendor: $vendor,
                url: $url,
                version: $version,
                description: $description,
                features: &[$($feature),*],
            };
            static REGISTRY: Registry = Registry::new(&CONFIG);

            unsafe extern "C" fn factory_count(_factory: *const abi::PluginFactory) -> u32 {
                1
            }

            unsafe extern "C" fn factory_descriptor(
                _factory: *const abi::PluginFactory,
                index: u32,
            ) -> *const abi::PluginDescriptor {
                $crate::wrapper::factory_descriptor(&REGISTRY, index)
            }

            unsafe extern "C" fn factory_create(
                _factory: *const abi::PluginFactory,
                host: *const abi::Host,
                id: *const c_char,
            ) -> *const abi::Plugin {
                $crate::wrapper::create_plugin::<$plugin>(&REGISTRY, host, id)
            }

            static FACTORY: abi::PluginFactory = abi::PluginFactory {
                get_plugin_count: factory_count,
                get_plugin_descriptor: factory_descriptor,
                create_plugin: factory_create,
            };

            unsafe extern "C" fn entry_init(path: *const c_char) -> bool {
                $crate::wrapper::entry_init(&REGISTRY, path)
            }

            unsafe extern "C" fn entry_deinit() {}

            unsafe extern "C" fn entry_get_factory(id: *const c_char) -> *const c_void {
                $crate::wrapper::entry_get_factory(id, &FACTORY)
            }

            #[no_mangle]
            #[used]
            pub static clap_entry: abi::PluginEntry = abi::PluginEntry {
                clap_version: abi::CLAP_VERSION,
                init: entry_init,
                deinit: entry_deinit,
                get_factory: entry_get_factory,
            };
        }
    };
}
