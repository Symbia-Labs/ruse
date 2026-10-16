# Packaging and installing

`package_plugin` turns a plugin into bundles a host can load: a `.clap` for CLAP hosts and a `.component` for Audio Unit hosts such as Logic and GarageBand.

## What it does

1. Refuses a plugin that fails a [gate](gates.md).
2. Saves the settings it was given in `manifest.json`, so later calls need only the plugin's name.
3. Generates a packaging crate for each format beside the plugin, such as `plugins/tilt-clap` and `plugins/tilt-au`. The plugin's own crate stays free of unsafe code.
4. Builds each one in release mode and assembles the bundle in the plugin's `dist/` folder.
5. Signs the bundle ad hoc, which is enough for it to load on the Mac that built it.
6. Loads the bundle the way a host does and runs its checks: 17 for CLAP and 25 for an Audio Unit. They cover the entry point or factory, channel layouts, parameters and their text, processing, parameter changes, saving and loading state, and changing the channel count.
7. When the call sets `install` to true, and only after every check has passed, copies each bundle into `~/Library/Audio/Plug-Ins/CLAP` or `~/Library/Audio/Plug-Ins/Components`, then runs Apple's `auval` on the Audio Unit.

## Settings

| Setting | Meaning |
|---|---|
| `vendor` | The maker a host shows beside the plugin. Required the first time |
| `formats` | `clap`, `au` or both. Defaults to `clap` |
| `version` | Defaults to 0.0.1 |
| `id` | The plugin's identity in reverse-domain style. Defaults to the vendor and the plugin name joined by a dot |
| `features` | CLAP feature names such as `audio-effect`, `filter` or `delay`. Defaults to `audio-effect` |
| `url` | A web address for the plugin or its maker |
| `au_subtype`, `au_manufacturer` | The Audio Unit's four-character codes. Made from the plugin and vendor names unless given. Two plugins on one Mac must not share both codes |
| `install` | Copy the bundles into the plug-in folders and run `auval`. Not saved |

For example:

```
muse-mcp call package_plugin '{"name":"tilt","vendor":"Your Name","formats":["au","clap"],"install":true}'
```

After an install, a host may need to rescan its plug-ins. In Logic, the Plug-in Manager does this.

## How installing works

When the workspace sits beside a `scripts/install-app.sh`, `package_plugin` hands each bundle to that script, so plugins install the same way as anything else built on the machine. Otherwise it removes the installed copy and copies the new bundle into the plug-in folder.

## Instruments

`package_plugin` packages effects only for now. The MUSE 808 example is an Audio Unit instrument, built and installed by `examples/kit-808-au/bundle.sh`. Set `SKIP_INSTALL=1` to build it without installing.

## Sharing a plugin

An ad hoc signature is enough on your own Mac. A plugin copied to another Mac also needs a Developer ID signature from a paid Apple Developer account, and notarization, or Gatekeeper refuses to load it. MUSE does not do either step yet. Apple's `codesign` and `notarytool` do both, on the bundles in `dist/`.

## Other platforms

Packaging and installing are built for macOS. `package_plugin` has code paths for a CLAP plugin on Windows and Linux, but nobody has built or tested one there yet. VST3 and Audio Unit v3 are not built.
